//! External provider routing (ADR-0002): one long-lived Python sidecar per
//! process, lazily started on first use, respawned after a crash. Explicit
//! unavailability, never a silent fallback to a built-in optimizer.

use serde_json::{Value, json};
use std::sync::Mutex;
use std::time::Duration;
use trustquant_core::contract::ErrorEnvelope as RtErrorEnvelope;
use trustquant_sidecar::{PROTOCOL_VERSION, SidecarError, SidecarHandle};

pub struct ProviderManager {
    inner: Mutex<Option<SidecarHandle>>,
}

impl ProviderManager {
    pub const fn new() -> Self {
        ProviderManager {
            inner: Mutex::new(None),
        }
    }

    fn script_path() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../sidecar/trustquant_provider.py")
    }

    /// Execute one tool call on the (reused) sidecar, mapping channel and
    /// provider errors into the contract taxonomy.
    pub fn call(&self, tool: &str, params: &Value) -> Result<Value, RtErrorEnvelope> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        const CALL_TIMEOUT: Duration = Duration::from_secs(60);
        let mut attempt = 0;
        loop {
            attempt += 1;
            if guard.is_none() {
                match SidecarHandle::start(&Self::script_path(), Duration::from_secs(20)) {
                    Ok(handle) => *guard = Some(handle),
                    Err(e @ SidecarError::ProviderUnavailable(_)) => return Err(e.into_envelope()),
                    Err(e) => return Err(e.into_envelope()),
                }
            }
            let handle = guard.as_mut().expect("just set");
            match handle.call(tool, params, CALL_TIMEOUT) {
                Ok(result) => return Ok(result),
                Err(e) => {
                    // Channel died with the handle: drop it and retry once
                    // (fresh process). Provider-side refusals arrive as
                    // structured errors below, not here.
                    let fatal = matches!(
                        e,
                        SidecarError::Crashed { .. } | SidecarError::Timeout { .. }
                    );
                    if !fatal || attempt >= 2 {
                        return Err(e.into_envelope());
                    }
                    *guard = None;
                }
            }
        }
    }

    /// Provider identity + versions for manifests (spawns once, cached).
    pub fn describe(&self) -> Value {
        match self.call("meta.version", &json!({})) {
            Ok(env) => json!({
                "provider_id": "python-sidecar",
                "protocol": PROTOCOL_VERSION,
                "environment": env,
            }),
            Err(e) => json!({
                "provider_id": "python-sidecar",
                "protocol": PROTOCOL_VERSION,
                "unavailable": true,
                "error_type": e.error_type_str(),
            }),
        }
    }
}

impl Default for ProviderManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Literal (non-collinear) series; this test checks channel plumbing and
    /// constraint invariants only — exact weights are arbitrated by the
    /// optimize fixtures against the independent python oracle.
    fn series() -> serde_json::Value {
        let p: Vec<f64> = (0..40)
            .map(|i| 100.0 * (1.0 + 0.001 * i as f64 + 0.02 * ((i % 7) as f64 - 3.0) / 3.0))
            .collect();
        let q: Vec<f64> = (0..40)
            .map(|i| 50.0 * (1.0 + 0.0015 * i as f64 + 0.05 * ((i % 5) as f64 - 2.0) / 2.0))
            .collect();
        json!({"P": p, "Q": q})
    }

    #[test]
    fn optimize_via_provider_satisfies_constraints() {
        let pm = ProviderManager::new();
        let params = json!({
            "tickers": ["P", "Q"],
            "prices": series(),
            "objective": "min_volatility",
            "max_weight": 1.0,
        });
        let result = pm
            .call("optimize_portfolio", &params)
            .expect("sidecar optimize");
        let w = &result["weights"];
        let wp = w["P"].as_f64().unwrap();
        let wq = w["Q"].as_f64().unwrap();
        let sum = wp + wq;
        assert!((sum - 1.0).abs() < 1e-4, "weights sum {sum}");
        for (name, v) in [("P", wp), ("Q", wq)] {
            assert!(
                (-1e-6..=1.0 + 1e-6).contains(&v),
                "weight bound violated: {name}={v}"
            );
        }
        assert!(result["volatility"].as_f64().unwrap() > 0.0);
    }

    #[test]
    fn provider_reports_environment_versions() {
        let pm = ProviderManager::new();
        let d = pm.describe();
        assert_eq!(d["provider_id"], "python-sidecar");
        let pkgs = &d["environment"]["packages"];
        assert!(
            pkgs["pypfopt"].as_str().is_some_and(|v| v != "absent"),
            "pypfopt must be present in the test env"
        );
    }
}
