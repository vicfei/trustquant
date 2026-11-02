//! finstrument Runtime: tool registration, validation, provider routing,
//! manifest persistence (one file per calculation, atomic write), replay,
//! and the MCP message handler. All entries (CLI / MCP / library) reuse this
//! single implementation (ADR-0006 gate 1).

pub mod mcp;
pub mod store;

use finstrument_core::contract::{ErrorEnvelope, RequestEnvelope, SCHEMA_VERSION, ToolOutput};
use finstrument_core::manifest::{self, CalculationManifest, ManifestContext};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const RUNTIME_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Full response envelope produced by the Runtime layer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResponseEnvelope {
    pub schema_version: String,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorEnvelope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calculation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_hash: Option<String>,
}

impl ResponseEnvelope {
    pub fn success(out: &ToolOutput, request_id: Option<&str>) -> Self {
        ResponseEnvelope {
            schema_version: SCHEMA_VERSION.to_string(),
            status: "success",
            request_id: request_id.map(str::to_string),
            result: Some(out.result.clone()),
            warnings: out.warnings.clone(),
            error: None,
            engine_id: Some(out.engine_id.to_string()),
            provider_id: Some(out.provider_id.to_string()),
            calculation_id: None,
            manifest_hash: None,
        }
    }

    pub fn error(e: ErrorEnvelope, request_id: Option<&str>) -> Self {
        ResponseEnvelope {
            schema_version: SCHEMA_VERSION.to_string(),
            status: "error",
            request_id: request_id.map(str::to_string),
            result: None,
            warnings: vec![],
            error: Some(e),
            engine_id: None,
            provider_id: None,
            calculation_id: None,
            manifest_hash: None,
        }
    }

    /// Canonical result digest — the equivalence key for the three-entry
    /// conformance test (gate 1).
    pub fn result_digest(&self) -> Option<String> {
        self.result
            .as_ref()
            .and_then(|r| finstrument_core::canonical::digest(r).ok())
    }
}

/// Execute a tool through the built-in Rust provider (M0 routing table).
pub fn execute(tool: &str, params: &Value, request_id: Option<&str>) -> ResponseEnvelope {
    match finstrument_core::dispatch(tool, params) {
        Ok(out) => ResponseEnvelope::success(&out, request_id),
        Err(e) => ResponseEnvelope::error(e, request_id),
    }
}

/// Execute AND persist a manifest (atomic write, ADR-0004 §2).
pub fn execute_and_record(
    store: &store::ManifestStore,
    request: &RequestEnvelope,
) -> (ResponseEnvelope, Option<CalculationManifest>) {
    let request_json = json!({
        "schema_version": request.schema_version,
        "tool": request.tool,
        "params": request.params,
    });
    let mut envelope = execute(
        &request.tool,
        &request.params,
        request.request_id.as_deref(),
    );
    let recorded = record_execution(
        store,
        &request_json,
        &request.tool,
        &request.params,
        &mut envelope,
    );
    (envelope, recorded)
}

