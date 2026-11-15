//! MCP JSON-RPC (2025-06-18 tools subset), transport-independent. Used by the headless stdio and
//! HTTP transports; the desktop app's HTTP server dispatches through the same `Service`.

use crate::Service;
use serde_json::{json, Value};

pub const PROTOCOL: &str = "2025-06-18";

/// Handle one message or a batch. `None`: nothing to send (notifications only).
pub fn handle(svc: &Service, body: &Value) -> Option<Value> {
    match body {
        Value::Array(batch) => {
            let out: Vec<Value> = batch.iter().filter_map(|m| dispatch(svc, m)).collect();
            (!out.is_empty()).then_some(Value::Array(out))
        }
        m => dispatch(svc, m),
    }
}

/// Parse error response for undecodable input.
pub fn parse_error(e: impl std::fmt::Display) -> Value {
    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":format!("parse error: {e}")}})
}

pub fn dispatch(svc: &Service, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned()?; // notifications get no response
    if msg.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32600,"message":"invalid request: jsonrpc must be \"2.0\""}}));
    }
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let result: Result<Value, Value> = match method {
        "initialize" => {
            let version = msg.pointer("/params/protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "spide", "version": env!("CARGO_PKG_VERSION")},
                "instructions": svc.instructions(),
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": svc.tool_list()})),
        "tools/call" => {
            let name = msg.pointer("/params/name").and_then(Value::as_str).unwrap_or("");
            let args = msg.pointer("/params/arguments").cloned().unwrap_or(json!({}));
            // a human action from Neovim proves itself with the user token; anything else is an agent
            let token = msg.pointer("/params/_meta/user_token").and_then(Value::as_str).unwrap_or("");
            let actor = if crate::discovery::token_ok(&svc.user_token, token) { "editor-user" } else { "agent" };
            Ok(match svc.call_tool(name, &args, actor) {
                Ok(text) => json!({"content":[{"type":"text","text":text}],"isError":false}),
                Err(e) => json!({"content":[{"type":"text","text":e}],"isError":true}),
            })
        }
        "resources/list" => Ok(json!({"resources": []})),
        "prompts/list" => Ok(json!({"prompts": []})),
        _ => Err(json!({"code": -32601, "message": format!("method not found: {method}")})),
    };
    Some(match result {
        Ok(r) => json!({"jsonrpc":"2.0","id":id,"result":r}),
        Err(e) => json!({"jsonrpc":"2.0","id":id,"error":e}),
    })
}
