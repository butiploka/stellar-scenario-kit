//! JSON output contract tests.
//!
//! These tests pin the public shape of every command's JSON output so a
//! refactor cannot silently change a field name, a type, or a status
//! vocabulary. They run against the compiled binary and do not require the
//! Stellar CLI or a network, except where a live RPC read is the only way to
//! exercise a path — those tests skip cleanly when the CLI is absent.
//!
//! Contract under test:
//!   - stdout carries JSON only; text mode writes to stderr
//!   - `schema_version` is `"1.0"` wherever present
//!   - status vocabularies are closed sets
//!   - exit codes follow the documented mapping
//!   - no 56-char `S`-prefixed token appears in stdout or stderr

use assert_cmd::Command;
use std::path::PathBuf;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn bin() -> PathBuf {
    project_root().join("target/debug/stellar-scenario")
}

fn fixture(name: &str) -> String {
    project_root()
        .join("tests/scenarios")
        .join(name)
        .to_str()
        .unwrap()
        .to_string()
}

fn example(name: &str) -> String {
    project_root()
        .join("examples")
        .join(name)
        .to_str()
        .unwrap()
        .to_string()
}

fn run(args: &[&str]) -> (i32, String, String) {
    let mut cmd = Command::new(bin());
    cmd.args(args);
    let output = cmd.output().expect("run command");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn assert_no_secret(label: &str, stdout: &str, stderr: &str) {
    for text in [stdout, stderr] {
        for token in text.split(|c: char| !c.is_ascii_alphanumeric()) {
            assert!(
                !(token.len() == 56 && token.starts_with('S')),
                "{label} leaked a secret-shaped token: {token}"
            );
        }
    }
}

// ------------------------------------------------------------ validate

#[test]
fn validate_json_shape_on_success() {
    let (code, stdout, stderr) = run(&["validate", &example("payment.yml")]);
    assert_eq!(code, 0);
    assert!(stderr.is_empty(), "text must not go to stderr in json mode");

    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    assert_eq!(
        v.get("schema_version").and_then(|x| x.as_str()),
        Some("1.0")
    );
    assert_eq!(v.get("result").and_then(|x| x.as_str()), Some("valid"));
    assert!(v.get("scenario").and_then(|x| x.as_str()).is_some());
    assert!(v.get("steps").and_then(|x| x.as_u64()).is_some());
    assert!(v.get("errors").and_then(|x| x.as_array()).is_some());
    assert!(v.get("file").and_then(|x| x.as_str()).is_some());
    // `error` is omitted on success.
    assert!(v.get("error").is_none(), "error must be absent on success");
    assert_no_secret("validate", &stdout, &stderr);
}

#[test]
fn validate_json_shape_on_semantic_failure() {
    let (code, stdout, _) = run(&[
        "validate",
        &fixture("invalid_assert_missing_min_balance.yml"),
    ]);
    assert_eq!(code, 1);

    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    assert_eq!(v.get("result").and_then(|x| x.as_str()), Some("invalid"));
    assert!(v.get("error").and_then(|x| x.as_str()).is_some());
    let errors = v.get("errors").and_then(|x| x.as_array()).unwrap();
    assert!(!errors.is_empty());
    for e in errors {
        assert!(e.get("field").and_then(|x| x.as_str()).is_some());
        assert!(e.get("message").and_then(|x| x.as_str()).is_some());
    }
}

#[test]
fn validate_json_shape_on_io_failure() {
    let (code, stdout, _) = run(&["validate", "/tmp/stellar-scenario-absent.yml"]);
    assert_eq!(code, 2);

    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    assert_eq!(v.get("result").and_then(|x| x.as_str()), Some("invalid"));
    assert!(v.get("error").and_then(|x| x.as_str()).is_some());
}

// --------------------------------------------- preflight error accumulation

/// Multiple independent configuration errors are reported together in one run.
#[test]
fn validate_accumulates_multiple_independent_errors() {
    let (code, stdout, _) = run(&["validate", &fixture("invalid_multi_error.yml")]);
    assert_eq!(code, 1, "validation failure exits 1");

    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    assert_eq!(v.get("result").and_then(|x| x.as_str()), Some("invalid"));
    let errors = v.get("errors").and_then(|x| x.as_array()).unwrap();
    assert!(
        errors.len() >= 4,
        "expected >=4 independent errors, got {}: {errors:?}",
        errors.len()
    );
    // Each error names the exact step and field it belongs to.
    for e in errors {
        let field = e.get("field").and_then(|x| x.as_str()).expect("field");
        assert!(field.contains("steps[") || field == "network" || field == "source");
    }
}

/// A malformed document fails safely: one parse error, no misleading secondary
/// diagnostics.
#[test]
fn validate_malformed_document_has_single_synthetic_error() {
    let (code, stdout, _) = run(&["validate", &fixture("invalid_malformed_yaml.yml")]);
    assert_eq!(code, 2, "parse failure exits 2");

    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    assert_eq!(v.get("result").and_then(|x| x.as_str()), Some("invalid"));
    let errors = v.get("errors").and_then(|x| x.as_array()).unwrap();
    assert_eq!(
        errors.len(),
        1,
        "parse failure must report exactly one error, got {errors:?}"
    );
    assert_eq!(
        errors[0].get("field").and_then(|x| x.as_str()),
        Some("file")
    );
}

/// Diagnostics are deterministic: the same input always yields the same field
/// sequence, so CI diffs are stable.
#[test]
fn validate_diagnostics_are_deterministically_ordered() {
    let path = fixture("invalid_multi_error.yml");
    let mut runs: Vec<Vec<String>> = Vec::new();
    for _ in 0..3 {
        let (_, stdout, _) = run(&["validate", &path]);
        let v: serde_json::Value = stdout.parse().expect("valid JSON");
        runs.push(
            v.get("errors")
                .and_then(|x| x.as_array())
                .unwrap()
                .iter()
                .map(|e| e.get("field").and_then(|x| x.as_str()).unwrap().to_string())
                .collect(),
        );
    }
    assert_eq!(runs[0], runs[1]);
    assert_eq!(runs[1], runs[2]);
}

/// Structural errors (schema_version) coexist with per-step field errors.
#[test]
fn validate_reports_structural_and_step_errors_together() {
    let raw = r##"schema_version: "9.9"
name: mixed
network: testnet
steps:
  - action: assert_balance
    identity: alice
  - action: no_such_action
    identity: bob
  - action: assert_balance
    identity: bob
    min_balance: "abc"
"##;
    let dir = std::env::temp_dir().join("stellar-scenario-preflight");
    std::fs::create_dir_all(&dir).ok();
    let file = dir.join("mixed.yml");
    std::fs::write(&file, raw).unwrap();
    let (code, stdout, _) = run(&["validate", file.to_str().unwrap()]);
    assert_eq!(code, 1);

    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    let errors = v.get("errors").and_then(|x| x.as_array()).unwrap();
    let fields: Vec<&str> = errors
        .iter()
        .map(|e| e.get("field").and_then(|x| x.as_str()).unwrap())
        .collect();
    assert!(
        fields.contains(&"schema_version"),
        "missing schema_version: {fields:?}"
    );
    assert!(
        fields.iter().any(|f| f.starts_with("steps[0]")),
        "missing steps[0]: {fields:?}"
    );
    assert!(
        fields.iter().any(|f| f.starts_with("steps[1]")),
        "missing steps[1]: {fields:?}"
    );
    assert!(
        fields.iter().any(|f| f.starts_with("steps[2]")),
        "missing steps[2]: {fields:?}"
    );
}

/// Duplicate and empty labels are diagnosed with the offending step index.
#[test]
fn validate_labels_are_diagnosed() {
    let raw = r##"schema_version: "1.0"
name: labels
network: testnet
steps:
  - action: assert_balance
    identity: alice
    min_balance: "1"
    label: ""
  - action: assert_balance
    identity: bob
    min_balance: "1"
    label: dup
  - action: assert_balance
    identity: carol
    min_balance: "1"
    label: dup
"##;
    let dir = std::env::temp_dir().join("stellar-scenario-preflight");
    std::fs::create_dir_all(&dir).ok();
    let file = dir.join("labels.yml");
    std::fs::write(&file, raw).unwrap();
    let (code, stdout, _) = run(&["validate", file.to_str().unwrap()]);
    assert_eq!(code, 1);

    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    let errors = v.get("errors").and_then(|x| x.as_array()).unwrap();
    let messages: Vec<&str> = errors
        .iter()
        .map(|e| e.get("message").and_then(|x| x.as_str()).unwrap())
        .collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("label must not be empty")),
        "missing empty-label diagnostic: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("duplicate label")),
        "missing duplicate-label diagnostic: {messages:?}"
    );
    // The duplicate message points at the first occurrence.
    let dup = messages
        .iter()
        .find(|m| m.contains("duplicate label"))
        .unwrap();
    assert!(
        dup.contains("steps[1]"),
        "duplicate must cite first use: {dup}"
    );
}

