//! Minimal MCP server (streamable HTTP, JSON responses) on 127.0.0.1.
//! Implements initialize, ping, tools/list and tools/call.

use crate::state::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
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
        .with_state(st);
    tauri::async_runtime::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("spide mcp: {e}");
        }
    });
    Ok(port)
}

async fn handle(State(st): State<AppState>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    // Only accept local origins (DNS-rebinding protection).
    if let Some(origin) = headers.get("origin").and_then(|o| o.to_str().ok()) {
        if !(origin.starts_with("http://127.0.0.1") || origin.starts_with("http://localhost")) {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
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

async fn dev_preflight() -> Response {
    let mut resp = StatusCode::NO_CONTENT.into_response();
    let h = resp.headers_mut();
    h.insert("access-control-allow-origin", HeaderValue::from_static("http://localhost:1420"));
    h.insert("access-control-allow-methods", HeaderValue::from_static("POST"));
    h.insert("access-control-allow-headers", HeaderValue::from_static("content-type"));
    resp
}
