//! CalculationManifest (ADR-0004): execution record for one calculation.
//!
//! calculation_id  — execution identity, minted by the Runtime,
//!                   NOT derivable from content, NOT an integrity check.
//! manifest_hash   — BLAKE3 over the canonical manifest body EXCLUDING the
//!                   `manifest_hash` field and the `environment` block.
//! The manifest is the execution record; the Replay Bundle is manifest +
//! referenced inputs. For self-contained requests the manifest file IS the
//! bundle (ADR-0004 §1).

use crate::canonical::digest;
use crate::contract::{SCHEMA_VERSION, ToolOutput};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnvironmentBlock {
    pub os: String,
    pub arch: String,
    pub rustc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalculationManifest {
    pub schema_version: String,
    pub calculation_id: String,
    pub tool: String,
    pub engine_id: String,
    pub provider_id: String,
    pub provider_version: String,
    pub core_version: String,
    pub runtime_version: String,
    pub canonicalization_version: u32,
    pub request: Value,
    pub input_digest: String,
    pub result_digest: String,
    pub environment: EnvironmentBlock,
    pub manifest_hash: String,
}

pub fn current_environment() -> EnvironmentBlock {
    EnvironmentBlock {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        rustc: option_env!("FLORIN_RUSTC_VERSION")
            .unwrap_or("not-recorded")
            .to_string(),
    }
}

pub struct ManifestContext<'a> {
    pub calculation_id: &'a str,
    pub runtime_version: &'a str,
    pub provider_version: &'a str,
}

/// Assemble + hash a manifest for one execution (core is pure; the Runtime
/// supplies identity and versions).
pub fn build(
    ctx: &ManifestContext,
    request: &Value,
    tool: &str,
    output: &ToolOutput,
) -> Result<CalculationManifest, String> {
    let body = json!({
        "schema_version": SCHEMA_VERSION,
        "calculation_id": ctx.calculation_id,
        "tool": tool,
        "engine_id": output.engine_id,
        "provider_id": output.provider_id,
        "provider_version": ctx.provider_version,
        "core_version": CORE_VERSION,
        "runtime_version": ctx.runtime_version,
        "canonicalization_version": crate::canonical::CANONICALIZATION_VERSION,
        "request": request,
        "input_digest": digest(request)?,
        "result_digest": digest(&output.result)?,
    });
    let manifest_hash = digest(&body)?;
    Ok(CalculationManifest {
        schema_version: SCHEMA_VERSION.to_string(),
        calculation_id: ctx.calculation_id.to_string(),
        tool: tool.to_string(),
        engine_id: output.engine_id.to_string(),
        provider_id: output.provider_id.to_string(),
        provider_version: ctx.provider_version.to_string(),
        core_version: CORE_VERSION.to_string(),
        runtime_version: ctx.runtime_version.to_string(),
        canonicalization_version: crate::canonical::CANONICALIZATION_VERSION,
        request: request.clone(),
        input_digest: body["input_digest"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        result_digest: body["result_digest"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        environment: current_environment(),
        manifest_hash,
    })
}

/// Recompute manifest_hash from the manifest's own content (minus the hash
/// field and the environment block) and compare.
pub fn verify_integrity(m: &CalculationManifest) -> Result<(), String> {
    let body = json!({
        "schema_version": m.schema_version,
        "calculation_id": m.calculation_id,
        "tool": m.tool,
        "engine_id": m.engine_id,
        "provider_id": m.provider_id,
        "provider_version": m.provider_version,
        "core_version": m.core_version,
        "runtime_version": m.runtime_version,
        "canonicalization_version": m.canonicalization_version,
        "request": m.request,
        "input_digest": m.input_digest,
        "result_digest": m.result_digest,
    });
    let expected = digest(&body)?;
    if expected == m.manifest_hash {
        Ok(())
    } else {
        Err(format!(
            "manifest_hash mismatch: stored {}, recomputed {}",
            m.manifest_hash, expected
        ))
    }
}

pub fn result_digest_of(result: &Value) -> Result<String, String> {
    digest(result)
}

pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::ToolOutput;
    use serde_json::json;

    #[test]
    fn hash_excludes_environment_and_hash_itself() {
        let req = json!({"tool":"price_european_option","params":{"spot":100}});
        let out = ToolOutput {
            engine_id: "bs_analytic.v1",
            provider_id: "builtin-rust",
            result: json!({"npv": 1.0}),
            warnings: vec![],
        };
        let ctx = ManifestContext {
            calculation_id: "id-1",
            runtime_version: "0.0.1",
            provider_version: "0.0.1",
        };
        let m1 = build(&ctx, &req, "price_european_option", &out).unwrap();
        let mut m2 = m1.clone();
        m2.environment.rustc = "different".into();
        assert_eq!(
            m1.manifest_hash, m2.manifest_hash,
            "environment excluded from hash domain"
        );
        assert!(verify_integrity(&m1).is_ok());
        m2.result_digest = "tampered".into();
        assert!(verify_integrity(&m2).is_err());
    }
}
