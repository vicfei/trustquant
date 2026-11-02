//! trustquant Runtime: tool registration, validation, provider routing,
//! dataset store (content-addressed), manifest persistence (one file per
//! calculation, atomic write), replay, and the MCP message handler. All
//! entries (CLI / MCP / Web / library) reuse this single implementation.

pub mod dataset;
pub mod http;
pub mod mcp;
pub mod provider;
pub mod store;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::OnceLock;
use trustquant_core::contract::{ErrorEnvelope, RequestEnvelope, SCHEMA_VERSION, ToolOutput};
use trustquant_core::manifest::{self, CalculationManifest, ManifestContext};

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

    /// Canonical result digest — the equivalence key for the entry-point
    /// conformance tests.
    pub fn result_digest(&self) -> Option<String> {
        self.result
            .as_ref()
            .and_then(|r| trustquant_core::canonical::digest(r).ok())
    }
}

// ---------------- shared context ----------------

pub struct RuntimeCtx {
    pub manifest_store: store::ManifestStore,
    pub dataset_store: dataset::DatasetStore,
    pub providers: provider::ProviderManager,
}

impl RuntimeCtx {
    fn from_env() -> Self {
        let manifest_store = store::ManifestStore::default_store();
        let dataset_store = dataset::DatasetStore::new(manifest_store.base_dir());
        RuntimeCtx {
            manifest_store,
            dataset_store,
            providers: provider::ProviderManager::new(),
        }
    }
}

pub fn ctx() -> &'static RuntimeCtx {
    static CTX: OnceLock<RuntimeCtx> = OnceLock::new();
    CTX.get_or_init(RuntimeCtx::from_env)
}

// ---------------- tool execution ----------------

/// Execute a tool call (built-in Rust provider + external providers).
pub fn execute(tool: &str, params: &Value, request_id: Option<&str>) -> ResponseEnvelope {
    match dispatch(tool, params) {
        Ok(out) => ResponseEnvelope::success(&out, request_id),
        Err(e) => ResponseEnvelope::error(e, request_id),
    }
}

fn dispatch(tool: &str, params: &Value) -> Result<ToolOutput, ErrorEnvelope> {
    match tool {
        "price_european_option" => trustquant_core::dispatch(tool, params),
        "load_market_dataset" => load_market_dataset_tool(params),
        "analyze_portfolio_risk" => with_dataset(params, |panel, p| {
            trustquant_core::portfolio::analyze_portfolio_risk_tool(panel, p)
        }),
        "stress_test_portfolio" => with_dataset(params, |panel, p| {
            trustquant_core::portfolio::stress_test_tool(panel, p)
        }),
        "optimize_portfolio" => optimize_tool(params),
        other => Err(ErrorEnvelope {
            error_type: trustquant_core::contract::ErrorType::UnsupportedFeature,
            message: format!("unknown tool '{other}'"),
            field_path: Some("tool".to_string()),
            correctable: false,
            retryable: false,
            detail: None,
        }),
    }
}

fn with_dataset(
    params: &Value,
    f: impl FnOnce(&trustquant_core::portfolio::PricePanel, &Value) -> Result<ToolOutput, ErrorEnvelope>,
) -> Result<ToolOutput, ErrorEnvelope> {
    let id = params
        .get("dataset")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ErrorEnvelope::invalid_input(
                "missing field 'dataset' (dataset_id from load_market_dataset)".into(),
                "dataset",
            )
        })?;
    let ds = ctx().dataset_store.load(id)?;
    let panel = ds.panel();
    let mut out = f(&panel, params)?;
    // Pin provenance into the result so manifests replay against content.
    out.result["dataset_id"] = json!(ds.dataset_id);
    Ok(out)
}