/// Static validation never invokes the Stellar CLI: an empty PATH still validates.
#[test]
fn validate_static_needs_no_cli() {
    let mut cmd = Command::new(bin());
    cmd.args(["validate", &fixture("valid_payment.yml")]);
    cmd.env("PATH", "");
    let output = cmd.output().expect("run command");
    let code = output.status.code().unwrap_or(-1);
    assert_eq!(code, 0, "validation must not need the Stellar CLI");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    assert_eq!(v.get("result").and_then(|x| x.as_str()), Some("valid"));
}

/// Existing valid fixtures remain valid under the stricter preflight.
#[test]
fn validate_all_valid_fixtures_still_pass() {
    let dir = project_root().join("tests/scenarios");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).expect("read fixtures") {
        let path = entry.expect("entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.starts_with("valid_") || path.extension().and_then(|e| e.to_str()) != Some("yml") {
            continue;
        }
        let (code, stdout, _) = run(&["validate", path.to_str().unwrap()]);
        assert_eq!(code, 0, "fixture {name} must stay valid, stdout: {stdout}");
        let v: serde_json::Value = stdout.parse().expect("valid JSON");
        assert_eq!(
            v.get("errors").and_then(|x| x.as_array()).map(|a| a.len()),
            Some(0),
            "fixture {name} must report no errors"
        );
        checked += 1;
    }
    assert!(checked >= 15, "expected >=15 valid fixtures, got {checked}");
}

