//! Fixture loading and comparison (ADR-0003). Fixtures are generated
//! independently by QuantLib C++ (fixture-generator/); this crate only reads
//! and compares. Test infrastructure, never part of the numerical core.

use serde_json::Value;
use std::path::Path;
use trustquant_core::contract::{NearZeroPolicy, ScaleType, ToleranceSpec};

pub struct Family {
    pub name: String,
    pub cases: Vec<Value>,
    pub provenance: Value,
}

pub fn load_family(root: &Path, name: &str) -> Family {
    let dir = root.join("fixtures").join(name);
    let read = |file: &str| {
        std::fs::read_to_string(dir.join(file))
            .unwrap_or_else(|e| panic!("read {dir:?}/{file}: {e}"))
    };
    let cases: Value = serde_json::from_str(&read("cases.json")).unwrap();
    let provenance: Value = serde_json::from_str(&read("provenance.json")).unwrap();
    Family {
        name: name.to_string(),
        cases: cases["cases"].as_array().expect("cases array").clone(),
        provenance,
    }
}

/// Compare a produced result against a fixture case under the case's own
/// tolerance. Exact metrics compare by JSON value; float metrics use
/// atol + rtol*|expected| with the case's near-zero policy if present.
pub fn compare_case(
    case_id: &str,
    expected: &Value,
    actual: &Value,
    tolerance: &Value,
) -> Result<(), String> {
    let mode = tolerance["mode"].as_str().unwrap_or("exact");
    let mut errors: Vec<String> = Vec::new();

    if mode == "mixed" {
        if let Some(exact_keys) = tolerance["exact_metrics"].as_array() {
            for k in exact_keys {
                let key = k.as_str().unwrap();
                match (expected.get(key), actual.get(key)) {
                    (Some(e), Some(a)) if e == a => {}
                    (Some(e), Some(a)) => {
                        errors.push(format!("{case_id}: exact metric '{key}' {a} != {e}"))
                    }
                    (Some(_), None) => {
                        errors.push(format!("{case_id}: metric '{key}' missing in actual"))
                    }
                    (None, _) => errors.push(format!(
                        "{case_id}: metric '{key}' not in expected (fixture bug)"
                    )),
                }
            }
        }
        if let Some(obj) = tolerance["float_metrics"].as_object() {
            for (key, spec) in obj {
                let e = expected.get(key).and_then(Value::as_f64).ok_or_else(|| {
                    format!("{case_id}: float metric '{key}' missing/not-float in expected")
                })?;
                let a = actual.get(key).and_then(Value::as_f64).ok_or_else(|| {
                    format!("{case_id}: float metric '{key}' missing/not-float in actual")
                })?;
                let tol = parse_spec(spec)
                    .map_err(|err| format!("{case_id}: bad tolerance for '{key}': {err}"))?;
                if let Err(err) = tol.check(a, e) {
                    errors.push(format!("{case_id}: float metric '{key}': {err}"));
                }
            }
        }
    } else {
        if let Some(exp) = expected.as_object() {
            for (k, v) in exp {
                match actual.get(k) {
                    Some(a) if a == v => {}
                    Some(a) => errors.push(format!("{case_id}: exact field '{k}' {a} != {v}")),
                    None => errors.push(format!("{case_id}: field '{k}' missing in actual")),
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn parse_spec(v: &Value) -> Result<ToleranceSpec, String> {
    let atol = v["atol"].as_f64().ok_or("atol missing")?;
    let rtol = v["rtol"].as_f64().ok_or("rtol missing")?;
    let scale_type = match v["scale_type"].as_str() {
        Some("expected_magnitude") => ScaleType::ExpectedMagnitude,
        Some("notional") => ScaleType::Notional,
        Some("annuity") => ScaleType::Annuity,
        Some("unit_price") => ScaleType::UnitPrice,
        other => return Err(format!("unknown scale_type {other:?}")),
    };
    let near_zero_policy = match &v["near_zero_policy"] {
        Value::Null => None,
        nz => Some(NearZeroPolicy {
            threshold: nz["threshold"]
                .as_f64()
                .ok_or("near_zero_policy.threshold missing")?,
            atol_floor: nz["atol_floor"]
                .as_f64()
                .ok_or("near_zero_policy.atol_floor missing")?,
        }),
    };
    Ok(ToleranceSpec {
        atol,
        rtol,
        scale_type,
        near_zero_policy,
    })
}
