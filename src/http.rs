//! Streamable HTTP transport: each POST to /mcp carries one message and gets a
//! plain JSON reply. The bearer token decides which agent is speaking.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::Value;

use crate::mcp;
use crate::store::Store;
use crate::tokens::Tokens;

/// The voice of a caller on this machine when no tokens have been issued.
pub const LOCAL: &str = "local";

pub struct App {
    pub store: Store,
    pub tokens: PathBuf,
    pub loopback: bool,
}

pub fn serve(addr: SocketAddr, app: App) -> Result<(), String> {
    let tokens = Tokens::load(&app.tokens).map_err(|e| format!("cannot read tokens: {e}"))?;
    if !app.loopback && tokens.is_empty() {
        return Err(format!(
            "refusing to listen on {addr} with no tokens; run `lucid-dream token add <agent>` \
             first, or bind to 127.0.0.1"
        ));
    }
    eprintln!(
        "lucid-dream: serving {} at http://{addr}/mcp",
        app.store.root().display()
    );
    let router = Router::new()
        .route("/mcp", post(post_mcp))
        .route("/health", get(|| async { "ok" }))
        .with_state(Arc::new(app));
    tokio::runtime::Runtime::new()
        .map_err(|e| e.to_string())?
        .block_on(async {
            let listener = tokio::net::TcpListener::bind(addr).await?;
            axum::serve(listener, router).await
        })
        .map_err(|e: std::io::Error| format!("http: {e}"))
}

async fn post_mcp(State(app): State<Arc<App>>, headers: HeaderMap, body: Bytes) -> Response {
    if !origin_allowed(&headers) {
        return (StatusCode::FORBIDDEN, "origin not allowed").into_response();
    }
    let Some(who) = identify(&app, &headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
            "missing or wrong bearer token",
        )
            .into_response();
    };
    let Ok(msg) = serde_json::from_slice::<Value>(&body) else {
        let reply = mcp::error(Value::Null, -32700, "parse error");
        return (StatusCode::BAD_REQUEST, Json(reply)).into_response();
    };
    match mcp::handle(&app.store, &who, &msg) {
        Some(reply) => Json(reply).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

/// Which agent is calling. The token file is re-read each time, so a token
/// added or replaced takes effect without a restart.
fn identify(app: &App, headers: &HeaderMap) -> Option<String> {
    let tokens = Tokens::load(&app.tokens).ok()?;
    if tokens.is_empty() {
        return app.loopback.then(|| LOCAL.to_string());
    }
    let given = headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")?;
    tokens.identify(given).map(str::to_string)
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
    use crate::store::tests::Temp;
    use crate::tokens;

    fn with(name: header::HeaderName, value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(name, value.parse().unwrap());
        h
    }

    fn app(t: &Temp, loopback: bool) -> App {
        App {
            store: t.store(),
            tokens: tokens::path(&t.0),
            loopback,
        }
    }

    #[test]
    fn without_tokens_only_loopback_is_let_in() {
        let t = Temp::new();
        assert_eq!(
            identify(&app(&t, true), &HeaderMap::new()).as_deref(),
            Some(LOCAL)
        );
        assert_eq!(identify(&app(&t, false), &HeaderMap::new()), None);
    }

    #[test]
    fn a_token_names_the_caller() {
        let t = Temp::new();
        let token = Tokens::add(&tokens::path(&t.0), "codex").unwrap();
        let a = app(&t, true);
        let bearer = with(header::AUTHORIZATION, &format!("Bearer {token}"));
        assert_eq!(identify(&a, &bearer).as_deref(), Some("codex"));
        assert_eq!(identify(&a, &HeaderMap::new()), None);
        assert_eq!(
            identify(&a, &with(header::AUTHORIZATION, "Bearer ld_nope")),
            None
        );
        assert_eq!(identify(&a, &with(header::AUTHORIZATION, &token)), None);
    }

    #[test]
    fn only_local_origins_pass() {
        assert!(origin_allowed(&HeaderMap::new()));
        assert!(origin_allowed(&with(
            header::ORIGIN,
            "http://localhost:3000"
        )));
        assert!(origin_allowed(&with(header::ORIGIN, "http://[::1]:3000")));
        assert!(!origin_allowed(&with(
            header::ORIGIN,
            "https://evil.example"
        )));
        assert!(!origin_allowed(&with(
            header::ORIGIN,
            "http://localhost.evil.example"
        )));
    }
}
