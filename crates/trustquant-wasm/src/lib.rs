//! WASM boundary for the pure-computation core (ADR-0003: M0-M2 differential
//! validation target, NOT a shipped interface). Byte-buffer ABI:
//!   qt_alloc(len) -> ptr; qt_dealloc(ptr, len); qt_dispatch(ptr, len) -> ptr
//! where the returned buffer is [u64 LE len][canonical JSON response].
//! calculation_id / manifests are runtime concerns and absent here.

use serde_json::{Value, json};
use trustquant_core::contract::SCHEMA_VERSION;

#[unsafe(no_mangle)]
pub extern "C" fn qt_alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    core::mem::forget(buf);
    ptr
}

/// # Safety
/// ptr must come from qt_alloc with the same len.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qt_dealloc(ptr: *mut u8, len: usize) {
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
}

fn envelope_for(request: &Value) -> Value {
    let tool = request["tool"].as_str().unwrap_or_default().to_string();
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let check = trustquant_core::contract::RequestEnvelope {
        schema_version: request["schema_version"]
            .as_str()
            .unwrap_or(SCHEMA_VERSION)
            .to_string(),
        tool: tool.clone(),
        params: params.clone(),
        request_id: None,
    };
    if let Err(e) = check.validate_schema_version() {
        return error_envelope(serde_json::to_value(e).unwrap());
    }
    match trustquant_core::dispatch(&tool, &params) {
        Ok(out) => json!({
            "schema_version": SCHEMA_VERSION, "status": "success",
            "engine_id": out.engine_id, "result": out.result, "warnings": out.warnings, "error": null,
        }),
        Err(e) => error_envelope(serde_json::to_value(e).unwrap()),
    }
}

fn error_envelope(e: Value) -> Value {
    json!({
        "schema_version": SCHEMA_VERSION, "status": "error",
        "engine_id": null, "result": null, "warnings": [], "error": e,
    })
}

/// # Safety
/// ptr/len must describe a valid UTF-8 buffer from qt_alloc.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qt_dispatch(ptr: *const u8, len: usize) -> *mut u8 {
    let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
    let body = core::str::from_utf8(slice)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .map(|req| envelope_for(&req))
        .unwrap_or_else(|| {
            error_envelope(json!({
                "error_type": "invalid_input", "message": "request is not valid UTF-8 JSON",
                "correctable": true, "retryable": false,
            }))
        });
    let bytes = serde_json::to_vec(&body).expect("envelope serialization cannot fail");
    let mut buf = Vec::<u8>::with_capacity(8 + bytes.len());
    buf.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    buf.extend_from_slice(&bytes);
    let out_ptr = buf.as_mut_ptr();
    core::mem::forget(buf);
    out_ptr
}
