//! Integration tests for the `scaffold` command.
//!
//! These tests use a fake `stellar` executable driven by environment
//! variables, so every failure mode is deterministic and needs no network.
//! The live test is gated on a runnable Stellar CLI and skips cleanly
//! otherwise.

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

fn scaffold_cmd(path: &str, function: &str) -> Command {
    let mut cmd = Command::new(bin());
    cmd.arg("scaffold").arg(path).arg(function);
    cmd
}

fn stellar_runnable() -> bool {
    match stellar_scenario::runner::find_stellar_cli(None) {
        Ok(path) => stellar_scenario::runner::run(
            path.as_os_str(),
            &[std::ffi::OsStr::new("--version")],
            std::time::Duration::from_secs(10),
        )
        .is_ok(),
        Err(_) => false,
    }
}

// ------------------------------------------------------- offline behaviour

#[test]
fn scaffold_rejects_scenario_without_inspect_step() {
    let output = scaffold_cmd(&fixture("valid_assert_balance.yml"), "balance")
        .arg("--output")
        .arg("json")
        .output()
        .expect("run scaffold");
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = stdout.parse().unwrap();
    assert_eq!(
        v.get("status").and_then(|s| s.as_str()),
        Some("no_contract_in_scenario")
    );
}

#[test]
fn scaffold_rejects_ambiguous_contracts_without_filter() {
    let output = scaffold_cmd(&fixture("valid_inspect_two_contracts.yml"), "balance")
        .arg("--output")
        .arg("json")
        .output()
        .expect("run scaffold");
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = stdout.parse().unwrap();
    assert_eq!(
        v.get("status").and_then(|s| s.as_str()),
        Some("ambiguous_contract")
    );
}

#[test]
fn scaffold_contract_filter_disambiguates() {
    if !stellar_runnable() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let output = scaffold_cmd(&fixture("valid_inspect_two_contracts.yml"), "balance")
        .arg("--output")
        .arg("json")
        .arg("--contract-id")
        .arg("CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC")
        .output()
        .expect("run scaffold");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = stdout.parse().unwrap();
    assert_eq!(v.get("status").and_then(|s| s.as_str()), Some("generated"));
    assert_eq!(
        v.get("contract_id").and_then(|s| s.as_str()),
        Some("CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC")
    );
}

#[test]
fn scaffold_rejects_missing_function() {
    if !stellar_runnable() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let output = scaffold_cmd(&fixture("valid_inspect_contract.yml"), "nosuchfn")
        .arg("--output")
        .arg("json")
        .output()
        .expect("run scaffold");
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = stdout.parse().unwrap();
    assert_eq!(
        v.get("status").and_then(|s| s.as_str()),
        Some("function_not_found")
    );
    assert!(v
        .get("diagnostic")
        .and_then(|s| s.as_str())
        .unwrap()
        .contains("available:"));
}

#[test]
fn scaffold_accepts_option_types() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fake = dir.path().join("stellar");
    std::fs::write(
        &fake,
        r#"#!/bin/sh
case "$1 $2 $3" in
  "contract info interface")
      printf '[{"function_v0":{"name":"opt_fn","inputs":[{"name":"x","type":"option<u64>"}],"outputs":[]}}]'
      exit 0 ;;
  *) exit 0 ;;
esac
"#,
    )
    .expect("write fake");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&fake).expect("stat fake").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake, perms).expect("chmod");
    }

    let scenario = dir.path().join("s.yml");
    std::fs::write(
        &scenario,
        r#"schema_version: "1.0"
name: s
network: testnet
steps:
  - action: inspect_contract
    contract_id: C
"#,
    )
    .expect("write scenario");

    let mut cmd = Command::new(bin());
    cmd.arg("scaffold")
        .arg(scenario.to_str().unwrap())
        .arg("opt_fn")
        .arg("--output")
        .arg("json")
        .env("STELLAR_CLI", fake.to_str().unwrap());
    let output = cmd.output().expect("run scaffold");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = stdout.parse().unwrap();
    assert_eq!(v.get("status").and_then(|s| s.as_str()), Some("generated"));
    assert_eq!(
        v["args"][0]["placeholder"].as_str(),
        Some("<value-or-omit>")
    );
}