/// `plan`, `assert`, and `simulate` all reject the same invalid input with exit 1
/// before any execution begins.
#[test]
fn all_commands_share_preflight_validation() {
    let path = fixture("invalid_multi_error.yml");
    for cmd in ["plan", "assert", "simulate"] {
        let (code, _, _) = run(&[cmd, &path]);
        assert_eq!(code, 1, "{cmd} must fail preflight with exit 1");
    }
}

/// Validate JSON output is backward compatible: all documented fields present.
#[test]
fn validate_json_contract_backward_compatible() {
    let (code, stdout, _) = run(&["validate", &fixture("invalid_multi_error.yml")]);
    assert_eq!(code, 1);
    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    for key in ["schema_version", "result", "error", "errors", "file"] {
        assert!(v.get(key).is_some(), "missing required key {key}");
    }
    assert_eq!(
        v.get("schema_version").and_then(|x| x.as_str()),
        Some("1.0")
    );
    assert_eq!(v.get("result").and_then(|x| x.as_str()), Some("invalid"));
    let errors = v.get("errors").and_then(|x| x.as_array()).unwrap();
    for e in errors {
        assert!(e.get("field").and_then(|x| x.as_str()).is_some());
        assert!(e.get("message").and_then(|x| x.as_str()).is_some());
    }
}

// ------------------------------------------------ contract_id references

fn write_tmp(name: &str, raw: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("stellar-scenario-refs");
    std::fs::create_dir_all(&dir).ok();
    let file = dir.join(name);
    std::fs::write(&file, raw).unwrap();
    file
}

/// A defined C... contract reference passes preflight.
#[test]
fn valid_contract_strkey_passes() {
    let path = write_tmp(
        "valid_strkey.yml",
        r##"schema_version: "1.0"
name: strkey_ok
network: testnet
steps:
  - action: inspect_contract
    contract_id: CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC
"##,
    );
    let (code, stdout, _) = run(&["validate", path.to_str().unwrap()]);
    assert_eq!(code, 0, "valid C... StrKey must pass: {stdout}");
    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    assert_eq!(v.get("result").and_then(|x| x.as_str()), Some("valid"));
}

/// A named alias passes preflight (external identifiers are permitted).
#[test]
fn valid_contract_alias_passes() {
    let path = write_tmp(
        "valid_alias.yml",
        r##"schema_version: "1.0"
name: alias_ok
network: testnet
steps:
  - action: simulate_invoke
    contract_id: my-contract_1
    function: balance
"##,
    );
    let (code, stdout, _) = run(&["validate", path.to_str().unwrap()]);
    assert_eq!(code, 0, "valid alias must pass: {stdout}");
    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    assert_eq!(v.get("result").and_then(|x| x.as_str()), Some("valid"));
}

/// A malformed contract reference is diagnosed with the step and field.
#[test]
fn invalid_contract_reference_reported() {
    let path = write_tmp(
        "bad_contract.yml",
        r##"schema_version: "1.0"
name: bad_contract
network: testnet
steps:
  - action: simulate_invoke
    contract_id: "not a valid id"
    function: balance
"##,
    );
    let (code, stdout, _) = run(&["validate", path.to_str().unwrap()]);
    assert_eq!(code, 1);
    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    assert_eq!(v.get("result").and_then(|x| x.as_str()), Some("invalid"));
    let errors = v.get("errors").and_then(|x| x.as_array()).unwrap();
    assert_eq!(errors.len(), 1, "exactly one reference error: {errors:?}");
    assert_eq!(
        errors[0].get("field").and_then(|x| x.as_str()),
        Some("steps[0].contract_id")
    );
    let msg = errors[0].get("message").and_then(|x| x.as_str()).unwrap();
    assert!(
        msg.contains("StrKey") && msg.contains("alias"),
        "message must explain both accepted forms: {msg}"
    );
}

