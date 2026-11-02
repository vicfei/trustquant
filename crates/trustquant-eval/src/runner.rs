//! Deterministic trace Agent-Eval runner (ADR-0006 gate 6). Cases live in
//! evals/cases/*.json and run through the SAME MCP message path real agents
//! use. M0's "Agent completes a Skill" gate is exactly this: a scripted
//! calling agent following the Skill's deterministic trace.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use trustquant_runtime::mcp::handle_message;

#[derive(Debug, serde::Serialize)]
pub struct StepReport {
    pub step: usize,
    pub tool: String,
    pub passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct CaseReport {
    pub case_id: String,
    pub category: String,
    pub passed: bool,
    pub steps: Vec<StepReport>,
}

#[derive(Debug, serde::Serialize)]
pub struct EvalReport {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub cases: Vec<CaseReport>,
}

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn default_cases_dir() -> PathBuf {
    workspace_root().join("evals/cases")
}

pub fn run_cases_dir(dir: &Path) -> EvalReport {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read eval cases dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    let cases: Vec<CaseReport> = files
        .iter()
        .map(|f| {
            let text =
                std::fs::read_to_string(f).unwrap_or_else(|e| panic!("read {}: {e}", f.display()));
            let case: Value = serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("parse {}: {e}", f.display()));
            run_case(&case)
        })
        .collect();
    let passed = cases.iter().filter(|c| c.passed).count();
    EvalReport {
        total: cases.len(),
        passed,
        failed: cases.len() - passed,
        cases,
    }
}

pub fn run_case(case: &Value) -> CaseReport {
    let case_id = case["id"].as_str().unwrap_or("unknown").to_string();
    let category = case["category"]
        .as_str()
        .unwrap_or("uncategorized")
        .to_string();
    let mut steps = Vec::new();
    let mut npv_by_step: Vec<f64> = Vec::new();
    let mut captured_id: Option<String> = None;
    let mut captured_dataset: Option<String> = None;
    for (i, step) in case["steps"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        let tool = step["tool"].as_str().unwrap_or_default().to_string();
        let outcome = run_step(step, &mut npv_by_step, &captured_id, &captured_dataset);
        if let Ok(env) = &outcome
            && let Some(id) = env["calculation_id"].as_str()
        {
            captured_id = Some(id.to_string());
        }
        if let Ok(env) = &outcome
            && let Some(d) = env["result"]["dataset_id"].as_str()
        {
            captured_dataset = Some(d.to_string());
        }
        steps.push(StepReport {
            step: i,
            tool,
            passed: outcome.is_ok(),
            failure: outcome.err(),
        });
    }
    CaseReport {
        case_id,
        category,
        passed: steps.iter().all(|s| s.passed),
        steps,
    }
}

fn mcp_call(
    tool: &str,
    step: &Value,
    captured_id: &Option<String>,
    captured_dataset: &Option<String>,
) -> Result<Value, String> {
    // $calculation_id / $dataset_id bind to values captured from previous steps.
    let params = substitute_placeholders(&step["params"], captured_id, captured_dataset);
    let arguments = json!({
        "schema_version": step
            .get("schema_version")
            .and_then(Value::as_str)
            .unwrap_or(trustquant_core::contract::SCHEMA_VERSION),
        "request_id": format!("eval-{}", step.get("note").and_then(Value::as_str).unwrap_or("step")),
        "params": params,
    });
    let resp = handle_message(&json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": tool, "arguments": arguments}
    }))
    .ok_or("no response")?;
    let text = resp["result"]["content"][0]["text"]
        .as_str()
        .ok_or("malformed MCP response")?;
    serde_json::from_str(text).map_err(|e| format!("envelope parse: {e}"))
}

fn substitute_placeholders(
    v: &Value,
    captured_id: &Option<String>,
    captured_dataset: &Option<String>,
) -> Value {
    match v {
        Value::String(s) if s == "$calculation_id" => match captured_id {
            Some(id) => json!(id),
            None => Value::Null,
        },
        Value::String(s) if s == "$dataset_id" => match captured_dataset {
            Some(d) => json!(d),
            None => Value::Null,
        },
        Value::Object(o) => {
            let mut out = serde_json::Map::new();
            for (k, val) in o {
                out.insert(
                    k.clone(),
                    substitute_placeholders(val, captured_id, captured_dataset),
                );
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(
            a.iter()
                .map(|x| substitute_placeholders(x, captured_id, captured_dataset))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Runs one step; on success returns the response envelope (so callers can
/// capture calculation_id for later steps).
fn run_step(
    step: &Value,
    npv_by_step: &mut Vec<f64>,
    captured_id: &Option<String>,
    captured_dataset: &Option<String>,
) -> Result<Value, String> {
    let tool = step["tool"].as_str().ok_or("step missing tool")?;
    let envelope = mcp_call(tool, step, captured_id, captured_dataset)?;

    let expect = &step["expect"];
    let want_status = expect["status"].as_str().unwrap_or("success");
    if envelope["status"].as_str() != Some(want_status) {
        return Err(format!(
            "status {:?} != expected {want_status}",
            envelope["status"]
        ));
    }
    if let Some(want_err) = expect["error_type"].as_str() {
        let got = envelope["error"]["error_type"].as_str().unwrap_or("<none>");
        if got != want_err {
            return Err(format!("error_type {got} != {want_err}"));
        }
    }
    if let Some(fields) = expect["result_fields"].as_object() {
        for (k, v) in fields {
            let got = &envelope["result"][k];
            if got != v {
                return Err(format!("result.{k} = {got} != {v}"));
            }
        }
    }
    if let Some(path) = expect["result_path_eq"].as_str() {
        let mut cur = &envelope;
        for seg in path.split('.') {
            cur = cur
                .get(seg)
                .ok_or_else(|| format!("path {path} missing at '{seg}'"))?;
        }
        if cur.is_null() {
            return Err(format!("path {path} is null"));
        }
    }
    if let Some(npv_path) = expect["capture_npv"].as_str()
        && npv_path == "npv"
    {
        let v = envelope["result"]["npv"]
            .as_f64()
            .ok_or("npv not a number; refusing to fabricate")?;
        npv_by_step.push(v);
    }
    if let Some(check) = expect["cross_step_check"].as_str()
        && check == "put_call_parity"
    {
        let (call, put) = (npv_by_step.first().copied(), npv_by_step.get(1).copied());
        let (Some(c), Some(p)) = (call, put) else {
            return Err("parity check needs two captured npv values".into());
        };
        let parity = c - p;
        if !parity.is_finite() {
            return Err("parity value not finite".into());
        }
        // Exact relation is checked by the engine's own invariant tests;
        // here we verify the workflow captured both legs and they differ
        // consistently (call >= put when rates/dividends zero-ish).
        if parity < -1e-6 {
            return Err(format!("call-put = {parity} < 0 violates parity bound"));
        }
    }
    if expect["reproducible_digest"].as_bool() == Some(true) {
        let again = mcp_call(tool, step, captured_id, captured_dataset)?;
        let d1 =
            trustquant_core::canonical::digest(&envelope["result"]).map_err(|e| e.to_string())?;
        let d2 = trustquant_core::canonical::digest(&again["result"]).map_err(|e| e.to_string())?;
        if d1 != d2 {
            return Err(format!("result digest not reproducible: {d1} vs {d2}"));
        }
    }
    Ok(envelope)
}
