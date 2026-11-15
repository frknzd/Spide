//! LSP base protocol: `Content-Length` framed JSON-RPC over byte streams.

use serde_json::Value;
use std::io::{BufRead, Write};

pub const MAX_MESSAGE: usize = 64 * 1024 * 1024;

pub fn write_message(w: &mut impl Write, v: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(v)?;
    write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
    w.write_all(&body)?;
    w.flush()
}

/// Read one message. `Ok(None)` at a clean end of stream.
pub fn read_message(r: &mut impl BufRead) -> std::io::Result<Option<Value>> {
    let mut len: Option<usize> = None;
    let mut line = String::new();
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let l = line.trim_end_matches(['\r', '\n']);
        if l.is_empty() {
            if len.is_some() {
                break;
            }
            continue;
        }
        if let Some((k, v)) = l.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().ok();
            }
        }
    }
    let len = len.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "missing Content-Length"))?;
    if len > MAX_MESSAGE {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("message too large ({len} bytes)")));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    serde_json::from_slice(&buf).map(Some).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn roundtrip_and_multibyte_lengths() {
        let mut buf = vec![];
        write_message(&mut buf, &json!({"a":"ü→🦀"})).unwrap();
        write_message(&mut buf, &json!({"b":2})).unwrap();
        let mut r = std::io::BufReader::new(&buf[..]);
        assert_eq!(read_message(&mut r).unwrap().unwrap()["a"], "ü→🦀");
        assert_eq!(read_message(&mut r).unwrap().unwrap()["b"], 2);
        assert!(read_message(&mut r).unwrap().is_none());
    }
}