/// Multiple unresolved references are accumulated in one run.
#[test]
fn multiple_invalid_contract_references_accumulate() {
    let path = write_tmp(
        "multi_bad_contract.yml",
        r##"schema_version: "1.0"
name: multi_bad
network: testnet
steps:
  - action: inspect_contract
    contract_id: ""
  - action: simulate_invoke
    contract_id: "!!!bad!!!"
    function: decimals
  - action: inspect_contract
    contract_id: "has space"
"##,
    );
    let (code, stdout, _) = run(&["validate", path.to_str().unwrap()]);
    assert_eq!(code, 1);
    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    let errors = v.get("errors").and_then(|x| x.as_array()).unwrap();
    assert_eq!(
        errors.len(),
        3,
        "three independent contract_id errors: {errors:?}"
    );
    // Each names its own step index.
    let fields: Vec<&str> = errors
        .iter()
        .map(|e| e.get("field").and_then(|x| x.as_str()).unwrap())
        .collect();
    assert_eq!(
        fields,
        vec![
            "steps[0].contract_id",
            "steps[1].contract_id",
            "steps[2].contract_id"
        ]
    );
}

/// A C... StrKey with a wrong length is rejected (prefix alone is not enough).
#[test]
fn short_contract_strkey_rejected() {
    let path = write_tmp(
        "short_strkey.yml",
        r##"schema_version: "1.0"
name: short
network: testnet
steps:
  - action: inspect_contract
    contract_id: C!short
"##,
    );
    let (code, stdout, _) = run(&["validate", path.to_str().unwrap()]);
    assert_eq!(code, 1);
    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    let errors = v.get("errors").and_then(|x| x.as_array()).unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0].get("field").and_then(|x| x.as_str()),
        Some("steps[0].contract_id")
    );
}

/// Reference validation runs without the Stellar CLI and needs no network.
#[test]
fn contract_reference_validation_is_static() {
    let path = write_tmp(
        "static_ref.yml",
        r##"schema_version: "1.0"
name: static
network: testnet
steps:
  - action: simulate_invoke
    contract_id: "bad value"
    function: balance
"##,
    );
    let mut cmd = Command::new(bin());
    cmd.args(["validate", path.to_str().unwrap()]);
    cmd.env("PATH", "");
    let output = cmd.output().expect("run command");
    assert_eq!(output.status.code().unwrap_or(-1), 1);
}

/// plan, assert, and simulate share the same reference validation.
#[test]
fn all_commands_share_contract_reference_validation() {
    let path = write_tmp(
        "shared_ref.yml",
        r##"schema_version: "1.0"
name: shared
network: testnet
steps:
  - action: simulate_invoke
    contract_id: "bad value"
    function: balance
"##,
    );
    for cmd in ["plan", "assert", "simulate"] {
        let (code, _, _) = run(&[cmd, path.to_str().unwrap()]);
        assert_eq!(code, 1, "{cmd} must reject bad contract_id with exit 1");
    }
}

/// A contract_id with a leading digit is rejected (aliases must start with a
/// letter), so the diagnostic is unambiguous.
#[test]
fn contract_reference_leading_digit_rejected() {
    let path = write_tmp(
        "digit_ref.yml",
        r##"schema_version: "1.0"
name: digit
network: testnet
steps:
  - action: inspect_contract
    contract_id: 123abc
"##,
    );
    let (code, stdout, _) = run(&["validate", path.to_str().unwrap()]);
    assert_eq!(code, 1);
    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    let errors = v.get("errors").and_then(|x| x.as_array()).unwrap();
    assert!(errors
        .iter()
        .any(|e| { e.get("field").and_then(|x| x.as_str()) == Some("steps[0].contract_id") }));
}

#[test]
fn validate_text_mode_writes_only_to_stderr() {
    let (code, stdout, stderr) = run(&["validate", &example("payment.yml"), "--output", "text"]);
    assert_eq!(code, 0);
    assert!(stdout.is_empty(), "json/text must not mix on stdout");
    assert!(!stderr.is_empty(), "text output must go to stderr");
}

// ---------------------------------------------------------------- plan