fn record_execution(
    store: &store::ManifestStore,
    request_json: &Value,
    tool: &str,
    params: &Value,
    envelope: &mut ResponseEnvelope,
) -> Option<CalculationManifest> {
    let provider_version = finstrument_core::CORE_VERSION.to_string();
    let built = match finstrument_core::dispatch(tool, params) {
        Ok(out) => manifest::build(
            &ManifestContext {
                calculation_id: &store::new_calculation_id(),
                runtime_version: RUNTIME_VERSION,
                provider_version: &provider_version,
            },
            request_json,
            tool,
            &out,
        ),
        Err(e) => {
            let out = ToolOutput {
                engine_id: "none",
                provider_id: "builtin-rust",
                result: serde_json::to_value(&e).unwrap_or(Value::Null),
                warnings: vec![],
            };
            manifest::build(
                &ManifestContext {
                    calculation_id: &store::new_calculation_id(),
                    runtime_version: RUNTIME_VERSION,
                    provider_version: &provider_version,
                },
                request_json,
                tool,
                &out,
            )
        }
    };
    match built
        .map_err(std::io::Error::other)
        .and_then(|m| store.save(&m).map(|_| m))
    {
        Ok(m) => {
            envelope.calculation_id = Some(m.calculation_id.clone());
            envelope.manifest_hash = Some(m.manifest_hash.clone());
            Some(m)
        }
        Err(io_err) => {
            envelope
                .warnings
                .push(format!("manifest could not be persisted: {io_err}"));
            None
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ReplayReport {
    pub calculation_id: String,
    pub tool: String,
    pub manifest_hash_valid: bool,
    pub result_digest_match: bool,
    pub recomputed_result_digest: String,
    pub stored_result_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ReplayReport {
    pub fn passed(&self) -> bool {
        self.manifest_hash_valid && self.result_digest_match
    }
}

/// `replay_calculation` tool: load bundle, re-execute, verify (gate 5).
/// Missing manifest -> replay_unavailable; corrupted/missing referenced
/// inputs -> artifact_unavailable (never "found manifest" == "replayable").
pub fn replay_calculation(
    store: &store::ManifestStore,
    calculation_id: &str,
) -> Result<ReplayReport, ErrorEnvelope> {
    let manifest = store
        .load(calculation_id)
        .map_err(|store::LoadError::NotFound| {
            ErrorEnvelope::replay_unavailable(format!(
                "no manifest for calculation_id '{calculation_id}' in {}",
                store.calculations_dir().display()
            ))
        })?;
    let integrity = manifest::verify_integrity(&manifest);
    if integrity.is_err() {
        return Err(ErrorEnvelope::artifact_unavailable(format!(
            "manifest '{calculation_id}' failed integrity verification"
        )));
    }
    let tool = manifest.request["tool"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let params = manifest.request["params"].clone();
    let outcome = finstrument_core::dispatch(&tool, &params);
    let recomputed = match &outcome {
        Ok(out) => manifest::result_digest_of(&out.result),
        Err(e) => manifest::result_digest_of(&serde_json::to_value(e).unwrap_or(Value::Null)),
    }
    .unwrap_or_else(|e| format!("digest-error:{e}"));
    Ok(ReplayReport {
        calculation_id: manifest.calculation_id.clone(),
        tool,
        manifest_hash_valid: integrity.is_ok(),
        result_digest_match: recomputed == manifest.result_digest,
        recomputed_result_digest: recomputed,
        stored_result_digest: manifest.result_digest.clone(),
        detail: None,
    })
}

/// `get_calculation_manifest` tool.
pub fn get_manifest(
    store: &store::ManifestStore,
    calculation_id: &str,
) -> Result<Value, ErrorEnvelope> {
    let manifest = store
        .load(calculation_id)
        .map_err(|store::LoadError::NotFound| {
            ErrorEnvelope::replay_unavailable(format!(
                "no manifest for calculation_id '{calculation_id}'"
            ))
        })?;
    manifest::verify_integrity(&manifest)
        .map_err(|e| ErrorEnvelope::artifact_unavailable(format!("integrity check failed: {e}")))?;
    serde_json::to_value(&manifest)
        .map_err(|e| ErrorEnvelope::internal(format!("serialization: {e}")))
}

pub fn tool_descriptors() -> Value {
    json!([
        {
            "name": "price_european_option",
            "description": "Price a European option with the analytic Black-Scholes-Merton engine and return NPV + Delta/Gamma/Vega/Theta with per-metric unit semantics. Vega is per 1.00 volatility; Theta is per year; rates are continuously compounded.",
            "inputSchema": {
                "type": "object",
                "required": ["option_type", "spot", "strike", "risk_free_rate", "volatility", "valuation_date", "maturity_date"],
                "properties": {
                    "option_type": {"type": "string", "enum": ["call", "put"]},
                    "exercise_style": {"type": "string", "enum": ["european"], "description": "american etc. return unsupported_feature; no value is fabricated"},
                    "spot": {"type": "number", "description": "underlying spot, > 0"},
                    "strike": {"type": "number", "description": "strike, > 0"},
                    "risk_free_rate": {"type": "number", "description": "continuously compounded"},
                    "dividend_yield": {"type": "number", "description": "continuously compounded, default 0"},
                    "volatility": {"type": "number", "description": ">= 0"},
                    "valuation_date": {"type": "string", "description": "ISO YYYY-MM-DD, years 1901-2199"},
                    "maturity_date": {"type": "string", "description": "ISO YYYY-MM-DD, after valuation_date"},
                    "day_counter": {"type": "string", "enum": ["Actual360", "Actual365Fixed", "Thirty360USA", "Thirty360European", "ActualActualISDA", "ActualActualISMA"], "description": "default Actual365Fixed; ISMA requires ref_period_* fields"}
                }
            }
        },
        {
            "name": "get_calculation_manifest",
            "description": "Fetch the full execution record (manifest) of a calculation by calculation_id. Verifies manifest_hash before returning.",
            "inputSchema": {"type": "object", "required": ["calculation_id"], "properties": {"calculation_id": {"type": "string"}}}
        },
        {
            "name": "replay_calculation",
            "description": "Re-execute a recorded calculation and report digest differences. Same-version same-environment promise; missing records return replay_unavailable.",
            "inputSchema": {"type": "object", "required": ["calculation_id"], "properties": {"calculation_id": {"type": "string"}}}
        }
    ])
}

pub fn known_tools() -> &'static [&'static str] {
    &[
        "price_european_option",
        "get_calculation_manifest",
        "replay_calculation",
    ]
}
