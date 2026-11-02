//! Minimal localhost-only HTTP entry (M1: bind 127.0.0.1, no auth, no
//! remote exposure — ADR/v2 review decision). Serves the demo page and a
//! single JSON endpoint that goes through the SAME execute path as
//! CLI/MCP/library. Blocking IO, one thread per connection; no async runtime.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use trustquant_core::contract::RequestEnvelope;

const INDEX_HTML: &str = include_str!("../../../assets/index.html");
const MAX_BODY: usize = 4 * 1024 * 1024;

pub fn serve(port: u16) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    eprintln!(
        "trustquant web demo on http://127.0.0.1:{port} (localhost-only, no auth, no remote bind — M1 policy)"
    );
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        std::thread::spawn(move || {
            let _ = handle(stream);
        });
    }
    Ok(())
}

fn handle(mut stream: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");

    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(v) = trimmed
            .to_ascii_lowercase()
            .strip_prefix("content-length:")
            .map(str::trim)
            .and_then(|v| v.parse::<usize>().ok())
        {
            content_length = v;
        }
    }
    if content_length > MAX_BODY {
        return respond(
            &mut stream,
            413,
            "application/json",
            json!({"error": "body too large"}).to_string(),
        );
    }

    match (method, path) {
        ("GET", "/") => respond(&mut stream, 200, "text/html; charset=utf-8", INDEX_HTML.to_string()),
        ("GET", "/api/health") => respond(
            &mut stream,
            200,
            "application/json",
            json!({"status": "ok", "runtime": crate::RUNTIME_VERSION, "schema": trustquant_core::contract::SCHEMA_VERSION}).to_string(),
        ),
        ("POST", "/api/tools/call") => {
            let mut body = vec![0u8; content_length];
            reader.read_exact(&mut body)?;
            let parsed: Result<RequestEnvelope, _> = serde_json::from_slice(&body);
            let envelope = match parsed {
                Ok(req) => {
                    if let Err(e) = req.validate_schema_version() {
                        crate::ResponseEnvelope::error(e, req.request_id.as_deref())
                    } else {
                        let (env, _) = crate::execute_and_record(&req);
                        env
                    }
                }
                Err(e) => crate::ResponseEnvelope::error(
                    trustquant_core::contract::ErrorEnvelope::invalid_input(format!("invalid request envelope: {e}"), "body"),
                    None,
                ),
            };
            respond(&mut stream, 200, "application/json", serde_json::to_string(&envelope).unwrap())
        }
        _ => respond(&mut stream, 404, "application/json", json!({"error": "not found"}).to_string()),
    }
}

fn respond(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: String,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        413 => "Payload Too Large",
        _ => "Not Found",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

/// Convenience used by CLI tests: one-shot request builder.
pub fn tool_call_body(tool: &str, params: Value) -> String {
    json!({
        "schema_version": trustquant_core::contract::SCHEMA_VERSION,
        "tool": tool,
        "params": params,
        "request_id": "web",
    })
    .to_string()
}