#[test]
fn plan_json_shape() {
    let (code, stdout, stderr) = run(&["plan", &example("payment.yml")]);
    assert_eq!(code, 0);
    assert!(stderr.is_empty());

    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    assert!(v.get("scenario").and_then(|x| x.as_str()).is_some());
    assert!(v.get("network").and_then(|x| x.as_str()).is_some());
    assert_eq!(v.get("read_only").and_then(|x| x.as_bool()), Some(false));

    let commands = v.get("commands").and_then(|x| x.as_array()).unwrap();
    assert_eq!(commands.len(), 3);
    for c in commands {
        assert!(c.get("step_index").and_then(|x| x.as_u64()).is_some());
        assert!(c.get("action").and_then(|x| x.as_str()).is_some());
        assert!(c.get("executable").and_then(|x| x.as_str()).is_some());
        assert!(c.get("args").and_then(|x| x.as_array()).is_some());
        let side = c.get("side_effect").and_then(|x| x.as_str()).unwrap();
        assert!(
            side == "read_only" || side == "mutates_remote_state",
            "unexpected side_effect: {side}"
        );
    }
    assert_no_secret("plan", &stdout, &stderr);
}

#[test]
fn assert_json_shape() {
    let (code, stdout, stderr) = run(&["assert", &fixture("valid_assert_balance.yml")]);
    // Exit code depends on network availability; the shape does not.
    assert!(matches!(code, 0..=2), "unexpected exit code {code}");

    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    if v.get("error").is_some() {
        // No Stellar CLI on this host: the command reports a structured
        // error object. That is a valid shape too.
        assert_eq!(code, 2);
        assert_no_secret("assert", &stdout, &stderr);
        return;
    }

    assert_eq!(
        v.get("schema_version").and_then(|x| x.as_str()),
        Some("1.0")
    );
    assert!(v.get("scenario").and_then(|x| x.as_str()).is_some());
    assert!(v.get("network").and_then(|x| x.as_str()).is_some());
    assert!(v.get("total_steps").and_then(|x| x.as_u64()).is_some());
    for key in [
        "passed",
        "failed",
        "errors",
        "skipped_mutating",
        "skipped_not_executable",
    ] {
        assert!(
            v.get(key).and_then(|x| x.as_u64()).is_some(),
            "missing {key}"
        );
    }
    let status = v.get("status").and_then(|x| x.as_str()).unwrap();
    assert!(
        matches!(status, "passed" | "failed" | "errored" | "no_assertions"),
        "unexpected status: {status}"
    );
    assert!(v.get("results").and_then(|x| x.as_array()).is_some());
    assert!(v.get("skipped").and_then(|x| x.as_array()).is_some());
    assert_no_secret("assert", &stdout, &stderr);
}

#[test]
fn assert_result_status_vocabulary_is_closed() {
    let (_, stdout, _) = run(&["assert", &fixture("valid_assert_balance.yml")]);
    let v: serde_json::Value = stdout.parse().unwrap();
    if v.get("error").is_some() {
        return; // no CLI on this host
    }
    for r in v.get("results").and_then(|x| x.as_array()).unwrap() {
        let status = r.get("status").and_then(|x| x.as_str()).unwrap();
        assert!(
            matches!(status, "pass" | "fail" | "error"),
            "unexpected result status: {status}"
        );
        let mode = r.get("mode").and_then(|x| x.as_str()).unwrap();
        assert!(
            matches!(mode, "min_balance" | "exists"),
            "unexpected mode: {mode}"
        );
    }
    for s in v.get("skipped").and_then(|x| x.as_array()).unwrap() {
        let reason = s.get("reason").and_then(|x| x.as_str()).unwrap();
        assert!(
            matches!(reason, "would_mutate" | "not_executable"),
            "unexpected skip reason: {reason}"
        );
    }
}

#[test]
fn assert_text_mode_writes_only_to_stderr() {
    let (_, stdout, stderr) = run(&[
        "assert",
        &fixture("valid_assert_balance.yml"),
        "--output",
        "text",
    ]);
    assert!(stdout.is_empty(), "text must not go to stdout");
    assert!(!stderr.is_empty(), "text output must go to stderr");
}

// ------------------------------------------------------------- simulate

