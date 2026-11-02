//! florin-core: immutable numerical core. No I/O, no async, no global
//! state (v2 architecture §3.1). Entry point: `dispatch`.

pub mod bs;
pub mod canonical;
pub mod contract;
pub mod date;
pub mod daycount;
pub mod manifest;

use contract::{ErrorEnvelope, ErrorType, ToolOutput};
use serde_json::Value;

pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Dispatch a tool executed by the built-in Rust provider. Pure function:
/// same inputs -> same ToolOutput.
pub fn dispatch(tool: &str, params: &Value) -> Result<ToolOutput, ErrorEnvelope> {
    match tool {
        "price_european_option" => bs::price_european_option_tool(params),
        other => Err(ErrorEnvelope {
            error_type: ErrorType::UnsupportedFeature,
            message: format!("tool '{other}' is not served by the built-in provider"),
            field_path: Some("tool".to_string()),
            correctable: false,
            retryable: false,
            detail: None,
        }),
    }
}

pub const BUILTIN_TOOLS: &[&str] = &["price_european_option"];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dispatch_success_and_unknown_tool() {
        let out = dispatch(
            "price_european_option",
            &json!({
                "option_type": "call", "spot": 100.0, "strike": 105.0,
                "risk_free_rate": 0.05, "dividend_yield": 0.02, "volatility": 0.2,
                "valuation_date": "2023-06-21", "maturity_date": "2024-06-20"
            }),
        )
        .unwrap();
        assert_eq!(out.engine_id, "bs_analytic.v1");
        assert_eq!(out.provider_id, "builtin-rust");
        assert!(out.result["npv"].as_f64().unwrap() > 0.0);

        let err = dispatch("optimize_portfolio", &json!({})).unwrap_err();
        assert_eq!(err.error_type_str(), "unsupported_feature");
    }

    #[test]
    fn american_style_rejected_without_fabrication() {
        let err = dispatch(
            "price_european_option",
            &json!({
                "option_type": "call", "spot": 100.0, "strike": 100.0,
                "risk_free_rate": 0.05, "volatility": 0.2,
                "valuation_date": "2023-06-21", "maturity_date": "2024-06-20",
                "exercise_style": "american"
            }),
        )
        .unwrap_err();
        assert_eq!(err.error_type_str(), "unsupported_feature");
        assert!(err.message.contains("no value was computed"));
    }
}
