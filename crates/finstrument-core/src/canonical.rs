//! Canonical serialization and stable digests (ADR-0003).
//!
//! canonicalization v1: serde_json compact form; object keys sorted
//! (BTreeMap semantics — the `preserve_order` feature must stay OFF);
//! floats via shortest round-trip; NaN/Inf rejected; strings as-is.
//! Digest: BLAKE3 over canonical bytes. std's DefaultHasher is explicitly
//! banned for digests (not stable across Rust versions).

use serde_json::Value;

pub const CANONICALIZATION_VERSION: u32 = 1;

pub fn canonical_json(v: &Value) -> Result<String, String> {
    if let Some(f) = find_non_finite(v) {
        return Err(format!(
            "non-finite float '{f}' cannot be canonically serialized"
        ));
    }
    serde_json::to_string(v).map_err(|e| format!("serialization failed: {e}"))
}

pub fn digest(v: &Value) -> Result<String, String> {
    let s = canonical_json(v)?;
    Ok(blake3::hash(s.as_bytes()).to_hex().to_string())
}

pub fn digest_str(s: &str) -> String {
    blake3::hash(s.as_bytes()).to_hex().to_string()
}

fn find_non_finite(v: &Value) -> Option<String> {
    match v {
        Value::Number(n) => {
            if let Some(f) = n.as_f64()
                && !f.is_finite()
            {
                return Some(n.to_string());
            }
            None
        }
        Value::Array(a) => a.iter().find_map(find_non_finite),
        Value::Object(o) => o.values().find_map(find_non_finite),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_form_is_key_sorted_and_compact() {
        let v: Value = serde_json::from_str(r#"{"b":1,"a":{"z":2,"y":3}}"#).unwrap();
        assert_eq!(canonical_json(&v).unwrap(), r#"{"a":{"y":3,"z":2},"b":1}"#);
    }

    #[test]
    fn digest_is_stable_across_key_order() {
        let a: Value = serde_json::from_str(r#"{"x":1.5,"y":"s"}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"y":"s","x":1.5}"#).unwrap();
        assert_eq!(digest(&a).unwrap(), digest(&b).unwrap());
    }

    #[test]
    fn rejects_non_finite() {
        // Parsing path: serde_json refuses out-of-range numbers outright.
        assert!(serde_json::from_str::<Value>(r#"{"x":1e999}"#).is_err());
        // Programmatic path: Number::from_f64(non-finite) is None, and the
        // json! macro silently emits null — so the guard belongs at the
        // computation boundary (see dispatch tools), which is documented in
        // ADR-0002 §3 ("no NaN/Inf masking").
        assert!(serde_json::Number::from_f64(f64::INFINITY).is_none());
        assert!(serde_json::Number::from_f64(f64::NAN).is_none());
    }

    #[test]
    fn known_digest_value() {
        assert_eq!(digest(&json!({"k":1})).unwrap(), digest_str(r#"{"k":1}"#));
    }
}