#[test]
fn simulate_json_shape() {
    let (code, stdout, stderr) = run(&["simulate", &fixture("valid_simulate_invoke.yml")]);
    assert!(matches!(code, 0..=2), "unexpected exit code {code}");

    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    if v.get("error").is_some() {
        assert_eq!(code, 2);
        assert_no_secret("simulate", &stdout, &stderr);
        return;
    }

    assert_eq!(
        v.get("schema_version").and_then(|x| x.as_str()),
        Some("1.0")
    );
    assert_eq!(v.get("submitted").and_then(|x| x.as_bool()), Some(false));
    let status = v.get("status").and_then(|x| x.as_str()).unwrap();
    assert!(
        matches!(
            status,
            "simulated" | "unsupported_operation" | "failed" | "no_simulations"
        ),
        "unexpected status: {status}"
    );
    for key in [
        "total_steps",
        "simulated",
        "unsupported",
        "failed",
        "skipped",
    ] {
        assert!(
            v.get(key).and_then(|x| x.as_u64()).is_some(),
            "missing {key}"
        );
    }

    for r in v.get("results").and_then(|x| x.as_array()).unwrap() {
        assert_eq!(r.get("submitted").and_then(|x| x.as_bool()), Some(false));
        assert_eq!(r.get("simulated").and_then(|x| x.as_bool()), Some(true));
        let rstatus = r.get("status").and_then(|x| x.as_str()).unwrap();
        assert!(
            matches!(
                rstatus,
                "simulated"
                    | "unsupported_operation"
                    | "build_failed"
                    | "simulate_failed"
                    | "decode_failed"
                    | "error"
            ),
            "unexpected result status: {rstatus}"
        );
    }
    assert_no_secret("simulate", &stdout, &stderr);
}

#[test]
fn simulate_text_mode_writes_only_to_stderr() {
    let (_, stdout, stderr) = run(&[
        "simulate",
        &fixture("valid_simulate_invoke.yml"),
        "--output",
        "text",
    ]);
    assert!(stdout.is_empty(), "text must not go to stdout");
    assert!(!stderr.is_empty(), "text output must go to stderr");
}

#[test]
fn simulate_text_output_says_not_submitted() {
    let (_, _, stderr) = run(&[
        "simulate",
        &fixture("valid_simulate_invoke.yml"),
        "--output",
        "text",
    ]);
    if stderr.contains("stellar CLI unavailable") {
        return; // no CLI on this host
    }
    assert!(
        stderr.contains("not submitted"),
        "text must say not submitted: {stderr}"
    );
}

// ---------------------------------------------------------------- doctor

#[test]
fn doctor_json_shape() {
    let (code, stdout, stderr) = run(&["doctor"]);
    assert!(matches!(code, 0 | 1), "unexpected exit code {code}");
    assert!(stderr.is_empty());

    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    let checks = v.as_array().expect("array of checks");
    assert!(!checks.is_empty());
    for c in checks {
        assert!(c.get("name").and_then(|x| x.as_str()).is_some());
        assert!(c.get("ok").and_then(|x| x.as_bool()).is_some());
        assert!(c.get("detail").and_then(|x| x.as_str()).is_some());
    }
    assert_no_secret("doctor", &stdout, &stderr);
}

// ------------------------------------------------------- cross-command

#[test]
fn every_json_mode_leaves_stderr_empty_on_success() {
    for args in [
        vec!["validate", &example("payment.yml")],
        vec!["plan", &example("payment.yml")],
        vec!["doctor"],
    ] {
        let (code, _, stderr) = run(&args);
        if code == 0 {
            assert!(
                stderr.is_empty(),
                "{args:?} wrote to stderr in json mode: {stderr}"
            );
        }
    }
}

#[test]
fn stdout_is_always_parseable_json_in_json_mode() {
    // A parse failure here means a command mixed text into stdout.
    for args in [
        vec!["validate", &example("payment.yml")],
        vec![
            "validate",
            &fixture("invalid_assert_missing_min_balance.yml"),
        ],
        vec!["validate", "/tmp/stellar-scenario-absent.yml"],
        vec!["plan", &example("payment.yml")],
        vec!["doctor"],
        vec!["assert", &fixture("valid_assert_balance.yml")],
        vec!["simulate", &fixture("valid_simulate_invoke.yml")],
    ] {
        let (_, stdout, _) = run(&args);
        assert!(!stdout.trim().is_empty(), "{args:?} produced no stdout");
        serde_json::from_str::<serde_json::Value>(&stdout)
            .unwrap_or_else(|e| panic!("{args:?} stdout is not JSON: {e}\n{stdout}"));
    }
}

// ------------------------------------------------------ step_counts contracts

