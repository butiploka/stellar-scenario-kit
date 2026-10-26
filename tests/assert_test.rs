//! Integration tests for the `assert` command.
//!
//! These tests never submit a transaction and never fund an account. The only
//! subprocess spawned is `stellar token balance`, which is a read-only RPC
//! call. Tests that need a live network are gated on the Stellar CLI being
//! present and on a configured network being reachable; the rest run offline
//! against fixtures.

use assert_cmd::Command;
use predicates::prelude::*;
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

fn assert_cmd(path: &str) -> Command {
    let mut cmd = Command::new(bin());
    cmd.arg("assert").arg(path);
    cmd
}

// ------------------------------------------------------- offline behaviour

#[test]
fn assert_rejects_invalid_scenario() {
    assert_cmd(&fixture("invalid_assert_missing_min_balance.yml"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("min_balance"));
}

#[test]
fn assert_rejects_min_balance_above_u128() {
    assert_cmd(&fixture("invalid_assert_balance_over_u128.yml"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("u128"));
}

#[test]
fn assert_accepts_min_balance_at_u128_max() {
    // Validation must accept u128::MAX exactly; only u128::MAX + 1 is out of
    // range. Use `validate` so the check does not depend on a live network.
    let mut cmd = Command::new(bin());
    cmd.arg("validate")
        .arg(fixture("valid_assert_balance_u128_max.yml"));
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("\"result\": \"valid\""));
}

#[test]
fn assert_rejects_zero_timeout() {
    assert_cmd(&fixture("invalid_timeout_zero.yml"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("timeout_seconds"));
}

#[test]
fn assert_rejects_excessive_timeout() {
    assert_cmd(&fixture("invalid_timeout_too_large.yml"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("timeout_seconds"));
}

#[test]
fn assert_accepts_boundary_timeouts() {
    for name in ["valid_timeout_min.yml", "valid_timeout_max.yml"] {
        assert_cmd(&fixture(name))
            .assert()
            .failure()
            .stdout(predicate::str::contains("timeout_seconds").not());
    }
}

#[test]
fn assert_emits_structured_report() {
    if !stellar_available() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let output = assert_cmd(&fixture("valid_assert_balance.yml"))
        .output()
        .expect("run assert");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().expect("assert stdout is valid JSON");

    assert_eq!(
        parsed.get("scenario").and_then(|v| v.as_str()),
        Some("assert_preview_balance")
    );
    assert!(parsed.get("results").is_some(), "missing results array");
    assert!(parsed.get("skipped").is_some(), "missing skipped array");
    assert!(parsed.get("status").is_some(), "missing status");
    assert_eq!(
        parsed.get("schema_version").and_then(|v| v.as_str()),
        Some("1.0")
    );
}

#[test]
fn assert_skips_mutating_steps() {
    if !stellar_available() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let payment = project_root().join("examples/payment.yml");
    let output = assert_cmd(payment.to_str().unwrap())
        .output()
        .expect("run assert");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().unwrap();
    let skipped = parsed
        .get("skipped")
        .and_then(|v| v.as_array())
        .expect("skipped array");

    assert_eq!(skipped.len(), 2, "skipped: {skipped:?}");
    let actions: Vec<_> = skipped
        .iter()
        .filter_map(|s| s.get("action").and_then(|v| v.as_str()))
        .collect();
    assert!(actions.contains(&"ensure_funded"));
    assert!(actions.contains(&"payment"));

    let results = parsed
        .get("results")
        .and_then(|v| v.as_array())
        .expect("results array");
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].get("step_index").and_then(|v| v.as_u64()),
        Some(2)
    );

    // Each skipped (mutation-blocked) step carries an explicit
    // execution_status and a block_reason, distinct from executed results.
    for s in skipped {
        assert!(
            s.get("execution_status").is_some(),
            "skipped step lacks execution_status: {s:?}"
        );
    }
    let mutating = skipped
        .iter()
        .find(|s| s.get("action").and_then(|v| v.as_str()) == Some("ensure_funded"));
    assert!(mutating.is_some(), "missing ensure_funded skip");
    let m = mutating.unwrap();
    assert_eq!(
        m.get("execution_status").and_then(|v| v.as_str()),
        Some("would_mutate")
    );
    assert!(m
        .get("block_reason")
        .and_then(|v| v.as_str())
        .unwrap()
        .contains("would mutate ledger state"));

    // Executed results carry execution_status = executable, no block_reason.
    for r in results {
        assert_eq!(
            r.get("execution_status").and_then(|v| v.as_str()),
            Some("executable"),
            "executed result should be executable: {r:?}"
        );
        assert!(
            r.get("block_reason").is_none(),
            "executed result should have no block_reason: {r:?}"
        );
    }
}

#[test]
fn assert_never_invokes_funding_or_payment() {
    if !stellar_available() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let raw = r#"
schema_version: "1.0"
name: mutating_only
network: testnet
steps:
  - action: ensure_funded
    identity: deployer
  - action: payment
    from: deployer
    to: preview
    amount: "1"
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("mutating.yml");
    std::fs::write(&path, raw).expect("write scenario");

    let output = assert_cmd(path.to_str().unwrap())
        .output()
        .expect("run assert");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().unwrap();

    let results = parsed
        .get("results")
        .and_then(|v| v.as_array())
        .expect("results array");
    assert!(
        results.is_empty(),
        "no step should be executed, got {results:?}"
    );

    let skipped = parsed
        .get("skipped")
        .and_then(|v| v.as_array())
        .expect("skipped array");
    assert_eq!(skipped.len(), 2);
}

#[test]
fn assert_result_never_contains_secret_material() {
    let output = assert_cmd(&fixture("valid_assert_balance.yml"))
        .output()
        .expect("run assert");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    for token in combined.split(|c: char| !c.is_ascii_alphanumeric()) {
        assert!(
            !(token.len() == 56 && token.starts_with('S')),
            "assert output leaked a secret-looking token: {token}"
        );
    }
}

#[test]
fn assert_text_output_goes_to_stderr() {
    if !stellar_available() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    assert_cmd(&fixture("valid_assert_balance.yml"))
        .arg("--output")
        .arg("text")
        .assert()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::is_empty().not());
}

