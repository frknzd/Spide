//! `spide-headless`: Spide without a window, pi or Neovim.
//!
//! ```text
//! spide-headless [--project DIR] mcp                    MCP over stdio (newline-delimited JSON-RPC)
//! spide-headless [--project DIR] serve [--port N]       MCP over authenticated loopback HTTP; publishes
//!                                                       .spide/endpoint.json (url + bearer token, 0600)
//! spide-headless [--project DIR] trust [--permission read_only|propose_only|autonomous]
//!                [--trusted|--untrusted] [--approve-command CMD] [--revoke-command CMD]
//!                                                       owner-only: edits ~/.config/spide/trust.json
//! spide-headless [--project DIR] trace [--failures] [--limit N]   compact trace
//! spide-headless [--project DIR] call TOOL [JSON]       one tool call (scripts, smoke tests)
//! ```

use serde_json::{json, Value};
use spide_service::{discovery, mcp, policy, trace, Host, Service};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;

fn usage() -> ! {
    eprintln!("{}", include_str!("main.rs").lines().skip(3).take(10).map(|l| l.trim_start_matches("//! ")).collect::<Vec<_>>().join("\n"));
    std::process::exit(2)
}

fn die(e: impl std::fmt::Display) -> ! {
    eprintln!("spide-headless: {e}");
    std::process::exit(1)
}

fn open(dir: &PathBuf) -> Service {
    Service::open(dir, Host::Headless, None).unwrap_or_else(|e| die(e))
}

fn main() {
    let mut project: Option<PathBuf> = std::env::var_os("SPIDE_PROJECT").map(PathBuf::from);
    let mut rest = vec![];
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--project" => project = Some(args.next().map(PathBuf::from).unwrap_or_else(|| usage())),
            "-h" | "--help" => usage(),
            _ => rest.push(a),
        }
    }
    let dir = project.unwrap_or_else(|| std::env::current_dir().expect("cwd"));
    let dir = spide_core::project::resolve_root(&dir);
    let cmd = rest.first().cloned().unwrap_or_else(|| "mcp".into());
    let opts = &rest[rest.len().min(1)..];
    match cmd.as_str() {
        "mcp" => serve_stdio(&open(&dir)),
        "serve" => {
            let port = flag(opts, "--port").map(|p| p.parse().unwrap_or_else(|_| usage())).unwrap_or(0u16);
            serve_http(Arc::new(open(&dir)), port)
        }
        "trust" => trust(&dir, opts),
        "trace" => {
            let j = trace::Journal::new(&dir);
            let f = trace::Filter { only: opts.iter().any(|o| o == "--failures").then(|| "failures".into()), limit: flag(opts, "--limit").and_then(|l| l.parse().ok()).unwrap_or(30), ..Default::default() };
            let (rows, total) = j.timeline(&f);
            for r in rows.iter().rev() {
                println!("{}  {:<10} {}", r["op"].as_str().unwrap_or(""), r["actor"].as_str().unwrap_or(""), r["summary"].as_str().unwrap_or(""));
            }
            eprintln!("({} of {total})", rows.len());
        }
        "call" => {
            let svc = open(&dir);
            let tool = opts.first().unwrap_or_else(|| usage());
            let args: Value = opts.get(1).map(|a| serde_json::from_str(a).unwrap_or_else(|e| die(e))).unwrap_or(json!({}));
            match svc.call_tool(tool, &args, "cli") {
                Ok(t) => println!("{t}"),
                Err(e) => die(e),
            }
        }
        _ => usage(),
    }
}

fn flag<'a>(opts: &'a [String], name: &str) -> Option<&'a String> {
    opts.iter().position(|o| o == name).and_then(|i| opts.get(i + 1))
}

fn trust(dir: &std::path::Path, opts: &[String]) {
    let path = policy::trust_path();
    let mut file = policy::TrustFile::load_from(&path);
    file.version = 1;
    let key = policy::key(dir);
    let t = file.projects.entry(key.clone()).or_default();
    if let Some(p) = flag(opts, "--permission") {
        t.permission = policy::Permission::parse(p).unwrap_or_else(|| die(format!("unknown permission {p}")));
    }
    if opts.iter().any(|o| o == "--trusted") {
        t.trusted = true;
    }
    if opts.iter().any(|o| o == "--untrusted") {
        t.trusted = false;
    }
    if let Some(c) = flag(opts, "--approve-command") {
        if !t.approved_commands.contains(c) {
            t.approved_commands.push(c.clone());
        }
    }
    if let Some(c) = flag(opts, "--revoke-command") {
        t.approved_commands.retain(|x| x != c);
    }
    let shown = serde_json::to_string_pretty(&*t).unwrap();
    if !opts.is_empty() {
        file.save_to(&path).unwrap_or_else(|e| die(e));
    }
    println!("{key}\n{shown}\n({})", path.display());
}

/// The process that took this workspace over (the desktop, after `yield_workspace`).
fn desktop(svc: &Service) -> Option<discovery::Endpoint> {
    discovery::Endpoint::find(&svc.root()).filter(|e| e.pid != std::process::id())
}

