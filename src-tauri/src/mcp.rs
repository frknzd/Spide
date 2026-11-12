//! Minimal MCP server (streamable HTTP, JSON responses) on 127.0.0.1.
//! Implements initialize, ping, tools/list and tools/call.

use crate::state::AppState;
use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use spide_core::tools;

pub const PREFERRED_PORT: u16 = 47800;
const PROTOCOL: &str = "2025-06-18";

/// Bind and serve; returns the port.
pub async fn serve(st: AppState) -> std::io::Result<u16> {
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", PREFERRED_PORT)).await {
        Ok(l) => l,
        Err(_) => tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?,
    };
    let port = listener.local_addr()?.port();
    let app = Router::new()
        .route("/mcp", post(handle).get(|| async { StatusCode::METHOD_NOT_ALLOWED }).delete(|| async { StatusCode::OK }))
        .route("/dev/snapshot", axum::routing::get(dev_snapshot))
        .route("/dev/invoke/{cmd}", post(dev_invoke).options(dev_preflight))
        .layer(middleware::from_fn(local_only))
        .with_state(st);
    tauri::async_runtime::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("spide mcp: {e}");
        }
    });
    Ok(port)
}

/// `host[:port]` names this machine's loopback interface.
fn is_loopback_authority(authority: &str) -> bool {
    let host = match authority.strip_prefix('[') {
        // [::1]:port
        Some(rest) => match rest.split_once(']') {
            Some((h, tail)) if tail.is_empty() || tail.starts_with(':') => h,
            _ => return false,
        },
        None => authority.split_once(':').map_or(authority, |(h, _)| h),
    };
    let port_ok = authority.rsplit_once(':').is_none_or(|(_, p)| p.ends_with(']') || p.parse::<u16>().is_ok());
    port_ok && matches!(host.to_ascii_lowercase().as_str(), "127.0.0.1" | "localhost" | "::1")
}

/// A browser Origin (`scheme://host[:port]`) that belongs to a local page or the Tauri webview.
fn is_local_origin(origin: &str) -> bool {
    if matches!(origin, "tauri://localhost" | "http://tauri.localhost" | "https://tauri.localhost") {
        return true;
    }
    match origin.split_once("://") {
        Some(("http" | "https", authority)) => is_loopback_authority(authority),
        _ => false,
    }
}

/// DNS-rebinding / cross-site protection for every route: the Host must be
/// loopback, and a browser Origin (if any) must be local. Prefix matching is
/// not enough (`http://localhost.evil.com`).
async fn local_only(req: Request, next: Next) -> Response {
    let allowed = {
        let h = req.headers();
        let get = |name: header::HeaderName| h.get(name).map(|v| v.to_str().unwrap_or(""));
        get(header::HOST).is_some_and(is_loopback_authority) && get(header::ORIGIN).is_none_or(is_local_origin)
    };
    if !allowed {
        return StatusCode::FORBIDDEN.into_response();
    }
    next.run(req).await
}

async fn handle(State(st): State<AppState>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    let session = headers.get("mcp-session-id").cloned();
    let responses: Vec<Value> = match &body {
        Value::Array(batch) => batch.iter().filter_map(|m| dispatch(&st, m)).collect(),
        m => dispatch(&st, m).into_iter().collect(),
    };
    if responses.is_empty() {
        return StatusCode::ACCEPTED.into_response();
    }
    let out = if body.is_array() { Value::Array(responses) } else { responses.into_iter().next().unwrap() };
    let mut resp = Json(out).into_response();
    let sid = session.unwrap_or_else(|| HeaderValue::from_static("spide"));
    resp.headers_mut().insert("mcp-session-id", sid);
    resp
}

fn dispatch(st: &AppState, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let id = id?; // notifications get no response
    let result = match method {
        "initialize" => {
            let version = msg.pointer("/params/protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "spide", "version": env!("CARGO_PKG_VERSION")},
                "instructions": tools::INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools::tool_list()})),
        "tools/call" => {
            let name = msg.pointer("/params/name").and_then(Value::as_str).unwrap_or("");
            let args = msg.pointer("/params/arguments").cloned().unwrap_or(json!({}));
            let outcome = {
                let mut s = st.lock();
                tools::call(&mut s.store, name, &args)
            };
            match outcome {
                Ok(o) => {
                    if o.changed {
                        st.changed();
                    }
                    Ok(json!({"content": [{"type": "text", "text": o.text}], "isError": false}))
                }
                Err(e) => Ok(json!({"content": [{"type": "text", "text": e}], "isError": true})),
            }
        }
        "resources/list" => Ok(json!({"resources": []})),
        "prompts/list" => Ok(json!({"prompts": []})),
        _ => Err(json!({"code": -32601, "message": format!("method not found: {method}")})),
    };
    Some(match result {
        Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
        Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
    })
}

/// Read-only snapshot for browser-based previews of the UI (dev aid).
async fn dev_snapshot(State(st): State<AppState>) -> Response {
    let snap = st.snapshot();
    let mut resp = Json(snap).into_response();
    resp.headers_mut().insert("access-control-allow-origin", HeaderValue::from_static("http://localhost:1420"));
    resp
}

/// Debug builds only: lets a plain browser drive the UI (used for automated UI checks).
async fn dev_invoke(State(st): State<AppState>, axum::extract::Path(cmd): axum::extract::Path<String>, Json(args): Json<Value>) -> Response {
    let mut resp = if cfg!(debug_assertions) {
        let st2 = st.clone();
        let r = tokio::task::spawn_blocking(move || crate::commands::dev_dispatch(&st2, &cmd, args)).await;
        match r {
            Ok(Ok(v)) => Json(v).into_response(),
            Ok(Err(e)) => (StatusCode::BAD_REQUEST, e).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    } else {
        StatusCode::NOT_FOUND.into_response()
    };
    resp.headers_mut().insert("access-control-allow-origin", HeaderValue::from_static("http://localhost:1420"));
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_hosts() {
        for ok in ["127.0.0.1:47800", "localhost:47800", "LOCALHOST", "[::1]:47800", "127.0.0.1"] {
            assert!(is_loopback_authority(ok), "{ok}");
        }
        for bad in ["evil.com", "localhost.evil.com:47800", "127.0.0.1.nip.io", "127.0.0.1:x", "[::1]x", ""] {
            assert!(!is_loopback_authority(bad), "{bad}");
        }
    }

    #[test]
    fn local_origins() {
        for ok in ["http://localhost:1420", "http://127.0.0.1", "tauri://localhost", "http://[::1]:3000"] {
            assert!(is_local_origin(ok), "{ok}");
        }
        for bad in ["http://localhost.evil.com", "http://127.0.0.1.evil.com", "https://evil.com", "null", "file://localhost"] {
            assert!(!is_local_origin(bad), "{bad}");
        }
    }
}

async fn dev_preflight() -> Response {
    let mut resp = StatusCode::NO_CONTENT.into_response();
    let h = resp.headers_mut();
    h.insert("access-control-allow-origin", HeaderValue::from_static("http://localhost:1420"));
    h.insert("access-control-allow-methods", HeaderValue::from_static("POST"));
    h.insert("access-control-allow-headers", HeaderValue::from_static("content-type"));
    resp
}
