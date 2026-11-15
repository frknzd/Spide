//! A tiny scripted language server for deterministic tests (`spide-mock-lsp`).
//!
//! Behaviour: negotiates `utf-16` (or `utf-8` with `--utf8`); on `didOpen` publishes one warning
//! per line containing `TODO` (source `mock`, code `todo`) at its UTF-16/UTF-8 column; answers
//! `documentSymbol` with one symbol per line starting with `fn `; asks the client for
//! `workspace/configuration` once and reports the answer as a diagnostic on the first file;
//! `--crash-after-open` exits right after the first didOpen; `--hang` never answers initialize.

use serde_json::{json, Value};
use std::io::{BufReader, Write};

#[path = "../transport.rs"]
mod transport;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let utf8 = args.iter().any(|a| a == "--utf8");
    let crash = args.iter().any(|a| a == "--crash-after-open");
    let hang = args.iter().any(|a| a == "--hang");
    let stdin = std::io::stdin();
    let mut r = BufReader::new(stdin.lock());
    let mut out = std::io::stdout();
    let mut config_answer: Option<String> = None;
    let mut first_uri: Option<String> = None;
    let send = |out: &mut std::io::Stdout, v: Value| {
        transport::write_message(out, &v).unwrap();
        out.flush().unwrap();
    };
    while let Ok(Some(msg)) = transport::read_message(&mut r) {
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let id = msg.get("id").cloned();
        match method {
            "initialize" => {
                if hang {
                    continue;
                }
                let enc = if utf8 { "utf-8" } else { "utf-16" };
                send(&mut out, json!({"jsonrpc":"2.0","id":id,"result":{"capabilities":{"positionEncoding":enc,"documentSymbolProvider":true,"definitionProvider":true,"textDocumentSync":1},"serverInfo":{"name":"mock-ls"}}}));
            }
            "initialized" => {
                send(&mut out, json!({"jsonrpc":"2.0","id":9001,"method":"workspace/configuration","params":{"items":[{"section":"mock.level"}]}}));
            }
            "textDocument/didOpen" => {
                let uri = msg.pointer("/params/textDocument/uri").and_then(Value::as_str).unwrap_or("").to_string();
                let text = msg.pointer("/params/textDocument/text").and_then(Value::as_str).unwrap_or("");
                let version = msg.pointer("/params/textDocument/version").cloned();
                if crash {
                    std::process::exit(3);
                }
                let mut diags = vec![];
                for (i, l) in text.split('\n').enumerate() {
                    if let Some(b) = l.find("TODO") {
                        let col: usize = if utf8 { b } else { l[..b].chars().map(char::len_utf16).sum() };
                        diags.push(json!({"range":{"start":{"line":i,"character":col},"end":{"line":i,"character":col+4}},"severity":2,"source":"mock","code":"todo","message":"todo left"}));
                    }
                }
                if first_uri.is_none() {
                    first_uri = Some(uri.clone());
                    if let Some(c) = &config_answer {
                        diags.push(json!({"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"severity":4,"source":"mock","code":"config","message":format!("config={c}")}));
                    }
                }
                send(&mut out, json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":uri,"version":version,"diagnostics":diags}}));
            }
            "textDocument/documentSymbol" => {
                let _ = &msg;
                send(&mut out, json!({"jsonrpc":"2.0","id":id,"result":[{"name":"main","kind":12,"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},"selectionRange":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}}}]}));
            }
            "shutdown" => send(&mut out, json!({"jsonrpc":"2.0","id":id,"result":null})),
            "exit" => std::process::exit(0),
            "" if id.as_ref().and_then(Value::as_i64) == Some(9001) => {
                config_answer = Some(msg.get("result").map(|v| v.to_string()).unwrap_or_default());
            }
            _ => {
                if let Some(id) = id {
                    send(&mut out, json!({"jsonrpc":"2.0","id":id,"result":null}));
                }
            }
        }
    }
}