fn forward(ep: &discovery::Endpoint, msg: &Value) -> Result<Value, String> {
    let url = ep.mcp_url.as_deref().ok_or("desktop has no MCP endpoint")?;
    let rest = url.strip_prefix("http://").ok_or("bad url")?;
    let (hostport, path) = rest.split_once('/').ok_or("bad url")?;
    let mut s = std::net::TcpStream::connect(hostport).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(120))).ok();
    let body = msg.to_string();
    use std::io::Read;
    write!(s, "POST /{path} HTTP/1.1\r\nHost: {hostport}\r\nContent-Type: application/json\r\nAuthorization: Bearer {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", ep.token, body.len()).map_err(|e| e.to_string())?;
    let mut r = String::new();
    s.read_to_string(&mut r).map_err(|e| e.to_string())?;
    let payload = r.split("\r\n\r\n").nth(1).unwrap_or("");
    if payload.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(payload).map_err(|e| e.to_string())
}

/// Serve locally, or — after the desktop took over — forward to it; take the workspace back when
/// the desktop is gone.
fn route(svc: &Service, msg: &Value) -> Option<Value> {
    use std::sync::atomic::Ordering;
    if svc.yielded.load(Ordering::SeqCst) {
        if let Some(ep) = desktop(svc) {
            match forward(&ep, msg) {
                Ok(Value::Null) => return None,
                Ok(v) => return Some(v),
                Err(e) => eprintln!("spide-headless: desktop unreachable ({e}); taking the workspace back"),
            }
        }
        if svc.store_host().reacquire().is_ok() {
            svc.yielded.store(false, Ordering::SeqCst);
        } else {
            let id = msg.get("id").cloned().unwrap_or(Value::Null);
            return Some(serde_json::json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":"the desktop owns this workspace but is unreachable; retry"}}));
        }
    }
    mcp::handle(svc, msg)
}

fn serve_stdio(svc: &Service) {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => route(svc, &msg),
            Err(e) => Some(mcp::parse_error(e)),
        };
        if let Some(r) = reply {
            let _ = writeln!(out, "{r}");
            let _ = out.flush();
        }
    }
}

#[derive(Clone)]
struct Http {
    svc: Arc<Service>,
    token: Arc<String>,
}

fn serve_http(svc: Arc<Service>, port: u16) {
    use axum::{extract::State, http::{header, HeaderMap, StatusCode}, response::IntoResponse, routing::post, Json, Router};
    let rt = tokio::runtime::Runtime::new().unwrap_or_else(|e| die(e));
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.unwrap_or_else(|e| die(e));
        let port = listener.local_addr().unwrap().port();
        let token = spide_service::workspace::token();
        let ep = discovery::Endpoint {
            version: 1,
            root: svc.root().display().to_string(),
            pid: std::process::id(),
            host: "headless".into(),
            mcp_url: Some(format!("http://127.0.0.1:{port}/mcp")),
            editor_socket: None,
            token: token.clone(),
            started_at: spide_core::store::now_ms(),
        };
        ep.publish().unwrap_or_else(|e| die(e));
        let ep_keep = ep.clone();
        let svc_watch = svc.clone();
        // after the desktop leaves, publish ourselves again
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if !svc_watch.yielded.load(std::sync::atomic::Ordering::SeqCst) && discovery::Endpoint::find(&svc_watch.root()).is_none_or(|e| e.pid != std::process::id()) && desktop(&svc_watch).is_none() {
                let _ = ep_keep.publish();
            }
        });
        eprintln!("spide-headless: MCP on http://127.0.0.1:{port}/mcp (token in {})", discovery::project_file(&svc.root()).display());
        async fn handle(State(h): State<Http>, headers: HeaderMap, Json(body): Json<Value>) -> axum::response::Response {
            let host_ok = headers.get(header::HOST).and_then(|v| v.to_str().ok()).is_some_and(|h| {
                let host = h.rsplit_once(':').map_or(h, |(a, _)| a);
                matches!(host, "127.0.0.1" | "localhost" | "[::1]")
            });
            let origin_ok = headers.get(header::ORIGIN).is_none();
            if !host_ok || !origin_ok {
                return StatusCode::FORBIDDEN.into_response();
            }
            if !discovery::bearer_ok(&h.token, headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok())) {
                return (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Bearer")]).into_response();
            }
            let svc = h.svc.clone();
            match tokio::task::spawn_blocking(move || route(&svc, &body)).await.ok().flatten() {
                Some(v) => Json(v).into_response(),
                None => StatusCode::ACCEPTED.into_response(),
            }
        }
        let app_svc = svc.clone();
        let app = Router::new().route("/mcp", post(handle)).with_state(Http { svc, token: Arc::new(token) });
        let shutdown = async {
            let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
        };
        let r = axum::serve(listener, app).with_graceful_shutdown(shutdown).await;
        ep.retract();
        // release the writer lock (Drop) before exiting
        let _ = app_svc.store_host().release();
        if let Err(e) = r {
            die(e)
        }
    });
}