fn load_market_dataset_tool(params: &Value) -> Result<ToolOutput, ErrorEnvelope> {
    let body = match (
        params.get("csv_body"),
        params.get("builtin"),
        params.get("path"),
    ) {
        (Some(inline), None, None) => inline.as_str().unwrap_or_default().to_string(),
        (None, _, _) if params.get("builtin").is_some() || params.get("path").is_some() => {
            match (params.get("builtin"), params.get("path")) {
                (Some(b), None) => {
                    let name = b.as_str().unwrap_or_default();
                    let path = match name {
                        "demo_prices" => std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                            .join("../../data/demo_prices.csv"),
                        _ => {
                            return Err(ErrorEnvelope::unsupported_feature(format!(
                                "unknown builtin dataset '{name}'; available: demo_prices"
                            )));
                        }
                    };
                    std::fs::read_to_string(&path).map_err(|e| {
                        ErrorEnvelope::invalid_input(
                            format!("cannot read builtin dataset: {e}"),
                            "builtin",
                        )
                    })?
                }
                (None, Some(p)) => {
                    let path = p.as_str().unwrap_or_default();
                    std::fs::read_to_string(path).map_err(|e| {
                        ErrorEnvelope::invalid_input(format!("cannot read '{path}': {e}"), "path")
                    })?
                }
                _ => {
                    return Err(ErrorEnvelope::invalid_input(
                        "provide exactly one of builtin | path | csv_body".to_string(),
                        "builtin",
                    ));
                }
            }
            .to_string()
        }
        _ => {
            return Err(ErrorEnvelope::invalid_input(
                "provide exactly one of builtin | path | csv_body".to_string(),
                "builtin",
            ));
        }
    };
    let source = params
        .get("path")
        .and_then(Value::as_str)
        .or_else(|| params.get("builtin").and_then(Value::as_str))
        .unwrap_or("inline")
        .to_string();
    let ds = dataset::parse_csv(&body, &source)?;
    ctx()
        .dataset_store
        .save(&ds)
        .map_err(|e| ErrorEnvelope::internal(format!("dataset store write failed: {e}")))?;
    let result = json!({
        "dataset_id": ds.dataset_id,
        "tickers": ds.tickers,
        "dates_count": ds.dates.len(),
        "first_date": ds.dates.first(),
        "last_date": ds.dates.last(),
        "source": ds.source_note,
        "conventions": {
            "schema": "date,ticker,close",
            "validation": "ADR-0005 (ISO dates ascending, no dupes, positive closes, UTF-8)",
            "identity": "content-addressed (BLAKE3 of canonical body)",
        },
    });
    Ok(ToolOutput {
        engine_id: "dataset.v1",
        provider_id: "runtime",
        result,
        warnings: vec![],
    })
}

fn optimize_tool(params: &Value) -> Result<ToolOutput, ErrorEnvelope> {
    let objective = params
        .get("objective")
        .and_then(Value::as_str)
        .unwrap_or("min_volatility");
    let risk_free = params
        .get("risk_free_rate")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let max_weight = params
        .get("max_weight")
        .and_then(Value::as_f64)
        .unwrap_or(1.0);

    // Prices: either an inline {ticker: [closes]} map or a dataset reference.
    let (tickers, prices, dataset_id) =
        if let Some(inline) = params.get("prices").and_then(Value::as_object) {
            let mut tickers: Vec<String> = inline.keys().cloned().collect();
            tickers.sort();
            let prices: serde_json::Map<String, Value> = tickers
                .iter()
                .map(|t| (t.clone(), inline[t].clone()))
                .collect();
            (tickers, Value::Object(prices), None)
        } else {
            let id = params
                .get("dataset")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ErrorEnvelope::invalid_input(
                        "provide 'dataset' (dataset_id) or inline 'prices'".into(),
                        "dataset",
                    )
                })?;
            let ds = ctx().dataset_store.load(id)?;
            (
                ds.tickers.clone(),
                json!(ds.close),
                Some(ds.dataset_id.clone()),
            )
        };

    let side_params = json!({
        "tickers": tickers,
        "prices": prices,
        "objective": objective,
        "risk_free_rate": risk_free,
        "max_weight": max_weight,
    });
    let mut result = ctx().providers.call("optimize_portfolio", &side_params)?;
    if let Some(id) = dataset_id {
        result["dataset_id"] = json!(id);
    }
    result["conventions"] = json!({
        "provider": "python-sidecar (PyPortfolioOpt, pinned env)",
        "long_only": true,
        "replay": "same-version same-environment only",
    });
    Ok(ToolOutput {
        engine_id: "portfolio_optimize.v1",
        provider_id: "python-sidecar",
        result,
        warnings: vec![],
    })
}

