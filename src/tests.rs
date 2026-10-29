use super::*;
use axum::body::to_bytes;
use tower::ServiceExt;

async fn send(app: Router, method: &str, uri: &str, body: impl Into<Body>) -> Response {
    app.oneshot(
        Request::builder()
            .method(method)
            .uri(uri)
            .header("x-test", "record-me")
            .body(body.into())
            .unwrap(),
    )
    .await
    .unwrap()
}

async fn body(response: Response) -> Bytes {
    to_bytes(response.into_body(), usize::MAX).await.unwrap()
}

fn app_from(source: &str) -> io::Result<Router> {
    build_app(toml::from_str(source).unwrap(), None)
}

#[test]
fn cli_validates_ports() {
    assert!(Args::try_parse_from(["teapot", "--port", "65536"]).is_err());
    assert!(Args::try_parse_from(["teapot", "--port", "-1"]).is_err());
    assert_eq!(
        Args::try_parse_from(["teapot", "--port", "0"])
            .unwrap()
            .port,
        0
    );
}

#[tokio::test]
async fn default_routes_support_all_methods_and_head() {
    let app = build_app(Config::default(), None).unwrap();
    for method in ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
        let response = send(app.clone(), method, "/", "hello").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body(response).await, "OK");
    }
    let response = send(app.clone(), "HEAD", "/", "").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body(response).await.is_empty());
    let response = send(app, "GET", "/missing", "").await;
    assert_eq!(response.status(), StatusCode::IM_A_TEAPOT);
    assert_eq!(body(response).await, "I'm a teapot");
}

#[tokio::test]
async fn configured_fallback_is_used() {
    let app = app_from("[fallback]\nresponse_code = '404'").unwrap();
    let response = send(app, "POST", "/missing", "").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(body(response).await, "Not Found");
}

#[tokio::test]
async fn bodyless_fallback_statuses_have_empty_bodies() {
    for code in [204, 205, 304] {
        let app = app_from(&format!("[fallback]\nresponse_code = '{code}'")).unwrap();
        for method in ["GET", "HEAD"] {
            let response = send(app.clone(), method, "/", "").await;
            assert_eq!(response.status().as_u16(), code);
            if code == 204 || code == 304 {
                assert!(!response.headers().contains_key("content-length"));
            }
            assert!(body(response).await.is_empty());
        }
    }
}

#[test]
fn invalid_routes_return_errors_instead_of_panicking() {
    for routes in [
        vec!["missing-slash"],
        vec!["/:name"],
        vec!["/*path"],
        vec!["/{name"],
        vec!["/{*path}/suffix"],
        vec!["/", "/"],
        vec!["/{first}", "/{second}"],
    ] {
        let source = routes
            .iter()
            .map(|path| format!("[[route]]\nmatches = '{path}'\nresponse_string = 'OK'\n"))
            .collect::<String>();
        assert!(app_from(&source).is_err(), "Accepted {routes:?}");
    }
    assert!(app_from("[[route]]\nmatches = '/static'\nresponse_string = 'OK'\n[[route]]\nmatches = '/{name}'\nresponse_string = 'OK'").is_ok());
}

#[test]
fn invalid_configuration_is_rejected() {
    for code in ["invalid", "99", "100", "1000"] {
        assert!(app_from(&format!("[fallback]\nresponse_code = '{code}'")).is_err());
    }
    assert!(app_from("[[route]]\nmatches = '/'").is_err());
    assert!(
        app_from("[[route]]\nmatches = '/'\nresponse_string = 'OK'\nresponse_path = 'test.txt'")
            .is_err()
    );
    assert!(toml::from_str::<Config>("routes = []").is_err());
}

#[tokio::test]
async fn sample_config_supports_captures_and_templates() {
    let app = app_from(include_str!("../config.toml")).unwrap();
    let response = send(app, "GET", "/alice", "").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body(response).await, "Hello world");
}

#[tokio::test]
async fn response_files_are_relative_to_config_and_render_templates() {
    let directory = tempfile::tempdir().unwrap();
    let config_path = directory.path().join("server.toml");
    fs::write(directory.path().join("hello.txt"), "Hello {{name}}!")
        .await
        .unwrap();
    fs::write(
        &config_path,
        "[[route]]\nmatches = '/'\nresponse_path = 'hello.txt'\ntemplate_vars = { name = 'Rust' }",
    )
    .await
    .unwrap();
    let config = load_config(Some(&config_path)).await.unwrap();
    let app = build_app(config, None).unwrap();
    let response = send(app, "GET", "/", "").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body(response).await, "Hello Rust!");
}

#[tokio::test]
async fn missing_response_files_return_an_error() {
    let directory = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.route[0].response_string = None;
    config.route[0].response_path = Some(directory.path().join("missing.txt"));
    let response = send(build_app(config, None).unwrap(), "GET", "/", "").await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn explicit_missing_and_malformed_configs_return_context() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.toml");
    let error = load_config(Some(&path)).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(error.to_string().contains("server.toml"));
    fs::write(&path, "not valid TOML").await.unwrap();
    let error = load_config(Some(&path)).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("server.toml"));
}

#[tokio::test]
async fn recordings_include_metadata_and_binary_bodies_for_routes_and_fallback() {
    let directory = tempfile::tempdir().unwrap();
    let app = build_app(Config::default(), Some(directory.path().to_owned())).unwrap();
    let payload = vec![0, 255, b'\n'];
    let (first, second) = tokio::join!(
        send(app.clone(), "POST", "/?test=1", payload.clone()),
        send(app, "PUT", "/missing?test=1", payload.clone()),
    );
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(second.status(), StatusCode::IM_A_TEAPOT);

    let mut entries = fs::read_dir(directory.path()).await.unwrap();
    let mut recordings = Vec::new();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        let recording = fs::read(entry.path()).await.unwrap();
        assert!(recording.ends_with(&payload));
        let text = String::from_utf8_lossy(&recording);
        assert!(text.contains("x-test: record-me\r\n"));
        assert!(text.contains("\r\n\r\n"));
        recordings.push(recording);
    }
    assert_eq!(recordings.len(), 2);
    assert!(
        recordings
            .iter()
            .any(|r| r.starts_with(b"POST /?test=1 HTTP/1.1\r\n"))
    );
    assert!(
        recordings
            .iter()
            .any(|r| r.starts_with(b"PUT /missing?test=1 HTTP/1.1\r\n"))
    );
}

#[tokio::test]
async fn oversized_recordings_are_rejected_without_writing_files() {
    let directory = tempfile::tempdir().unwrap();
    let app = build_app(Config::default(), Some(directory.path().to_owned())).unwrap();
    let response = send(app, "POST", "/", vec![0; 2 * 1024 * 1024 + 1]).await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(
        fs::read_dir(directory.path())
            .await
            .unwrap()
            .next_entry()
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn recording_failures_are_reported_to_the_client() {
    let directory = tempfile::tempdir().unwrap();
    let app = build_app(Config::default(), Some(directory.path().join("missing"))).unwrap();
    let response = send(app, "POST", "/", "hello").await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body(response).await, "Could not record request");
}
