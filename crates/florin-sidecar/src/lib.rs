//! External Python Provider sidecar client (ADR-0002). JSON Lines over
//! stdio: stdout carries protocol messages only, stderr carries provider
//! logs. No implicit installation: if the interpreter or sidecar cannot
//! start, callers get `provider_unavailable` — never a silent fallback.
//!
//! M0 scope: engineering channel prototype (cold start, version reporting,
//! one deterministic round-trip, timeout, crash). No portfolio optimization
//! logic lives here yet.

use florin_core::contract::ErrorEnvelope;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const PROTOCOL_VERSION: u32 = 1;
pub const PROVIDER_ID: &str = "python-sidecar";

/// Path to the sidecar script shipped in this repo.
pub fn sidecar_script_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sidecar/florin_provider.py")
}

/// Interpreter override for tests and heterogeneous environments.
pub fn python_command() -> String {
    std::env::var("FLORIN_PROVIDER_PYTHON").unwrap_or_else(|_| "python3".to_string())
}

#[derive(Debug)]
pub struct SidecarHandle {
    child: Child,
    stdin: std::process::ChildStdin,
    rx: std::sync::mpsc::Receiver<String>,
    pub hello: Value,
}

#[derive(Debug)]
pub enum SidecarError {
    ProviderUnavailable(String),
    Timeout { after_ms: u64 },
    Crashed { detail: String },
}

impl SidecarError {
    pub fn into_envelope(self) -> ErrorEnvelope {
        match self {
            SidecarError::ProviderUnavailable(msg) => ErrorEnvelope::provider_unavailable(
                msg,
                "install a Python 3.10+ interpreter; see ADR-0002 (no implicit install)",
            ),
            SidecarError::Timeout { after_ms } => ErrorEnvelope::resource_limit(format!(
                "provider did not answer within {after_ms} ms"
            )),
            SidecarError::Crashed { detail } => {
                ErrorEnvelope::internal(format!("provider process failed: {detail}"))
            }
        }
    }
}

impl SidecarHandle {
    /// Cold start: spawn, read hello with a startup timeout.
    pub fn start(
        script: &std::path::Path,
        startup_timeout: Duration,
    ) -> Result<Self, SidecarError> {
        let cmd = python_command();
        let mut child = Command::new(&cmd)
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| SidecarError::ProviderUnavailable(format!("cannot start '{cmd}': {e}")))?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        // Reader thread + channel: makes read deadlines real (a blocking
        // read_line cannot honor a timeout).
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if tx.send(line.clone()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        let mut handle = SidecarHandle {
            child,
            stdin,
            rx,
            hello: Value::Null,
        };
        let hello = handle.read_message(startup_timeout)?;
        if hello["type"].as_str() != Some("hello") {
            return Err(SidecarError::Crashed {
                detail: format!("first message is not hello: {hello}"),
            });
        }
        if hello["protocol"].as_u64() != Some(PROTOCOL_VERSION as u64) {
            return Err(SidecarError::Crashed {
                detail: format!("protocol mismatch: sidecar speaks {:?}", hello["protocol"]),
            });
        }
        handle.hello = hello;
        Ok(handle)
    }