// -------------------------------------------------- live-network behaviour
//
// Gated on the Stellar CLI being installed. These run read-only balance
// queries against testnet and never mutate state.

fn stellar_available() -> bool {
    stellar_scenario::runner::find_stellar_cli(None).is_ok()
}

/// True when the resolved CLI can actually run.
///
/// `find_stellar_cli` returns `Ok` for any explicit `STELLAR_CLI` path,
/// including one that does not exist. A test that only checks `is_ok()` then
/// runs a real query and gets a spawn failure it did not expect. Probing with
/// `--version` catches that.
fn stellar_runnable() -> bool {
    match stellar_scenario::runner::find_stellar_cli(None) {
        Ok(path) => {
            let out = stellar_scenario::runner::run(
                path.as_os_str(),
                &[std::ffi::OsStr::new("--version")],
                std::time::Duration::from_secs(10),
            );
            out.is_ok()
        }
        Err(_) => false,
    }
}

#[test]
fn assert_against_funded_account_passes() {
    if !stellar_runnable() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let raw = r#"
schema_version: "1.0"
name: live_pass_probe
network: testnet
steps:
  - action: assert_balance
    identity: deployer
    asset: native
    min_balance: "1"
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("live_pass.yml");
    std::fs::write(&path, raw).expect("write scenario");

    let output = assert_cmd(path.to_str().unwrap())
        .output()
        .expect("run assert");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().unwrap();
    let results = parsed.get("results").unwrap().as_array().unwrap();
    assert_eq!(results.len(), 1);

    let status = results[0].get("status").and_then(|v| v.as_str()).unwrap();
    if status == "error" {
        let diag = results[0]
            .get("diagnostic")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if diag.contains("not Found") || diag.contains("account entry is missing") {
            eprintln!("skipping: deployer identity not funded on this host");
            return;
        }
        panic!("unexpected error status: {diag}");
    }
    assert_eq!(status, "pass");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn assert_against_funded_account_fails_when_too_high() {
    if !stellar_runnable() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let raw = r#"
schema_version: "1.0"
name: live_fail_probe
network: testnet
steps:
  - action: assert_balance
    identity: deployer
    asset: native
    min_balance: "999999999999999"
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("live_fail.yml");
    std::fs::write(&path, raw).expect("write scenario");

    let output = assert_cmd(path.to_str().unwrap())
        .output()
        .expect("run assert");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().unwrap();
    let results = parsed.get("results").unwrap().as_array().unwrap();

    let status = results[0].get("status").and_then(|v| v.as_str()).unwrap();
    if status == "error" {
        let diag = results[0]
            .get("diagnostic")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if diag.contains("not Found") || diag.contains("account entry is missing") {
            eprintln!("skipping: deployer identity not funded on this host");
            return;
        }
        panic!("unexpected error status: {diag}");
    }
    assert_eq!(status, "fail");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn assert_unknown_identity_reports_error() {
    if !stellar_available() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let raw = r#"
schema_version: "1.0"
name: live_unknown_identity
network: testnet
steps:
  - action: assert_balance
    identity: no_such_identity_xyz
    asset: native
    min_balance: "0"
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("live_unknown.yml");
    std::fs::write(&path, raw).expect("write scenario");

    let output = assert_cmd(path.to_str().unwrap())
        .output()
        .expect("run assert");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().unwrap();
    let results = parsed.get("results").unwrap().as_array().unwrap();

    let status = results[0].get("status").and_then(|v| v.as_str()).unwrap();
    assert_eq!(status, "error", "unknown identity must be an error");
    assert!(results[0].get("actual_balance").unwrap().is_null());
    assert_eq!(output.status.code(), Some(2));
}

/// An asset whose issuer is not a configured identity exits 1 with a JSON
/// error object on stdout. The structured path must run *before* the raw-text
/// fallback, or the JSON is read as arbitrary text and classified `unknown`.
/// Verified live: `stellar token balance --id UNRESOLVED:<addr> --output json`
/// → `{"error":{"type":"invalid_asset",...}}`, exit 1.
#[test]
fn assert_invalid_asset_issuer_is_classified() {
    if !stellar_runnable() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let output = assert_cmd(&fixture("assert_invalid_asset_issuer.yml"))
        .output()
        .expect("run assert");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().unwrap();
    let results = parsed.get("results").unwrap().as_array().unwrap();

    let status = results[0].get("status").and_then(|v| v.as_str()).unwrap();
    assert_eq!(status, "error");
    assert_eq!(output.status.code(), Some(2));

    let category = results[0]
        .get("diagnostic_category")
        .and_then(|v| v.as_str())
        .expect("diagnostic_category present");
    assert_eq!(category, "identity_not_configured");
    let remediation = results[0]
        .get("remediation")
        .and_then(|v| v.as_str())
        .expect("remediation present");
    assert!(
        remediation.contains("keys generate"),
        "remediation: {remediation}"
    );
}

// ------------------------------------------------------- report-level tests

#[test]
fn assert_report_status_precedence() {
    // Offline: build a report manually and verify precedence:
    // errored > failed > passed > no_assertions.
    use stellar_scenario::assert::{
        AssertBalanceResult, AssertionMode, ScenarioReport, ScenarioStatus,
    };
    use stellar_scenario::scenario::Scenario;

    let s = Scenario::from_str(
        r#"schema_version: "1.0"
name: r
network: testnet
steps:
  - action: assert_balance
    identity: alice
    min_balance: "1"
  - action: assert_balance
    identity: bob
    min_balance: "1"
"#,
    )
    .unwrap();

    let results = vec![
        AssertBalanceResult {
            scenario: s.name.clone(),
            step_index: 0,
            action: "assert_balance".into(),
            mode: AssertionMode::MinBalance,
            account: "alice".into(),
            network: s.network.clone(),
            asset: "native".into(),
            expected_min_balance: Some("1".into()),
            actual_balance: Some("0".into()),
            status: stellar_scenario::assert::AssertStatus::Fail,
            diagnostic: "balance 0 < expected 1".into(),
            diagnostic_category: None,
            remediation: None,
            execution_status: stellar_scenario::plan::ExecutionStatus::Executable,
            block_reason: None,
        },
        AssertBalanceResult {
            scenario: s.name.clone(),
            step_index: 1,
            action: "assert_balance".into(),
            mode: AssertionMode::MinBalance,
            account: "bob".into(),
            network: s.network.clone(),
            asset: "native".into(),
            expected_min_balance: Some("1".into()),
            actual_balance: None,
            status: stellar_scenario::assert::AssertStatus::Error,
            diagnostic: "timeout".into(),
            diagnostic_category: None,
            remediation: None,
            execution_status: stellar_scenario::plan::ExecutionStatus::Executable,
            block_reason: None,
        },
    ];
    let rep = ScenarioReport::new(&s, results, vec![]);
    assert_eq!(rep.status, ScenarioStatus::Errored);
    assert_eq!(rep.errors, 1);
    assert_eq!(rep.failed, 1);
    assert_eq!(rep.passed, 0);
}

#[test]
fn assert_report_no_assertions_when_all_skipped() {
    use stellar_scenario::assert::{ScenarioReport, ScenarioStatus, SkipReason, SkippedStep};
    use stellar_scenario::scenario::Scenario;

    let s = Scenario::from_str(
        r#"schema_version: "1.0"
name: r
network: testnet
steps:
  - action: ensure_funded
    identity: alice
"#,
    )
    .unwrap();
    let rep = ScenarioReport::new(
        &s,
        vec![],
        vec![SkippedStep {
            step_index: 0,
            action: "ensure_funded".into(),
            reason: SkipReason::WouldMutate,
            execution_status: stellar_scenario::plan::ExecutionStatus::WouldMutate,
            block_reason: Some("would mutate ledger state; blocked in plan mode".to_string()),
        }],
    );
    assert_eq!(rep.status, ScenarioStatus::NoAssertions);
    assert_eq!(rep.passed, 0);
    assert_eq!(rep.skipped_mutating, 1);
}

#[test]
fn assert_report_json_has_stable_field_order() {
    use stellar_scenario::assert::{AssertBalanceResult, AssertionMode, ScenarioReport};
    use stellar_scenario::scenario::Scenario;

    let s = Scenario::from_str(
        r#"schema_version: "1.0"
name: r
network: testnet
steps:
  - action: assert_balance
    identity: alice
    min_balance: "1"
"#,
    )
    .unwrap();
    let rep = ScenarioReport::new(
        &s,
        vec![AssertBalanceResult {
            scenario: s.name.clone(),
            step_index: 0,
            action: "assert_balance".into(),
            mode: AssertionMode::MinBalance,
            account: "alice".into(),
            network: s.network.clone(),
            asset: "native".into(),
            expected_min_balance: Some("1".into()),
            actual_balance: Some("5".into()),
            status: stellar_scenario::assert::AssertStatus::Pass,
            diagnostic: "ok".into(),
            diagnostic_category: None,
            remediation: None,
            execution_status: stellar_scenario::plan::ExecutionStatus::Executable,
            block_reason: None,
        }],
        vec![],
    );
    let json = rep.to_json();
    let scenario_pos = json.find("\"scenario\"").unwrap();
    let status_pos = json.find("\"status\"").unwrap();
    assert!(scenario_pos < status_pos);
    assert!(json.contains("\"schema_version\": \"1.0\""));
    assert!(json.contains("\"total_steps\": 1"));
}

#[test]
fn assert_report_text_lists_passes_fails_errors_and_skips() {
    use stellar_scenario::assert::{
        AssertBalanceResult, AssertionMode, ScenarioReport, SkipReason, SkippedStep,
    };
    use stellar_scenario::scenario::Scenario;

    let s = Scenario::from_str(
        r#"schema_version: "1.0"
name: r
network: testnet
steps:
  - action: ensure_funded
    identity: alice
  - action: assert_balance
    identity: bob
    min_balance: "1"
"#,
    )
    .unwrap();
    let rep = ScenarioReport::new(
        &s,
        vec![AssertBalanceResult {
            scenario: s.name.clone(),
            step_index: 1,
            action: "assert_balance".into(),
            mode: AssertionMode::MinBalance,
            account: "bob".into(),
            network: s.network.clone(),
            asset: "native".into(),
            expected_min_balance: Some("1".into()),
            actual_balance: Some("0".into()),
            status: stellar_scenario::assert::AssertStatus::Fail,
            diagnostic: "balance 0 < expected 1".into(),
            diagnostic_category: None,
            remediation: None,
            execution_status: stellar_scenario::plan::ExecutionStatus::Executable,
            block_reason: None,
        }],
        vec![SkippedStep {
            step_index: 0,
            action: "ensure_funded".into(),
            reason: SkipReason::WouldMutate,
            execution_status: stellar_scenario::plan::ExecutionStatus::WouldMutate,
            block_reason: Some("would mutate ledger state; blocked in plan mode".to_string()),
        }],
    );
    let text = rep.to_text();
    assert!(text.contains("failed"));
    assert!(text.contains("FAIL"));
    assert!(text.contains("SKIP"));
    assert!(text.contains("ensure_funded"));
}

// ------------------------------------------------------- JSONL output tests

#[test]
fn assert_jsonl_stdout_event_stream_and_stderr_aggregate() {
    if !stellar_available() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let payment = project_root().join("examples/payment.yml");
    let output = assert_cmd(payment.to_str().unwrap())
        .arg("--output")
        .arg("jsonl")
        .output()
        .expect("run assert --output jsonl");

    // stdout: one JSON object per line, no surrounding prose.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        !lines.is_empty(),
        "expected at least one JSONL event line, got: {stdout:?}"
    );
    let events: Vec<serde_json::Value> = lines
        .iter()
        .map(|l| {
            serde_json::from_str(l)
                .unwrap_or_else(|e| panic!("stdout line not valid JSON ({e}): {l}"))
        })
        .collect();

    // Every event carries a type discriminator.
    for ev in &events {
        let t = ev.get("type").and_then(|v| v.as_str());
        assert!(
            t == Some("result") || t == Some("skipped"),
            "event missing type discriminator: {ev:?}"
        );
    }

    // Result events preserve status and execution_status.
    let result_events: Vec<_> = events
        .iter()
        .filter(|e| e.get("type").and_then(|v| v.as_str()) == Some("result"))
        .collect();
    assert!(
        !result_events.is_empty(),
        "expected at least one result event"
    );
    for ev in &result_events {
        assert!(
            ev.get("execution_status").is_some(),
            "result event missing execution_status: {ev:?}"
        );
        assert!(
            ev.get("status").is_some(),
            "result event missing status: {ev:?}"
        );
    }

    // Skipped events preserve execution_status and block_reason.
    let skip_events: Vec<_> = events
        .iter()
        .filter(|e| e.get("type").and_then(|v| v.as_str()) == Some("skipped"))
        .collect();
    assert!(
        !skip_events.is_empty(),
        "expected at least one skipped event"
    );
    for ev in &skip_events {
        assert!(
            ev.get("execution_status").is_some(),
            "skipped event missing execution_status: {ev:?}"
        );
        assert!(
            ev.get("block_reason").is_some(),
            "mutating skipped event missing block_reason: {ev:?}"
        );
    }

    // stderr: compact aggregate JSON with step_counts.
    assert!(
        !stderr.trim().is_empty(),
        "expected stderr aggregate report, got: {stderr:?}"
    );
    let aggregate: serde_json::Value =
        serde_json::from_str(stderr.trim()).expect("stderr aggregate is valid JSON");
    assert!(
        aggregate.get("step_counts").is_some(),
        "aggregate missing step_counts: {aggregate:?}"
    );
    assert!(
        aggregate.get("results").is_some(),
        "aggregate missing results: {aggregate:?}"
    );
    assert!(
        aggregate.get("skipped").is_some(),
        "aggregate missing skipped: {aggregate:?}"
    );

    // Exit codes unchanged: payment.yml has a failing assertion (invalid
    // address), so exit 2 (errored) is expected.
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn assert_jsonl_rejects_invalid_scenario_cleanly() {
    // A scenario that never parses produces no step events. stdout must stay
    // empty (the machine-readable event stream), and the error report goes to
    // stderr as a compact JSON object.
    let output = assert_cmd(&fixture("invalid_assert_missing_min_balance.yml"))
        .arg("--output")
        .arg("jsonl")
        .output()
        .expect("run assert --output jsonl");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stdout.trim().is_empty(),
        "stdout must be empty on validation failure, got: {stdout:?}"
    );
    let aggregate: serde_json::Value =
        serde_json::from_str(stderr.trim()).expect("stderr report is valid JSON");
    assert_eq!(
        aggregate.get("result").and_then(|v| v.as_str()),
        Some("invalid")
    );
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn assert_json_and_text_output_unchanged() {
    // Backward compatibility: --output json and --output text produce their
    // original output streams (no JSONL event type discriminator).
    if !stellar_available() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let payment = project_root().join("examples/payment.yml");
    let json_out = assert_cmd(payment.to_str().unwrap())
        .output()
        .expect("run assert --output json (default)");
    let stdout = String::from_utf8_lossy(&json_out.stdout);
    assert!(
        !stdout.contains("\"type\": \"result\""),
        "json output must not contain JSONL type discriminator"
    );
    assert!(
        stdout.contains("\"step_counts\""),
        "json output keeps aggregate step_counts"
    );

    let text_out = assert_cmd(payment.to_str().unwrap())
        .arg("--output")
        .arg("text")
        .output()
        .expect("run assert --output text");
    assert!(
        String::from_utf8_lossy(&text_out.stdout).trim().is_empty(),
        "text output goes to stderr, not stdout"
    );
    assert!(!String::from_utf8_lossy(&text_out.stderr).trim().is_empty());
}
