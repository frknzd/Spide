//! Minimal MCP server (streamable HTTP, JSON responses) on 127.0.0.1.
//! Every request needs `Authorization: Bearer <token>`; dispatch is `spide_service::mcp`.

use crate::state::AppState;
use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::Value;

pub const PREFERRED_PORT: u16 = 47800;

/// Bind and serve; returns the port.
pub async fn serve(st: AppState) -> std::io::Result<u16> {
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", PREFERRED_PORT)).await {
        Ok(l) => l,
        Err(_) => tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?,
    };
    let port = listener.local_addr()?.port();
    let app = Router::new()
        .route("/mcp", post(handle).get(|| async { StatusCode::METHOD_NOT_ALLOWED }).delete(|| async { StatusCode::OK }))
        .layer(middleware::from_fn(local_only))
        .with_state(st);
    tokio::spawn(async move {
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

/// A browser Origin (`scheme://host[:port]`) that belongs to a local page.
fn is_local_origin(origin: &str) -> bool {
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
    let auth = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok());
    if !spide_service::discovery::bearer_ok(&st.token, auth) {
        return (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Bearer")], "missing or invalid bearer token").into_response();
    }
    let session = headers.get("mcp-session-id").cloned();
    let Some(svc) = crate::service::get(&st) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    // the service blocks (store lock, file IO): keep it off the async workers
    let out = tokio::task::spawn_blocking(move || spide_service::mcp::handle(&svc, &body)).await.ok().flatten();
    let Some(out) = out else {
        return StatusCode::ACCEPTED.into_response();
    };
    let mut resp = Json(out).into_response();
    let sid = session.unwrap_or_else(|| HeaderValue::from_static("spide"));
    resp.headers_mut().insert("mcp-session-id", sid);
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
        for ok in ["http://localhost:1420", "http://127.0.0.1", "http://[::1]:3000"] {
            assert!(is_local_origin(ok), "{ok}");
        }
        for bad in ["http://localhost.evil.com", "http://127.0.0.1.evil.com", "https://evil.com", "null", "file://localhost", "tauri://localhost"] {
            assert!(!is_local_origin(bad), "{bad}");
        }
    }
}