// ---------------- manifest recording + replay ----------------

/// Execute AND persist a manifest (atomic write, ADR-0004 §2).
pub fn execute_and_record(
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
    let provider_version = if request.tool == "optimize_portfolio" {
        provider_version_string()
    } else {
        trustquant_core::CORE_VERSION.to_string()
    };
    let recorded = record_execution(
        &request_json,
        &request.tool,
        &request.params,
        &mut envelope,
        &provider_version,
    );
    (envelope, recorded)
}

fn provider_version_string() -> String {
    let d = ctx().providers.describe();
    let pkgs = &d["environment"]["packages"];
    format!(
        "python-sidecar/{} pypfopt={} cvxpy={}",
        d["environment"]["python"].as_str().unwrap_or("?"),
        pkgs["pypfopt"].as_str().unwrap_or("?"),
        pkgs["cvxpy"].as_str().unwrap_or("?"),
    )
}

fn record_execution(
    request_json: &Value,
    tool: &str,
    params: &Value,
    envelope: &mut ResponseEnvelope,
    provider_version: &str,
) -> Option<CalculationManifest> {
    let built = match dispatch(tool, params) {
        Ok(out) => manifest::build(
            &ManifestContext {
                calculation_id: &store::new_calculation_id(),
                runtime_version: RUNTIME_VERSION,
                provider_version,
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
                    provider_version,
                },
                request_json,
                tool,
                &out,
            )
        }
    };
    match built
        .map_err(std::io::Error::other)
        .and_then(|m| ctx().manifest_store.save(&m).map(|_| m))
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

/// `replay_calculation`: load bundle, re-execute, verify digests. Missing
/// manifest -> replay_unavailable; missing referenced dataset ->
/// artifact_unavailable (surfaced by dispatch when it loads the dataset).
pub fn replay_calculation(calculation_id: &str) -> Result<ReplayReport, ErrorEnvelope> {
    let m = ctx()
        .manifest_store
        .load(calculation_id)
        .map_err(|store::LoadError::NotFound| {
            ErrorEnvelope::replay_unavailable(format!(
                "no manifest for calculation_id '{calculation_id}' in {}",
                ctx().manifest_store.calculations_dir().display()
            ))
        })?;
    let integrity = manifest::verify_integrity(&m);
    if integrity.is_err() {
        return Err(ErrorEnvelope::artifact_unavailable(format!(
            "manifest '{calculation_id}' failed integrity verification"
        )));
    }
    let tool = m.request["tool"].as_str().unwrap_or_default().to_string();
    let params = m.request["params"].clone();
    let outcome = dispatch(&tool, &params);
    let recomputed = match &outcome {
        Ok(out) => manifest::result_digest_of(&out.result),
        Err(e) => manifest::result_digest_of(&serde_json::to_value(e).unwrap_or(Value::Null)),
    }
    .unwrap_or_else(|e| format!("digest-error:{e}"));
    Ok(ReplayReport {
        calculation_id: m.calculation_id.clone(),
        tool,
        manifest_hash_valid: integrity.is_ok(),
        result_digest_match: recomputed == m.result_digest,
        recomputed_result_digest: recomputed,
        stored_result_digest: m.result_digest.clone(),
        detail: None,
    })
}