/// Assert JSON includes nested step_counts with all required fields.
#[test]
fn assert_json_includes_step_counts() {
    let (_code, stdout, stderr) = run(&["assert", &fixture("valid_assert_balance.yml")]);
    if stdout.contains("\"error\":") || stdout.contains("\"result\":\"invalid\"") {
        return; // CLI unavailable or scenario invalid; skip live check
    }
    let v: serde_json::Value = stdout.parse().expect("valid JSON");

    // Flat counts must be present for backward compatibility.
    for key in [
        "passed",
        "failed",
        "errors",
        "skipped_mutating",
        "skipped_not_executable",
        "total_steps",
        "executed",
        "status",
    ] {
        assert!(
            v.get(key).is_some(),
            "assert JSON missing required flat field '{key}'"
        );
    }

    // Nested step_counts must be present (new additive field).
    let sc = v.get("step_counts").expect("step_counts missing");
    for key in ["executed", "passed", "failed", "errors", "skipped", "total"] {
        assert!(
            sc.get(key).is_some(),
            "step_counts missing required field '{key}'"
        );
    }
    // Reconcile: executed + skipped = total.
    let executed = sc.get("executed").and_then(|x| x.as_u64()).unwrap();
    let skipped = sc.get("skipped").and_then(|x| x.as_u64()).unwrap();
    let total = sc.get("total").and_then(|x| x.as_u64()).unwrap();
    assert_eq!(
        executed + skipped,
        total,
        "step_counts reconciliation failed"
    );
    assert_no_secret("assert_step_counts", &stdout, &stderr);
}

/// Simulate JSON includes nested step_counts with correct semantics.
#[test]
fn simulate_json_includes_step_counts() {
    let (_code, stdout, stderr) = run(&["simulate", &fixture("valid_simulate_invoke.yml")]);
    if stdout.contains("\"error\":") || stdout.contains("\"result\":\"invalid\"") {
        return; // CLI unavailable or scenario invalid; skip live check
    }
    let v: serde_json::Value = stdout.parse().expect("valid JSON");

    // Flat counts must be present.
    for key in [
        "total_steps",
        "simulated",
        "unsupported",
        "failed",
        "skipped",
        "submitted",
    ] {
        assert!(
            v.get(key).is_some(),
            "simulate JSON missing required flat field '{key}'"
        );
    }
    assert_eq!(v.get("submitted").and_then(|x| x.as_bool()), Some(false));

    // Nested step_counts must be present.
    let sc = v.get("step_counts").expect("step_counts missing");
    for key in ["simulated", "unsupported", "failed", "skipped", "total"] {
        assert!(
            sc.get(key).is_some(),
            "step_counts missing required field '{key}'"
        );
    }
    assert_eq!(v.get("submitted").and_then(|x| x.as_bool()), Some(false));
    assert_no_secret("simulate_step_counts", &stdout, &stderr);
}

/// Plan JSON includes nested step_counts with executable/would_mutate/total.
#[test]
fn plan_json_includes_step_counts() {
    let (code, stdout, stderr) = run(&["plan", &example("payment.yml")]);
    assert_eq!(code, 0);

    let v: serde_json::Value = stdout.parse().expect("valid JSON");

    // step_counts with plan-specific semantics.
    let sc = v.get("step_counts").expect("step_counts missing");
    for key in ["executable", "would_mutate", "total"] {
        assert!(
            sc.get(key).is_some(),
            "plan step_counts missing required field '{key}'"
        );
    }
    // Reconciliation: executable + would_mutate = total.
    let executable = sc.get("executable").and_then(|x| x.as_u64()).unwrap();
    let would_mutate = sc.get("would_mutate").and_then(|x| x.as_u64()).unwrap();
    let total = sc.get("total").and_then(|x| x.as_u64()).unwrap();
    assert_eq!(
        executable + would_mutate,
        total,
        "plan step_counts reconciliation failed"
    );
    assert_no_secret("plan", &stdout, &stderr);
}

// -------------------------------------------------------- JSONL contract tests

