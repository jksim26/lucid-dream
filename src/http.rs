//! Streamable HTTP transport: each POST to /mcp carries one message and gets a
//! plain JSON reply. The bearer token decides which agent is speaking.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
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
    /// A door: when set, this listener accepts that one token (agent, label)
    /// and nothing else. Put a tunnel in front of a door, never in front of
    /// the main listener.
    pub only: Option<(String, String)>,
}

pub fn serve(addr: SocketAddr, app: App) -> Result<(), String> {
    let tokens = Tokens::load(&app.tokens).map_err(|e| format!("cannot read tokens: {e}"))?;
    match &app.only {
        Some((agent, label)) if !tokens.has(agent, label) => {
            return Err(format!(
                "no token {agent}/{label}; issue it with `lucid-dream token add {agent} --label {label}`"
            ));
        }
        None if !app.loopback && tokens.is_empty() => {
            return Err(format!(
                "refusing to listen on {addr} with no tokens; run `lucid-dream token add <agent>` \
                 first, or bind to 127.0.0.1"
            ));
        }
        _ => {}
    }
    let scope = match &app.only {
        Some((agent, label)) => format!(" (door for {agent}/{label} only)"),
        None => String::new(),
    };
    eprintln!(
        "lucid-dream: serving {} at http://{addr}/mcp{scope}",
        app.store.root().display()
    );
    let router = Router::new()
        .route("/mcp", post(post_mcp))
        .route("/mcp/{token}", post(post_mcp_with_path_token))
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
    respond(&app, &headers, None, &body)
}

/// For a client that cannot send a header: the token is the last part of the
/// URL. Doors only, so the main listener never takes a secret from a URL.
async fn post_mcp_with_path_token(
    State(app): State<Arc<App>>,
    Path(token): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if app.only.is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    respond(&app, &headers, Some(&token), &body)
}