/// `get_calculation_manifest` tool.
pub fn get_manifest(calculation_id: &str) -> Result<Value, ErrorEnvelope> {
    let m = ctx()
        .manifest_store
        .load(calculation_id)
        .map_err(|store::LoadError::NotFound| {
            ErrorEnvelope::replay_unavailable(format!(
                "no manifest for calculation_id '{calculation_id}'"
            ))
        })?;
    manifest::verify_integrity(&m)
        .map_err(|e| ErrorEnvelope::artifact_unavailable(format!("integrity check failed: {e}")))?;
    serde_json::to_value(&m).map_err(|e| ErrorEnvelope::internal(format!("serialization: {e}")))
}

pub fn known_tools() -> &'static [&'static str] {
    &[
        "price_european_option",
        "load_market_dataset",
        "optimize_portfolio",
        "analyze_portfolio_risk",
        "stress_test_portfolio",
        "get_calculation_manifest",
        "replay_calculation",
    ]
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
                    "day_counter": {"type": "string", "enum": ["Actual360", "Actual365Fixed", "Thirty360USA", "Thirty360European", "ActualActualISDA", "ActualActualISMA"], "description": "default Actual365Fixed"}
                }
            }
        },
        {
            "name": "load_market_dataset",
            "description": "Load and validate a market price dataset (CSV: date,ticker,close) into a content-addressed snapshot. Returns dataset_id used by portfolio tools. Validation per ADR-0005.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "builtin": {"type": "string", "enum": ["demo_prices"], "description": "shipped synthetic demo dataset"},
                    "path": {"type": "string", "description": "path to a CSV file"},
                    "csv_body": {"type": "string", "description": "inline CSV content (exactly one of builtin/path/csv_body)"}
                }
            }
        },
        {
            "name": "optimize_portfolio",
            "description": "Long-only portfolio optimization (min_volatility or max_sharpe with weight bounds) on a loaded dataset or inline prices. Executed by the external Python provider (PyPortfolioOpt); the result records provider versions. Provider absent -> provider_unavailable, never a silent fallback.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dataset": {"type": "string", "description": "dataset_id from load_market_dataset"},
                    "prices": {"type": "object", "description": "inline {ticker: [closes]}"},
                    "objective": {"type": "string", "enum": ["min_volatility", "max_sharpe"], "description": "default min_volatility"},
                    "risk_free_rate": {"type": "number", "description": "annualized, for max_sharpe (default 0)"},
                    "max_weight": {"type": "number", "description": "per-asset cap in (0,1], default 1"}
                }
            }
        },
        {
            "name": "analyze_portfolio_risk",
            "description": "Portfolio risk metrics (total/annualized return, annualized volatility, max drawdown, Sharpe) on a loaded dataset with explicit weights. Conventions: daily simple returns, daily-rebalanced fixed weights, 252-day year, ddof=1.",
            "inputSchema": {
                "type": "object",
                "required": ["dataset", "weights"],
                "properties": {
                    "dataset": {"type": "string"},
                    "weights": {"type": "object", "description": "{ticker: weight}; normalized to sum 1"},
                    "risk_free_rate": {"type": "number", "description": "default 0"}
                }
            }
        },
        {
            "name": "stress_test_portfolio",
            "description": "Direct price-shock stress scenarios on a portfolio's last close (baseline per unit notional). Contributions sum to the scenario return.",
            "inputSchema": {
                "type": "object",
                "required": ["dataset", "weights", "scenarios"],
                "properties": {
                    "dataset": {"type": "string"},
                    "weights": {"type": "object"},
                    "scenarios": {"type": "array", "items": {"type": "object", "properties": {"name": {"type": "string"}, "shocks": {"type": "object"}}}}
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
            "description": "Re-execute a recorded calculation and report digest differences. Same-version same-environment promise; missing records return replay_unavailable, missing datasets artifact_unavailable.",
            "inputSchema": {"type": "object", "required": ["calculation_id"], "properties": {"calculation_id": {"type": "string"}}}
        }
    ])
}
