//! Subprocess and RPC robustness tests for `assert`.
//!
//! These tests use a fake `stellar` executable driven by environment
//! variables, so every failure mode is deterministic and needs no network:
//!
//!   - empty stdout/stderr
//!   - malformed JSON
//!   - error JSON on stdout with exit code 0
//!   - non-zero exit with empty streams
//!   - non-zero exit with a real stderr message
//!   - boundary and malformed balance values
//!
//! The fake is written to a tempdir and pointed at with `STELLAR_CLI`, so
//! the tests never touch the real Stellar CLI or a live RPC.

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

/// Write a fake `stellar` that answers `token balance` from env vars.
fn fake_stellar(dir: &std::path::Path) -> PathBuf {
    let path = dir.join("stellar");
    std::fs::write(
        &path,
        r#"#!/bin/sh
case "$1 $2" in
  "token balance")
      if [ -n "$FAKE_BAL" ]; then printf '%s' "$FAKE_BAL"; fi
      if [ -n "$FAKE_BAL_ERR" ]; then printf '%s' "$FAKE_BAL_ERR" >&2; fi
      exit "${FAKE_BAL_EXIT:-0}" ;;
  *) exit 0 ;;
esac
"#,
    )
    .expect("write fake stellar");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path).expect("stat fake").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod");
    }
    path
}

/// Run `assert` against the fake CLI and return the parsed JSON report.
fn assert_with_fake(bal: &str, bal_exit: &str, bal_err: &str) -> serde_json::Value {
    let dir = tempfile::tempdir().expect("tempdir");
    let fake = fake_stellar(dir.path());

    let scenario = dir.path().join("scenario.yml");
    std::fs::write(
        &scenario,
        r#"schema_version: "1.0"
name: probe
network: testnet
timeout_seconds: 30
steps:
  - action: assert_balance
    identity: deployer
    min_balance: "1"
"#,
    )
    .expect("write scenario");

    let mut cmd = Command::new(bin());
    cmd.arg("assert")
        .arg(scenario.to_str().unwrap())
        .env("STELLAR_CLI", fake.to_str().unwrap())
        .env("FAKE_BAL", bal)
        .env("FAKE_BAL_EXIT", bal_exit)
        .env("FAKE_BAL_ERR", bal_err);
    let output = cmd.output().expect("run assert");
    String::from_utf8_lossy(&output.stdout)
        .parse()
        .expect("assert stdout is valid JSON")
}

fn first_result(report: &serde_json::Value) -> &serde_json::Value {
    &report["results"][0]
}

// ------------------------------------------------------------- happy path

#[test]
fn fake_cli_success_passes() {
    let r = assert_with_fake("{\"balance\":\"500\"}", "0", "");
    let res = first_result(&r);
    assert_eq!(res["status"], "pass");
    assert_eq!(res["actual_balance"], "500");
    assert_eq!(r["status"], "passed");
}

#[test]
fn fake_cli_below_minimum_fails() {
    let r = assert_with_fake("{\"balance\":\"0\"}", "0", "");
    let res = first_result(&r);
    assert_eq!(res["status"], "fail");
    assert_eq!(r["status"], "failed");
}

// ------------------------------------------------- error JSON on stdout

#[test]
fn error_json_on_stdout_with_exit_zero_is_an_error() {
    let r = assert_with_fake(
        "{\"error\":{\"type\":\"config\",\"message\":\"bad net\"}}",
        "0",
        "",
    );
    let res = first_result(&r);
    assert_eq!(res["status"], "error");
    assert!(res["actual_balance"].is_null());
    assert!(res["diagnostic"].as_str().unwrap().contains("bad net"));
}

// -------------------------------------------------------- malformed JSON

#[test]
fn malformed_json_is_an_error_not_a_zero() {
    for bad in [
        "{\"balance\":",
        "not json at all",
        "",
        "   ",
        "[]",
        "null",
        "{\"balance\":123}",
        "{\"balance\":null}",
        "{\"balance\":{}}",
    ] {
        let r = assert_with_fake(bad, "0", "");
        let res = first_result(&r);
        assert_eq!(
            res["status"], "error",
            "input {bad:?} must be an error, got {}",
            res["diagnostic"]
        );
        assert!(
            res["actual_balance"].is_null(),
            "input {bad:?} must not produce a balance"
        );
    }
}

// ------------------------------------------------- non-zero exit handling

#[test]
fn nonzero_exit_with_empty_streams_reports_exit_code() {
    let r = assert_with_fake("", "3", "");
    let res = first_result(&r);
    assert_eq!(res["status"], "error");
    let diag = res["diagnostic"].as_str().unwrap();
    assert!(
        diag.contains("code 3"),
        "diagnostic must name the exit code: {diag}"
    );
    assert!(
        !diag.contains("EOF while parsing"),
        "diagnostic must not masquerade as a JSON parse failure: {diag}"
    );
}