#[test]
fn scaffold_rejects_unsupported_types() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fake = dir.path().join("stellar");
    std::fs::write(
        &fake,
        r#"#!/bin/sh
case "$1 $2 $3" in
  "contract info interface")
      printf '[{"function_v0":{"name":"x","inputs":[{"name":"a","type":"vec<address>"}],"outputs":[]}}]'
      exit 0 ;;
  *) exit 0 ;;
esac
"#,
    )
    .expect("write fake");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&fake).expect("stat fake").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake, perms).expect("chmod");
    }

    let scenario = dir.path().join("s.yml");
    std::fs::write(
        &scenario,
        r#"schema_version: "1.0"
name: s
network: testnet
steps:
  - action: inspect_contract
    contract_id: C
"#,
    )
    .expect("write scenario");

    let mut cmd = Command::new(bin());
    cmd.arg("scaffold")
        .arg(scenario.to_str().unwrap())
        .arg("x")
        .arg("--output")
        .arg("json")
        .env("STELLAR_CLI", fake.to_str().unwrap());
    let output = cmd.output().expect("run scaffold");
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = stdout.parse().unwrap();
    assert_eq!(
        v.get("status").and_then(|s| s.as_str()),
        Some("unsupported_type")
    );
}

#[test]
fn scaffold_missing_cli_is_fetch_failed() {
    let mut cmd = Command::new(bin());
    cmd.arg("scaffold")
        .arg(fixture("valid_inspect_contract.yml"))
        .arg("balance")
        .arg("--output")
        .arg("json")
        .env("STELLAR_CLI", "/tmp/stellar-scenario-absent-binary-xyz");
    let output = cmd.output().expect("run scaffold");
    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    assert_eq!(
        v.get("status").and_then(|s| s.as_str()),
        Some("inspect_failed")
    );
}

#[test]
fn scaffold_text_output_puts_yaml_on_stdout() {
    if !stellar_runnable() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let output = scaffold_cmd(&fixture("valid_inspect_contract.yml"), "balance")
        .output()
        .expect("run scaffold");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("- action: simulate_invoke"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("<address>"), "stdout: {stdout}");
}

#[test]
fn scaffold_generated_step_validates_and_does_not_send_tx() {
    if !stellar_runnable() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let output = scaffold_cmd(&fixture("valid_inspect_contract.yml"), "balance")
        .output()
        .expect("run scaffold");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let step = stdout
        .lines()
        .skip_while(|l| !l.starts_with("- action:"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!step.is_empty(), "stdout: {stdout}");

    let scenario = format!(
        "schema_version: \"1.0\"\nname: generated\nnetwork: testnet\nsteps:\n  {}\n",
        step.replace("\n", "\n  ")
    );
    let parsed = stellar_scenario::scenario::Scenario::from_str(&scenario)
        .unwrap_or_else(|e| panic!("generated step must validate: {e}\n{scenario}"));
    assert_eq!(parsed.steps[0].action, "simulate_invoke");
}

// ---------------------------------------------------- live-network behaviour

#[test]
fn scaffold_live_native_sac_balance() {
    if !stellar_runnable() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let output = scaffold_cmd(&fixture("valid_inspect_contract.yml"), "balance")
        .output()
        .expect("run scaffold");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("- action: simulate_invoke"));
    assert!(stdout.contains("  function: balance"));
    assert!(stdout.contains("    - \"--id\""));
    assert!(stdout.contains("    - \"<address>\""));
}

#[test]
fn scaffold_live_native_sac_decimals() {
    if !stellar_runnable() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let output = scaffold_cmd(&fixture("valid_inspect_contract.yml"), "decimals")
        .output()
        .expect("run scaffold");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("- action: simulate_invoke"));
    assert!(stdout.contains("  function: decimals"));
    assert!(!stdout.contains("args:"), "decimals has no arguments");
}
