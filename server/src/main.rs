//! lucid-dream-server: read tools for the shared memory store, over MCP.
//!
//!   lucid-dream-server --root <memory dir> [--bind 127.0.0.1:7878]   Streamable HTTP at /mcp
//!   lucid-dream-server --root <memory dir> --stdio                   stdio, for a local client
//!
//! Environment: LUCID_ROOT, LUCID_BIND, LUCID_TOKEN (flags win over the environment).

mod mcp;
mod store;

use std::io::{BufRead, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::Value;

use store::Store;

const DEFAULT_BIND: &str = "127.0.0.1:7878";

struct Config {
    root: PathBuf,
    bind: String,
    token: Option<String>,
    stdio: bool,
}

struct App {
    store: Store,
    token: Option<String>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("lucid-dream-server: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let config = parse_args()?;
    let store = Store::open(&config.root)
        .map_err(|e| format!("cannot open memory root {}: {e}", config.root.display()))?;

    if config.stdio {
        return serve_stdio(&store).map_err(|e| format!("stdio: {e}"));
    }

    let addr: SocketAddr = config
        .bind
        .parse()
        .map_err(|_| format!("bad bind address: {}", config.bind))?;
    if !addr.ip().is_loopback() && config.token.is_none() {
        return Err(format!(
            "refusing to listen on {addr} without a token; set LUCID_TOKEN or bind to 127.0.0.1"
        ));
    }
    eprintln!("lucid-dream-server: serving {} at http://{addr}/mcp", store.root().display());
    let app = Arc::new(App { store, token: config.token });
    tokio::runtime::Runtime::new()
        .map_err(|e| e.to_string())?
        .block_on(serve_http(addr, app))
        .map_err(|e| format!("http: {e}"))
}

fn parse_args() -> Result<Config, String> {
    let env = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
    let mut root = env("LUCID_ROOT").map(PathBuf::from);
    let mut bind = env("LUCID_BIND").unwrap_or_else(|| DEFAULT_BIND.to_string());
    let mut stdio = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--root" => root = Some(PathBuf::from(value("--root")?)),
            "--bind" => bind = value("--bind")?,
            "--stdio" => stdio = true,
            "-h" | "--help" => {
                println!(
                    "usage: lucid-dream-server --root <memory dir> [--bind {DEFAULT_BIND}] [--stdio]\n\
                     env:   LUCID_ROOT, LUCID_BIND, LUCID_TOKEN (bearer token; required off loopback)"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(Config {
        root: root.ok_or("no memory root; pass --root or set LUCID_ROOT")?,
        bind,
        token: env("LUCID_TOKEN"),
        stdio,
    })
}

/// stdio transport: one JSON-RPC message per line.
fn serve_stdio(store: &Store) -> std::io::Result<()> {
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => mcp::handle(store, &msg),
            Err(_) => Some(mcp::error(Value::Null, -32700, "parse error")),
        };
        if let Some(reply) = reply {
            writeln!(out, "{reply}")?;
            out.flush()?;
        }
    }
    Ok(())
}

async fn serve_http(addr: SocketAddr, app: Arc<App>) -> std::io::Result<()> {
    let router = Router::new()
        .route("/mcp", post(post_mcp))
        .route("/health", get(|| async { "ok" }))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await
}

/// Streamable HTTP: each POST carries one message and gets a plain JSON reply.
async fn post_mcp(State(app): State<Arc<App>>, headers: HeaderMap, body: Bytes) -> Response {
    if !origin_allowed(&headers) {
        return (StatusCode::FORBIDDEN, "origin not allowed").into_response();
    }
    if !authorized(&headers, app.token.as_deref()) {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
            "missing or wrong bearer token",
        )
            .into_response();
    }
    let Ok(msg) = serde_json::from_slice::<Value>(&body) else {
        let reply = mcp::error(Value::Null, -32700, "parse error");
        return (StatusCode::BAD_REQUEST, Json(reply)).into_response();
    };
    match mcp::handle(&app.store, &msg) {
        Some(reply) => Json(reply).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

fn authorized(headers: &HeaderMap, token: Option<&str>) -> bool {
    let Some(token) = token else {
        return true;
    };
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|given| constant_time_eq(given.as_bytes(), token.as_bytes()))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Agents send no Origin header. A browser does, so only a local page may call
/// the server; this blocks DNS-rebinding attacks from websites.
fn origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    let rest = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .unwrap_or("");
    let host = match rest.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => rest.split([':', '/']).next().unwrap_or(""),
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(name: header::HeaderName, value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(name, value.parse().unwrap());
        h
    }

    #[test]
    fn token_must_match_when_set() {
        assert!(authorized(&HeaderMap::new(), None));
        assert!(!authorized(&HeaderMap::new(), Some("s3")));
        assert!(authorized(&with(header::AUTHORIZATION, "Bearer s3"), Some("s3")));
        assert!(!authorized(&with(header::AUTHORIZATION, "Bearer s4"), Some("s3")));
        assert!(!authorized(&with(header::AUTHORIZATION, "s3"), Some("s3")));
    }

    #[test]
    fn only_local_origins_pass() {
        assert!(origin_allowed(&HeaderMap::new()));
        assert!(origin_allowed(&with(header::ORIGIN, "http://localhost:3000")));
        assert!(origin_allowed(&with(header::ORIGIN, "http://[::1]:3000")));
        assert!(!origin_allowed(&with(header::ORIGIN, "https://evil.example")));
        assert!(!origin_allowed(&with(header::ORIGIN, "http://localhost.evil.example")));
    }
}
