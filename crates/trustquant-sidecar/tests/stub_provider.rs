//! Provider-channel generality (M2 exit: a new provider plugs into the same
//! protocol without touching agent workflows). A stub provider speaking
//! protocol v1 — but registering a DIFFERENT tool — must flow through the
//! identical channel, with its structured refusals preserved.

use serde_json::json;
use std::time::Duration;
use trustquant_sidecar::SidecarHandle;

fn stub_script() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("tq-stub-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("stub_provider.py");
    std::fs::write(
        &path,
        r#"#!/usr/bin/env python3
import json, sys
print(json.dumps({"type":"hello","protocol":1,"provider":"stub-provider","version":"9.9.9",
                  "capabilities":["stub_tool"]}), flush=True)
for line in sys.stdin:
    msg = json.loads(line)
    if msg.get("type") != "request":
        continue
    if msg["tool"] == "stub_tool":
        out = {"type":"response","request_id":msg["request_id"],"status":"ok",
               "result":{"answer": msg["params"]["x"] * 2}}
    else:
        out = {"type":"response","request_id":msg["request_id"],"status":"error",
               "error":{"error_type":"unsupported_feature","message":"stub only serves stub_tool"}}
    print(json.dumps(out), flush=True)
"#,
    )
    .unwrap();
    path
}

#[test]
fn a_new_provider_flows_through_the_same_channel() {
    // system python3 is enough for the dependency-free stub; this test is a
    // separate binary so the env override cannot race other suites
    // SAFETY: single-threaded setup in a dedicated test binary
    unsafe { std::env::set_var("TRUSTQUANT_PROVIDER_PYTHON", "python3") };
    let script = stub_script();
    let mut sc = SidecarHandle::start(&script, Duration::from_secs(15)).expect("stub starts");
    // SAFETY: restore after the interpreter command was already resolved
    unsafe { std::env::remove_var("TRUSTQUANT_PROVIDER_PYTHON") };
    assert_eq!(sc.hello["provider"], "stub-provider");
    assert!(sc.capabilities().iter().any(|c| c == "stub_tool"));

    // its tool works through the unchanged protocol
    let r = sc
        .call("stub_tool", &json!({"x": 21}), Duration::from_secs(5))
        .expect("stub tool roundtrip");
    assert_eq!(r["answer"], 42);

    // its structured refusal keeps semantics (unsupported_feature, not a crash)
    let err = sc
        .call("optimize_portfolio", &json!({}), Duration::from_secs(5))
        .unwrap_err();
    let env = err.into_envelope();
    assert_eq!(env.error_type_str(), "unsupported_feature");
    assert!(!env.correctable);
}