#[test]
fn nonzero_exit_surfaces_cli_stderr() {
    let r = assert_with_fake("", "1", "❌ error: account not found");
    let res = first_result(&r);
    assert_eq!(res["status"], "error");
    let diag = res["diagnostic"].as_str().unwrap();
    assert!(diag.contains("code 1"), "missing exit code: {diag}");
    assert!(
        diag.contains("account not found"),
        "CLI stderr must survive into the diagnostic: {diag}"
    );
}

#[test]
fn nonzero_exit_prefers_stderr_over_stdout() {
    // Both streams carry text; stderr is the CLI's real error channel.
    let r = assert_with_fake("partial junk", "2", "the real reason");
    let res = first_result(&r);
    let diag = res["diagnostic"].as_str().unwrap();
    assert!(diag.contains("the real reason"), "diag: {diag}");
    assert!(!diag.contains("partial junk"), "diag: {diag}");
}

// ------------------------------------------------- boundary balance values

#[test]
fn boundary_balance_values_are_compared_exactly() {
    // u128::MAX passes validation; the comparison must still be exact.
    let max = "340282366920938463463374607431768211455";
    let r = assert_with_fake(&format!("{{\"balance\":\"{max}\"}}"), "0", "");
    assert_eq!(first_result(&r)["status"], "pass");

    let r = assert_with_fake(&format!("{{\"balance\":\"{}\"}}", max), "0", "");
    assert_eq!(first_result(&r)["status"], "pass");
}

#[test]
fn malformed_balance_values_are_errors_not_zeros() {
    for bad in ["12.5", "-1", "+1", "1e10", " 1", "1 ", "0x10", "1,000", ""] {
        let r = assert_with_fake(&format!("{{\"balance\":\"{bad}\"}}"), "0", "");
        let res = first_result(&r);
        assert_eq!(
            res["status"], "error",
            "balance {bad:?} must be an error, got {}",
            res["diagnostic"]
        );
    }
}

#[test]
fn balance_above_u128_is_an_error() {
    let r = assert_with_fake(
        "{\"balance\":\"340282366920938463463374607431768211456\"}",
        "0",
        "",
    );
    assert_eq!(first_result(&r)["status"], "error");
}

// ------------------------------------------------------ secret redaction

#[test]
fn cli_stderr_seed_is_redacted() {
    let seed = format!("S{}", "A".repeat(55));
    let r = assert_with_fake("", "1", &format!("failed with key {seed}"));
    let diag = first_result(&r)["diagnostic"].as_str().unwrap();
    assert!(!diag.contains(&seed), "seed leaked: {diag}");
    assert!(diag.contains("<redacted-secret>"), "diag: {diag}");
}

// --------------------------------------------------------- report shape

#[test]
fn report_stays_well_formed_on_every_failure_mode() {
    for (bal, code) in [
        ("{\"balance\":\"5\"}", "0"),
        ("{\"balance\":\"0\"}", "0"),
        ("garbage", "0"),
        ("", "0"),
        ("", "1"),
        ("{\"error\":{\"type\":\"x\",\"message\":\"y\"}}", "0"),
    ] {
        let r = assert_with_fake(bal, code, "");
        for key in [
            "scenario",
            "schema_version",
            "network",
            "total_steps",
            "passed",
            "failed",
            "errors",
            "status",
            "results",
            "skipped",
        ] {
            assert!(r.get(key).is_some(), "missing {key} for input {bal:?}");
        }
        assert!(matches!(
            r["status"].as_str().unwrap(),
            "passed" | "failed" | "errored" | "no_assertions"
        ));
    }
}

#[test]
fn missing_cli_is_reported_as_a_structured_error() {
    let mut cmd = Command::new(bin());
    cmd.arg("assert")
        .arg(fixture("valid_assert_balance.yml"))
        .env("STELLAR_CLI", "/tmp/stellar-scenario-absent-binary-xyz");
    let output = cmd.output().expect("run assert");
    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    // Either a bare error object (CLI discovery failed) or a report whose
    // only result errored. Both are exit 2 with no fabricated balance.
    let has_error_object = v.get("error").is_some();
    let has_errored_result = v
        .get("results")
        .and_then(|r| r.as_array())
        .map(|a| a.iter().any(|x| x["status"] == "error"))
        .unwrap_or(false);
    assert!(has_error_object || has_errored_result, "stdout: {stdout}");
}