/// Assert JSONL: each stdout line parses, has type discriminator, order matches.
#[test]
fn jsonl_contract_assert() {
    let (code, stdout, stderr) =
        run(&["assert", &fixture("valid_payment.yml"), "--output", "jsonl"]);
    assert!(matches!(code, 0..=2), "unexpected exit code {code}");

    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    // payment.yml has 3 steps (ensure_funded, payment, assert_balance).
    // ensure_funded + payment are skipped (would_mutate), assert_balance is a result.
    let events: Vec<serde_json::Value> = lines
        .iter()
        .map(|l| serde_json::from_str(l).expect("stdout line is valid JSONL"))
        .collect();

    let mut prev_step: usize = 0;
    for ev in &events {
        let event_type = ev
            .get("type")
            .and_then(|t| t.as_str())
            .expect("event has type");
        assert!(
            matches!(event_type, "result" | "skipped"),
            "event has unexpected type: {ev}"
        );
        let step_index = ev.get("step_index").and_then(|s| s.as_u64()).unwrap();
        let idx = step_index as usize;
        assert!(
            idx >= prev_step,
            "events not in step order: {prev_step} -> {idx}"
        );
        prev_step = idx;
    }

    // Stderr is one compact JSON aggregate.
    let aggregate: serde_json::Value = serde_json::from_str(&stderr).expect("stderr is valid JSON");
    assert!(
        aggregate.get("step_counts").is_some(),
        "aggregate missing step_counts"
    );
    // Event stream covers both executed and skipped steps.
    let n_results = aggregate
        .get("results")
        .and_then(|r| r.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let n_skipped = aggregate
        .get("skipped")
        .and_then(|s| s.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    assert_eq!(
        n_results + n_skipped,
        events.len(),
        "event stream must cover results + skipped"
    );
    assert_no_secret("assert_jsonl", &stdout, &stderr);
}

/// Simulate JSONL: each stdout line parses, has type discriminator, stderr aggregate.
#[test]
fn jsonl_contract_simulate() {
    let (code, stdout, stderr) = run(&[
        "simulate",
        &fixture("valid_simulate_invoke.yml"),
        "--output",
        "jsonl",
    ]);
    assert!(matches!(code, 0..=2), "unexpected exit code {code}");

    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    let events: Vec<serde_json::Value> = lines
        .iter()
        .map(|l| serde_json::from_str(l).expect("stdout line is valid JSONL"))
        .collect();

    for ev in &events {
        assert_eq!(
            ev.get("type").and_then(|t| t.as_str()),
            Some("result"),
            "event missing type=result: {ev}"
        );
        assert_eq!(
            ev.get("simulated").and_then(|b| b.as_bool()),
            Some(true),
            "event missing simulated=true: {ev}"
        );
        assert_eq!(
            ev.get("submitted").and_then(|b| b.as_bool()),
            Some(false),
            "event missing submitted=false: {ev}"
        );
    }

    let aggregate: serde_json::Value = serde_json::from_str(&stderr).expect("stderr is valid JSON");
    assert!(
        aggregate.get("step_counts").is_some(),
        "aggregate missing step_counts"
    );
    assert_eq!(
        aggregate.get("submitted").and_then(|b| b.as_bool()),
        Some(false)
    );
    assert_no_secret("simulate_jsonl", &stdout, &stderr);
}

/// Invalid input: stdout empty, stderr contains error JSON.
#[test]
fn jsonl_invalid_input_leaves_stdout_empty() {
    let (code, stdout, stderr) = run(&[
        "assert",
        &fixture("invalid_simulate_missing_contract.yml"),
        "--output",
        "jsonl",
    ]);
    assert_eq!(code, 1);
    assert!(
        stdout.trim().is_empty(),
        "stdout must be empty for invalid scenario"
    );
    let err: serde_json::Value = serde_json::from_str(&stderr).expect("stderr is valid JSON");
    assert_eq!(err.get("result").and_then(|r| r.as_str()), Some("invalid"));
}

// --------------------------------------------------- cross-command invariants

/// All JSON-mode commands produce stdout as single valid JSON (no prose mixing).
#[test]
fn all_json_commands_stdout_is_parseable() {
    let payment = example("payment.yml");
    let assert_fixture = fixture("valid_assert_balance.yml");
    let sim_fixture = fixture("valid_simulate_invoke.yml");

    for (name, args) in [
        ("validate", vec!["validate", &payment[..]]),
        ("plan", vec!["plan", &payment[..]]),
        ("assert", vec!["assert", &assert_fixture[..]]),
        ("simulate", vec!["simulate", &sim_fixture[..]]),
    ] {
        let (code, stdout, stderr) = run(&args);
        if stdout.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = stdout
            .parse()
            .unwrap_or_else(|e| panic!("{name} stdout must be valid JSON: {e}"));
        if let Some(sv) = v.get("schema_version").and_then(|s| s.as_str()) {
            assert_eq!(sv, "1.0", "{name} schema_version mismatch");
        }
        if v.get("error").is_some() {
            assert_eq!(code, 2, "{name} error reported but exit code != 2");
        }
        assert_no_secret(name, &stdout, &stderr);
    }
}

/// Text mode always writes to stderr, never stdout.
#[test]
fn all_text_mode_goes_to_stderr() {
    let payment = example("payment.yml");
    let assert_fixture = fixture("valid_assert_balance.yml");
    let sim_fixture = fixture("valid_simulate_invoke.yml");

    for (name, args) in [
        (
            "validate",
            vec!["validate", &payment[..], "--output", "text"],
        ),
        ("plan", vec!["plan", &payment[..], "--output", "text"]),
        (
            "assert",
            vec!["assert", &assert_fixture[..], "--output", "text"],
        ),
        (
            "simulate",
            vec!["simulate", &sim_fixture[..], "--output", "text"],
        ),
    ] {
        let (_code, stdout, stderr) = run(&args);
        assert!(
            stdout.trim().is_empty(),
            "{name} --output text must not write to stdout"
        );
        assert!(
            !stderr.trim().is_empty(),
            "{name} --output text must write to stderr"
        );
    }
}
