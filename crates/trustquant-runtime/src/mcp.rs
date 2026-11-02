//! MCP-over-stdio message handler (protocol conversion ONLY — every
//! computation goes through the Runtime; ADR-0006 gate 1 requires the same
//! results as library and CLI).
//!
//! tools/call arguments embed the contract request:
//! {"schema_version": "..", "request_id"?: "..", "params": {..}}
//! Tool-level failures return isError results carrying the response
//! envelope; protocol mistakes return JSON-RPC errors.

use crate::{execute_and_record, known_tools, replay_calculation, tool_descriptors};

use serde_json::{Value, json};
use trustquant_core::contract::RequestEnvelope;

pub const PROTOCOL_VERSION: &str = "2025-06-18";

pub fn handle_message(msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let reply = |id: Option<Value>, result: Value| {
        id.map(|i| json!({"jsonrpc": "2.0", "id": i, "result": result}))
    };
    let error = |id: Option<Value>, code: i64, message: &str| {
        id.map(|i| json!({"jsonrpc": "2.0", "id": i, "error": {"code": code, "message": message}}))
    };

    match method {
        "initialize" => reply(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "trustquant", "version": crate::RUNTIME_VERSION}
            }),
        ),
        "notifications/initialized" => None,
        "tools/list" => reply(id, json!({"tools": tool_descriptors()})),
        "tools/call" => {
            let i = id?;
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            let Some(tool) = params["name"].as_str().map(str::to_string) else {
                return error(Some(i), -32602, "tools/call requires params.name");
            };
            if !known_tools().contains(&tool.as_str()) {
                return error(Some(i), -32602, &format!("unknown tool '{tool}'"));
            }
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            let schema_version = arguments["schema_version"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| trustquant_core::contract::SCHEMA_VERSION.to_string());
            let request = RequestEnvelope {
                schema_version,
                tool: tool.clone(),
                params: arguments.get("params").cloned().unwrap_or(json!({})),
                request_id: arguments["request_id"].as_str().map(str::to_string),
            };
            if let Err(e) = request.validate_schema_version() {
                let envelope = crate::ResponseEnvelope::error(e, request.request_id.as_deref());
                return Some(tool_result(i, &envelope));
            }
            if tool == "replay_calculation" {
                let cid = request.params["calculation_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let envelope = match replay_calculation(&cid) {
                    Ok(report) => {
                        let mut env = crate::ResponseEnvelope::success(
                            &trustquant_core::contract::ToolOutput {
                                engine_id: "runtime.replay.v1",
                                provider_id: "runtime",
                                result: serde_json::to_value(&report).unwrap_or(Value::Null),
                                warnings: vec![],
                            },
                            request.request_id.as_deref(),
                        );
                        env.provider_id = Some("runtime".into());
                        env
                    }
                    Err(e) => crate::ResponseEnvelope::error(e, request.request_id.as_deref()),
                };
                return Some(tool_result(i, &envelope));
            }
            if tool == "get_calculation_manifest" {
                let cid = request.params["calculation_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let envelope = match crate::get_manifest(&cid) {
                    Ok(m) => crate::ResponseEnvelope::success(
                        &trustquant_core::contract::ToolOutput {
                            engine_id: "runtime.manifest.v1",
                            provider_id: "runtime",
                            result: m,
                            warnings: vec![],
                        },
                        request.request_id.as_deref(),
                    ),
                    Err(e) => crate::ResponseEnvelope::error(e, request.request_id.as_deref()),
                };
                return Some(tool_result(i, &envelope));
            }
            let (envelope, _) = execute_and_record(&request);
            Some(tool_result(i, &envelope))
        }
        "" => error(id, -32600, "missing method"),
        _ => error(id, -32601, &format!("unknown method '{method}'")),
    }
}

fn tool_result(id: Value, envelope: &crate::ResponseEnvelope) -> Value {
    json!({
        "jsonrpc": "2.0", "id": id,
        "result": {
            "content": [{"type": "text", "text": serde_json::to_string(envelope).unwrap()}],
            "isError": envelope.status == "error"
        }
    })
}

/// stdio server loop: one JSON message per line.
pub fn run_stdio() -> i32 {
    use std::io::{BufRead, Write};
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(trimmed) {
            Ok(msg) => handle_message(&msg),
            Err(e) => Some(json!({
                "jsonrpc": "2.0", "id": null,
                "error": {"code": -32700, "message": format!("parse error: {e}")}
            })),
        };
        if let Some(r) = response
            && writeln!(out, "{r}").and_then(|_| out.flush()).is_err()
        {
            return 1;
        }
    }
    0
}
