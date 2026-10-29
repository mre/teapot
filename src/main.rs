use std::collections::BTreeMap;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{FromRequest, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use clap::Parser;
use serde::Deserialize;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Set output directory for recording requests
    #[arg(short, long)]
    record: Option<PathBuf>,

    /// Set port number for the server
    #[arg(short, long, env = "PORT", default_value_t = 8080)]
    port: u16,

    /// Read configuration from this file (defaults to config.toml if present)
    #[arg(short, long)]
    config: Option<PathBuf>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Config {
    #[serde(default)]
    route: Vec<Route>,
    #[serde(default)]
    fallback: Fallback,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            route: vec![Route {
                matches: "/".into(),
                response_string: Some("OK".into()),
                response_path: None,
                template_vars: BTreeMap::new(),
            }],
            fallback: Fallback::default(),
        }
    }
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Route {
    matches: String,
    response_string: Option<String>,
    response_path: Option<PathBuf>,
    #[serde(default)]
    template_vars: BTreeMap<String, String>,
}

#[derive(Deserialize, Debug)]
#[serde(default, deny_unknown_fields)]
struct Fallback {
    response_code: String,
}

impl Default for Fallback {
    fn default() -> Self {
        Self {
            response_code: "teapot".into(),
        }
    }
}

impl Fallback {
    fn status(&self) -> io::Result<StatusCode> {
        if self.response_code == "teapot" {
            return Ok(StatusCode::IM_A_TEAPOT);
        }
        self.response_code
            .parse::<u16>()
            .ok()
            .and_then(|code| StatusCode::from_u16(code).ok())
            .filter(|status| !status.is_informational())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "fallback.response_code must be 'teapot' or a final HTTP status code (200–999)",
                )
            })
    }
}

async fn load_config(path: Option<&Path>) -> io::Result<Config> {
    let config_path = path.unwrap_or_else(|| Path::new("config.toml"));
    let source = match fs::read_to_string(config_path).await {
        Ok(source) => source,
        Err(error) if path.is_none() && error.kind() == io::ErrorKind::NotFound => {
            return Ok(Config::default());
        }
        Err(error) => {
            return Err(io::Error::new(
                error.kind(),
                format!("Could not read {}: {error}", config_path.display()),
            ));
        }
    };
    let mut config: Config = toml::from_str(&source).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Invalid configuration in {}: {error}",
                config_path.display()
            ),
        )
    })?;
    let base = config_path.parent().unwrap_or_else(|| Path::new("."));
    for route in &mut config.route {
        if let Some(path) = &mut route.response_path {
            *path = base.join(&*path);
        }
    }
    Ok(config)
}

fn build_app(config: Config, record: Option<PathBuf>) -> io::Result<Router> {
    let status = config.fallback.status()?;
    let mut app = Router::new().fallback(move || async move {
        if matches!(
            status,
            StatusCode::NO_CONTENT | StatusCode::RESET_CONTENT | StatusCode::NOT_MODIFIED
        ) {
            // An unknown size prevents Axum from adding Content-Length to 204/304.
            let body = Body::from_stream(futures_util::stream::empty::<
                Result<Bytes, std::convert::Infallible>,
            >());
            (status, body).into_response()
        } else {
            (
                status,
                status.canonical_reason().unwrap_or("Unknown status"),
            )
                .into_response()
        }
    });
    let mut paths = matchit::Router::new();

    for route in config.route {
        if !route.matches.starts_with('/')
            || route
                .matches
                .split('/')
                .any(|segment| segment.starts_with([':', '*']))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Invalid route {:?}: use a leading '/' and brace-style captures, e.g. /{{name}}",
                    route.matches
                ),
            ));
        }
        paths.insert(&route.matches, ()).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Invalid route {:?}: {error}", route.matches),
            )
        })?;
        if route.response_string.is_some() == route.response_path.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Route {:?} must set exactly one of response_string or response_path",
                    route.matches
                ),
            ));
        }
        let path = route.matches.clone();
        let route = Arc::new(route);
        app = app.route(
            &path,
            any(move || {
                let route = Arc::clone(&route);
                async move { respond(&route).await }
            }),
        );
    }

    if let Some(path) = record {
        app = app.layer(middleware::from_fn_with_state(path, record_request));
    }
    Ok(app)
}

async fn respond(route: &Route) -> Response {
    let mut content = if let Some(path) = &route.response_path {
        match fs::read_to_string(path).await {
            Ok(content) => content,
            Err(error) => {
                eprintln!("Could not read response file {}: {error}", path.display());
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Could not read response file",
                )
                    .into_response();
            }
        }
    } else {
        route.response_string.clone().unwrap_or_default()
    };
    for (key, value) in &route.template_vars {
        content = content.replace(&format!("{{{{{key}}}}}"), value);
    }
    content.into_response()
}

async fn record_request(
    State(directory): State<PathBuf>,
    request: Request,
    next: Next,
) -> Response {
    let (parts, body) = request.into_parts();
    // Bytes enforces Axum's default 2 MiB limit before buffering a recording.
    let body = match Bytes::from_request(Request::from_parts(parts.clone(), body), &()).await {
        Ok(body) => body,
        Err(rejection) => return rejection.into_response(),
    };
    let mut data = format!("{} {} {:?}\r\n", parts.method, parts.uri, parts.version).into_bytes();
    for (name, value) in &parts.headers {
        data.extend_from_slice(name.as_str().as_bytes());
        data.extend_from_slice(b": ");
        data.extend_from_slice(value.as_bytes());
        data.extend_from_slice(b"\r\n");
    }
    data.extend_from_slice(b"\r\n");
    data.extend_from_slice(&body);

    if let Err(error) = write_recording(&directory, &data).await {
        eprintln!(
            "Could not record request in {}: {error}",
            directory.display()
        );
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not record request",
        )
            .into_response();
    }
    next.run(Request::from_parts(parts, Body::from(body))).await
}

async fn write_recording(directory: &Path, data: &[u8]) -> io::Result<()> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let path = directory.join(format!(
        "request_{timestamp}_{}_{sequence}.txt",
        std::process::id()
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await?;
    file.write_all(data).await?;
    file.flush().await
}

async fn run(args: Args) -> io::Result<()> {
    let config = load_config(args.config.as_deref()).await?;
    if let Some(directory) = &args.record {
        fs::create_dir_all(directory).await?;
    }
    let app = build_app(config, args.record)?;
    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    let listener = TcpListener::bind(addr).await?;
    println!("Listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                eprintln!("Could not listen for Ctrl-C: {error}");
            }
        })
        .await
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Args::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("teapot: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests;
