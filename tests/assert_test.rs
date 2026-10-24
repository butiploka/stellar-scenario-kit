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
    assert_cmd(&fixture("invalid_assert_negative_balance.yml"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("min_balance"));
}

#[test]
fn assert_emits_structured_json() {
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
    assert!(parsed.get("skipped_steps").is_some());
}

#[test]
fn assert_skips_mutating_steps() {
    if !stellar_available() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    // The payment scenario funds and pays. `assert` must skip those steps
    // rather than execute them.
    let payment = project_root().join("examples/payment.yml");
    let output = assert_cmd(payment.to_str().unwrap())
        .output()
        .expect("run assert");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().unwrap();
    let skipped = parsed
        .get("skipped_steps")
        .and_then(|v| v.as_array())
        .expect("skipped_steps array");

    // Steps 0 (ensure_funded) and 1 (payment) mutate remote state.
    assert_eq!(skipped.len(), 2, "skipped: {skipped:?}");
    assert!(skipped.contains(&serde_json::json!(0)));
    assert!(skipped.contains(&serde_json::json!(1)));

    // Only the read-only assert_balance step produced a result.
    let results = parsed
        .get("results")
        .and_then(|v| v.as_array())
        .expect("results array");
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].get("step_index").and_then(|v| v.as_u64()),
        Some(2)
    );
}

#[test]
fn assert_never_invokes_funding_or_payment() {
    if !stellar_available() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    // A scenario whose only steps are ensure_funded and payment must produce
    // zero executed results. If the runner ever started executing those,
    // this test would see results and fail.
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
        .get("skipped_steps")
        .and_then(|v| v.as_array())
        .expect("skipped_steps array");
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

#[test]
fn assert_against_funded_account_passes() {
    if !stellar_available() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    // `deployer` is a pre-existing funded testnet identity on this host.
    // If it is absent, the query returns an error and the test skips.
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
    assert_eq!(
        status,
        "pass",
        "diagnostic: {}",
        results[0]
            .get("diagnostic")
            .and_then(|v| v.as_str())
            .unwrap_or("")
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn assert_against_funded_account_fails_when_too_high() {
    if !stellar_available() {
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
