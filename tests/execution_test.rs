//! Integration tests for the execution foundation: `plan` and `doctor`.
//!
//! These tests never contact a network and never fund an account. `plan`
//! spawns no subprocess at all; `doctor` only runs read-only Stellar CLI
//! commands against local config.

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

fn example(name: &str) -> String {
    project_root()
        .join("examples")
        .join(name)
        .to_str()
        .unwrap()
        .to_string()
}

// ------------------------------------------------------------------- plan

#[test]
fn plan_emits_json_without_running_anything() {
    let mut cmd = Command::new(bin());
    cmd.arg("plan").arg(example("payment.yml"));
    let output = cmd.output().expect("run plan");
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().expect("plan stdout is valid JSON");

    assert_eq!(
        parsed.get("scenario").and_then(|v| v.as_str()),
        Some("payment_alice_to_bob")
    );
    assert_eq!(
        parsed.get("network").and_then(|v| v.as_str()),
        Some("testnet")
    );
    assert_eq!(
        parsed.get("read_only").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(
        parsed
            .get("commands")
            .and_then(|v| v.as_array())
            .map(Vec::len),
        Some(3)
    );
}

#[test]
fn plan_marks_side_effects_per_step() {
    let mut cmd = Command::new(bin());
    cmd.arg("plan").arg(example("payment.yml"));
    let output = cmd.output().expect("run plan");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().unwrap();

    let commands = parsed.get("commands").unwrap().as_array().unwrap();
    assert_eq!(
        commands[0].get("side_effect").and_then(|v| v.as_str()),
        Some("mutates_remote_state")
    );
    assert_eq!(
        commands[2].get("side_effect").and_then(|v| v.as_str()),
        Some("read_only")
    );
}

#[test]
fn plan_read_only_scenario_reports_read_only() {
    let mut cmd = Command::new(bin());
    cmd.arg("plan").arg(fixture("valid_read_only.yml"));
    let output = cmd.output().expect("run plan");
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().unwrap();
    assert_eq!(
        parsed.get("read_only").and_then(|v| v.as_bool()),
        Some(true)
    );
}

#[test]
fn plan_text_output_goes_to_stderr() {
    let mut cmd = Command::new(bin());
    cmd.arg("plan")
        .arg(example("payment.yml"))
        .arg("--output")
        .arg("text");
    cmd.assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("keys fund"))
        .stderr(predicate::str::contains("token balance"));
}

#[test]
fn plan_rejects_invalid_scenario() {
    let mut cmd = Command::new(bin());
    cmd.arg("plan").arg(fixture("invalid_unknown_action.yml"));
    cmd.assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("unknown action"));
}

#[test]
fn plan_never_emits_secret_keys() {
    let mut cmd = Command::new(bin());
    cmd.arg("plan").arg(example("payment.yml"));
    let output = cmd.output().expect("run plan");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // No 56-character S-prefixed Stellar secret seed anywhere in the plan.
    for token in stdout.split(|c: char| !c.is_ascii_alphanumeric()) {
        assert!(
            !(token.len() == 56 && token.starts_with('S')),
            "plan output leaked a secret-looking token: {token}"
        );
    }
    assert!(
        stdout.contains("alice"),
        "identity names should still appear"
    );
}

#[test]
fn plan_does_not_mutate_remote_state() {
    // The scenario used here would fund and pay if executed. Running `plan`
    // must leave the local Stellar CLI identity list unchanged.
    let before = stellar_scenario::stellar::StellarCli::discover()
        .and_then(|c| c.list_identities())
        .expect("list identities before");

    let mut cmd = Command::new(bin());
    cmd.arg("plan").arg(example("payment.yml"));
    cmd.assert().success();

    let after = stellar_scenario::stellar::StellarCli::discover()
        .and_then(|c| c.list_identities())
        .expect("list identities after");

    assert_eq!(before, after, "plan must not create or remove identities");
}

// ----------------------------------------------------------------- doctor

#[test]
fn doctor_emits_structured_json() {
    let mut cmd = Command::new(bin());
    cmd.arg("doctor");
    let output = cmd.output().expect("run doctor");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().expect("doctor stdout is valid JSON");
    let checks = parsed.as_array().expect("array of checks");
    assert!(!checks.is_empty());
    assert!(checks.iter().all(|c| c.get("name").is_some()));
    assert!(checks.iter().all(|c| c.get("ok").is_some()));
}

#[test]
fn doctor_reports_stellar_cli_presence() {
    let mut cmd = Command::new(bin());
    cmd.arg("doctor");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("stellar_cli"));
}

#[test]
fn doctor_never_prints_secret_material() {
    let mut cmd = Command::new(bin());
    cmd.arg("doctor");
    let output = cmd.output().expect("run doctor");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    for token in combined.split(|c: char| !c.is_ascii_alphanumeric()) {
        assert!(
            !(token.len() == 56 && token.starts_with('S')),
            "doctor output leaked a secret-looking token: {token}"
        );
    }
}

#[test]
fn doctor_text_output_lists_checks() {
    let mut cmd = Command::new(bin());
    cmd.arg("doctor").arg("--output").arg("text");
    cmd.assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("stellar_cli"));
}