fn respond(app: &App, headers: &HeaderMap, path_token: Option<&str>, body: &[u8]) -> Response {
    let who = match identify(app, headers, path_token) {
        Caller::Agent(who) => who,
        // Admitted without a token, so a web page in a local browser could be
        // the one calling. Only a local page may; this blocks DNS rebinding.
        Caller::Local if origin_allowed(headers) => LOCAL.to_string(),
        Caller::Local => return (StatusCode::FORBIDDEN, "origin not allowed").into_response(),
        Caller::Nobody => {
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Bearer")],
                "missing or wrong bearer token",
            )
                .into_response();
        }
    };
    let Ok(msg) = serde_json::from_slice::<Value>(body) else {
        let reply = mcp::error(Value::Null, -32700, "parse error");
        return (StatusCode::BAD_REQUEST, Json(reply)).into_response();
    };
    match mcp::handle(&app.store, &who, &msg) {
        Some(reply) => Json(reply).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

#[derive(Debug, PartialEq)]
enum Caller {
    Agent(String),
    Local,
    Nobody,
}

/// Who is calling. The token file is re-read each time, so a token added or
/// revoked takes effect without a restart.
fn identify(app: &App, headers: &HeaderMap, path_token: Option<&str>) -> Caller {
    let Ok(tokens) = Tokens::load(&app.tokens) else {
        return Caller::Nobody;
    };
    // Some clients send "Bearer <token>", some send the bare token.
    let header_token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.strip_prefix("Bearer ").unwrap_or(v).trim());
    let given = header_token
        .or(path_token)
        .and_then(|token| tokens.identify(token));
    match (&app.only, given) {
        // A door never falls back to the tokenless local caller: a tunnel
        // makes the whole internet look like loopback.
        (Some((agent, label)), Some(found)) if found == (agent.as_str(), label.as_str()) => {
            Caller::Agent(agent.clone())
        }
        (Some(_), _) => Caller::Nobody,
        (None, Some((agent, _))) => Caller::Agent(agent.to_string()),
        (None, None) if tokens.is_empty() && app.loopback => Caller::Local,
        (None, None) => Caller::Nobody,
    }
}

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
    use crate::tokens::{self, DEFAULT_LABEL};

    fn with(name: header::HeaderName, value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(name, value.parse().unwrap());
        h
    }

    fn identify3(app: &App, headers: &HeaderMap) -> Caller {
        identify(app, headers, None)
    }

    fn bearer(token: &str) -> HeaderMap {
        with(header::AUTHORIZATION, &format!("Bearer {token}"))
    }

    fn app(t: &Temp, loopback: bool, only: Option<(&str, &str)>) -> App {
        App {
            store: t.store(),
            tokens: tokens::path(&t.0),
            loopback,
            only: only.map(|(a, l)| (a.to_string(), l.to_string())),
        }
    }

    #[test]
    fn without_tokens_only_loopback_is_let_in() {
        let t = Temp::new();
        assert_eq!(
            identify3(&app(&t, true, None), &HeaderMap::new()),
            Caller::Local
        );
        assert_eq!(
            identify3(&app(&t, false, None), &HeaderMap::new()),
            Caller::Nobody
        );
    }

    #[test]
    fn a_token_names_the_caller() {
        let t = Temp::new();
        let token = Tokens::add(&tokens::path(&t.0), "codex", DEFAULT_LABEL).unwrap();
        let a = app(&t, true, None);
        assert_eq!(
            identify3(&a, &bearer(&token)),
            Caller::Agent("codex".into())
        );
        assert_eq!(identify3(&a, &HeaderMap::new()), Caller::Nobody);
        assert_eq!(identify3(&a, &bearer("ld_nope")), Caller::Nobody);
        // The bare token, without "Bearer ", is accepted too.
        assert_eq!(
            identify3(&a, &with(header::AUTHORIZATION, &token)),
            Caller::Agent("codex".into())
        );
    }

    #[test]
    fn a_door_admits_one_token_and_nothing_else() {
        let t = Temp::new();
        let file = tokens::path(&t.0);
        let devices = Tokens::add(&file, "claude", DEFAULT_LABEL).unwrap();
        let phone = Tokens::add(&file, "claude", "phone").unwrap();
        let grok = Tokens::add(&file, "grok", DEFAULT_LABEL).unwrap();

        let door = app(&t, true, Some(("claude", "phone")));
        assert_eq!(
            identify3(&door, &bearer(&phone)),
            Caller::Agent("claude".into())
        );
        // A leaked device token is useless from the internet, and so is another door's.
        assert_eq!(identify3(&door, &bearer(&devices)), Caller::Nobody);
        assert_eq!(identify3(&door, &bearer(&grok)), Caller::Nobody);
        assert_eq!(identify3(&door, &HeaderMap::new()), Caller::Nobody);

        // Revoking the phone closes its door at once; the others keep working.
        Tokens::remove(&file, "claude", "phone").unwrap();
        assert_eq!(identify3(&door, &bearer(&phone)), Caller::Nobody);
        assert_eq!(
            identify3(&app(&t, true, None), &bearer(&devices)),
            Caller::Agent("claude".into())
        );
    }

    #[test]
    fn a_door_takes_its_token_from_the_url_when_there_is_no_header() {
        let t = Temp::new();
        let file = tokens::path(&t.0);
        let devices = Tokens::add(&file, "claude", DEFAULT_LABEL).unwrap();
        let phone = Tokens::add(&file, "claude", "phone").unwrap();
        let door = app(&t, true, Some(("claude", "phone")));
        let none = HeaderMap::new();
        assert_eq!(
            identify(&door, &none, Some(&phone)),
            Caller::Agent("claude".into())
        );
        assert_eq!(identify(&door, &none, Some(&devices)), Caller::Nobody);
        assert_eq!(identify(&door, &none, Some("ld_guess")), Caller::Nobody);
    }

    #[test]
    fn a_door_never_admits_a_tokenless_caller() {
        let t = Temp::new();
        let door = app(&t, true, Some(("claude", "phone")));
        assert_eq!(identify3(&door, &HeaderMap::new()), Caller::Nobody);
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
