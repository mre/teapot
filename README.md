# 🫖 teapot

A small dummy server for testing HTTP clients. Configure a few responses, send
requests, and optionally record what arrives.

## Installation

Download a binary for Linux, macOS, or Windows from the
[latest release](https://github.com/mre/teapot/releases/latest).

To build from source, use Rust 1.88 or newer:

```sh
cargo install --locked --git https://github.com/mre/teapot.git
```

## Usage

```sh
teapot
```

Teapot listens only on `127.0.0.1`, on port `8080` by default. It serves all HTTP
methods. Without a config file, `/` responds with `OK` and unmatched paths return
`418 I'm a teapot`. Press Ctrl-C for a graceful shutdown.

Set the port with `PORT` or `--port` (`-p`). The flag takes precedence over the
environment variable; the value must fit in a `u16` (`0`–`65535`).

```sh
PORT=9000 teapot
teapot --port 9000
```

## Configuration

Teapot looks for `config.toml` in the current directory. If it's absent, the
defaults above apply. Use `--config` (`-c`) to select a file explicitly; a missing
explicit file is an error.

Save this as `config.toml`:

```toml
[[route]]
matches = "/"
response_string = "OK"

[[route]]
matches = "/{usr}"
response_string = "Hello {{name}}"
template_vars = { name = "world" }

[fallback]
response_code = "teapot"
```

Then start the server:

```sh
teapot --config config.toml
```

From another terminal:

```sh
curl http://127.0.0.1:8080/
curl http://127.0.0.1:8080/alice
```

These return `OK` and `Hello world`. Template variables are literal `{{key}}`
substitutions, not values captured from the URL.

Routes use Axum 0.8 syntax: `/hello/{name}`, not `/hello/:name`. When migrating an
older config, replace colon-prefixed parameters with braces.

Each route needs either `response_string` for inline text or `response_path` for
a UTF-8 text file, not both. File paths are relative to the config file, and
`template_vars` substitutions work with either source. File read errors return
HTTP `500`.

The fallback status defaults to `418`. Set `response_code` to `"teapot"` or a
numeric string such as `"404"`; the response body is the status's reason phrase
(for example, `Not Found`). Statuses `204`, `205`, and `304` return an empty body.

## Recording requests

Use `--record` (`-r`) to save requests. Teapot creates the directory if needed.

```sh
teapot --record ./recordings
```

Each request, including requests handled by the fallback, gets a unique
`request_*.txt` file containing the method, URI (including the query string),
HTTP version, headers, a blank line, and the raw body bytes. Despite the `.txt`
extension, the body may be binary. This is an application-level record, not an
exact capture of the bytes sent over the network.

With recording enabled, request bodies are limited to 2 MiB. Larger bodies are
rejected with HTTP `413` and are not recorded. Recording failures return HTTP `500`.

**Recordings can contain sensitive data**, including authorization headers,
cookies, query parameters, and request bodies. Keep them out of version control
and delete them when you're done.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```

CI runs these checks, tests on Linux, macOS, and Windows, and checks Rust 1.88
compatibility. To release, update the version in `Cargo.toml` and `Cargo.lock`,
commit, and push a matching `vX.Y.Z` tag. The release workflow runs CI, builds
native binaries, checks their versions, and publishes archives with SHA-256
checksums to GitHub Releases. It does not publish to crates.io.

## Credits

Based on [mockserver](https://github.com/takakawa/mockserver) by
[takakawa](https://github.com/takakawa).
