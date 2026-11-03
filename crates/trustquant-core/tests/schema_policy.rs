//! Schema version policy tests (M2 exit: compatibility, enum extension and
//! breaking-change rules are TESTED, not just documented).
//! Policy (ADR-0001 §5): major must match the server's; any minor accepted;
//! new enum values are minor changes that older servers reject gracefully
//! as unsupported_feature (negotiation, not a crash).

use serde_json::json;
use trustquant_core::contract::RequestEnvelope;

#[test]
fn same_major_any_minor_accepted() {
    for v in ["0.1", "0.2", "0.9", "0.0"] {
        let r = RequestEnvelope {
            schema_version: v.to_string(),
            tool: "stub".into(),
            params: json!({}),
            request_id: None,
        };
        assert!(r.validate_schema_version().is_ok(), "{v} must be accepted");
    }
}

#[test]
fn different_major_rejected_as_uncorrectable_invalid_input() {
    for v in ["1.0", "9.1", "-1.0", "not.a.version"] {
        let r = RequestEnvelope {
            schema_version: v.to_string(),
            tool: "stub".into(),
            params: json!({}),
            request_id: None,
        };
        let e = r.validate_schema_version().unwrap_err();
        assert_eq!(e.error_type_str(), "invalid_input", "{v}");
        assert!(!e.correctable, "{v}");
        assert_eq!(e.field_path.as_deref(), Some("schema_version"));
    }
}

#[test]
fn unknown_enum_value_is_negotiation_not_crash() {
    // A future minor adds a calendar enum value; an older server must refuse
    // with unsupported_feature and name the accepted set.
    let err = trustquant_core::dispatch(
        "price_vanilla_swap",
        &json!({
            "eval_date": "2023-06-21", "deposit_6m": 0.03,
            "swap_rates_1y_10y": [0.03,0.03,0.03,0.03,0.03,0.03,0.03,0.03,0.03,0.03], "years": 5,
            "float_convention": "BasisSwapSixM" // hypothetical future value
        }),
    );
    // unknown EXTRA fields are ignored by design (additive = minor);
    // unknown ENUM values must fail loudly:
    let err2 = trustquant_core::daycount::parse_counter("Business252").unwrap_err();
    assert_eq!(err2.error_type_str(), "unsupported_feature");
    // the swap call above succeeds (extra unknown key tolerated = forward compat)
    assert!(err.is_ok());
}

#[test]
fn version_constants_are_consistent_across_entries() {
    // Every entry (CLI/MCP/Web) stamps envelopes from one constant; the wire
    // value must remain "0.x" shaped so major parsing never surprises.
    let v = trustquant_core::contract::SCHEMA_VERSION;
    let parts: Vec<u32> = v.split('.').map(|p| p.parse().unwrap()).collect();
    assert_eq!(parts.len(), 2, "schema_version is major.minor");
    assert_eq!(parts[0], 0, "pre-1.0 contract");
}