    fn read_message(&mut self, timeout: Duration) -> Result<Value, SidecarError> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(SidecarError::Timeout {
                    after_ms: timeout.as_millis() as u64,
                });
            }
            let line = self.rx.recv_timeout(remaining).map_err(|_| {
                // Channel closed (EOF) or deadline hit. Distinguish via liveness.
                match self.child.try_wait() {
                    Ok(Some(status)) => SidecarError::Crashed {
                        detail: format!("provider exited with {status} before answering"),
                    },
                    Ok(None) => SidecarError::Timeout {
                        after_ms: timeout.as_millis() as u64,
                    },
                    Err(e) => SidecarError::Crashed {
                        detail: format!("liveness check failed: {e}"),
                    },
                }
            })?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            return serde_json::from_str(trimmed).map_err(|e| SidecarError::Crashed {
                detail: format!("bad protocol JSON: {e}"),
            });
        }
    }

    fn send(&mut self, msg: &Value) -> Result<(), SidecarError> {
        self.stdin
            .write_all(format!("{}\n", serde_json::to_string(msg).unwrap()).as_bytes())
            .and_then(|_| self.stdin.flush())
            .map_err(|e| SidecarError::Crashed {
                detail: format!("write error: {e}"),
            })
    }

    /// One deterministic round-trip: request -> matching response.
    pub fn call(
        &mut self,
        tool: &str,
        params: &Value,
        timeout: Duration,
    ) -> Result<Value, SidecarError> {
        let request_id = "m0-proto";
        self.send(
            &json!({"type": "request", "request_id": request_id, "tool": tool, "params": params}),
        )?;
        let resp = self.read_message(timeout)?;
        if resp["type"].as_str() != Some("response")
            || resp["request_id"].as_str() != Some(request_id)
        {
            return Err(SidecarError::Crashed {
                detail: format!("unexpected message: {resp}"),
            });
        }
        if resp["status"].as_str() != Some("ok") {
            return Err(SidecarError::Crashed {
                detail: format!("provider returned error: {resp}"),
            });
        }
        Ok(resp["result"].clone())
    }

    pub fn provider_version(&self) -> String {
        self.hello["version"]
            .as_str()
            .unwrap_or("unknown")
            .to_string()
    }

    pub fn capabilities(&self) -> Vec<String> {
        self.hello["capabilities"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for SidecarHandle {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script() -> std::path::PathBuf {
        sidecar_script_path()
    }

    // Sidecar tests spawn real processes and (one of them) mutates a global
    // env var; serialize the whole module to keep them deterministic.
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn guard() -> std::sync::MutexGuard<'static, ()> {
        // SAFETY-free: static Mutex is fine without unsafe since Rust 1.63.
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn cold_start_handshake_and_roundtrip() {
        let _g = guard();
        let mut sc =
            SidecarHandle::start(&script(), Duration::from_secs(15)).expect("sidecar starts");
        assert!(sc.provider_version().starts_with("0."));
        assert!(sc.capabilities().iter().any(|c| c == "echo_add"));
        let result = sc
            .call(
                "echo_add",
                &json!({"a": 2, "b": 40}),
                Duration::from_secs(5),
            )
            .expect("roundtrip");
        assert_eq!(result["sum"].as_f64(), Some(42.0));
    }

    #[test]
    fn missing_interpreter_maps_to_provider_unavailable() {
        let _g = guard();
        let old = std::env::var("FLORIN_PROVIDER_PYTHON").ok();
        // SAFETY: test-only mutation of a process-global env var, serialized
        // by the module lock across all sidecar tests.
        unsafe {
            std::env::set_var("FLORIN_PROVIDER_PYTHON", "/nonexistent/python-for-test");
        }
        let err = SidecarHandle::start(&script(), Duration::from_secs(5)).unwrap_err();
        // SAFETY: same serialization as above.
        unsafe {
            if let Some(prev) = old {
                std::env::set_var("FLORIN_PROVIDER_PYTHON", prev);
            } else {
                std::env::remove_var("FLORIN_PROVIDER_PYTHON");
            }
        }
        match err {
            SidecarError::ProviderUnavailable(_) => {}
            other => panic!("expected ProviderUnavailable, got {other:?}"),
        }
        let envelope = err.into_envelope();
        assert_eq!(envelope.error_type_str(), "provider_unavailable");
        assert!(envelope.correctable && envelope.retryable);
    }

    #[test]
    fn crash_maps_to_internal_error() {
        let _g = guard();
        let mut sc =
            SidecarHandle::start(&script(), Duration::from_secs(15)).expect("sidecar starts");
        let err = sc
            .call("die", &json!({}), Duration::from_secs(5))
            .unwrap_err();
        match &err {
            SidecarError::Crashed { .. } => {}
            other => panic!("expected Crashed, got {other:?}"),
        }
        let envelope = err.into_envelope();
        assert_eq!(envelope.error_type_str(), "internal_error");
        assert!(envelope.retryable);
    }

    #[test]
    fn timeout_maps_to_resource_limit() {
        let _g = guard();
        let mut sc =
            SidecarHandle::start(&script(), Duration::from_secs(15)).expect("sidecar starts");
        let err = sc
            .call(
                "sleep_long",
                &json!({"ms": 3000}),
                Duration::from_millis(300),
            )
            .unwrap_err();
        let envelope = err.into_envelope();
        assert_eq!(envelope.error_type_str(), "resource_limit");
        assert!(envelope.retryable);
    }
}
