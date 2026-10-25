//! Integration tests for `simulate_invoke` failure diagnostics.
//!
//! These tests use a fake `stellar` executable driven by environment
//! variables, so every failure mode is deterministic and needs no network.
//! Each error string below was copied from real Stellar CLI 28.1.0 output.

use assert_cmd::Command;
use std::path::PathBuf;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn bin() -> PathBuf {
    project_root().join("target/debug/stellar-scenario")
}

/// Write a fake `stellar` that answers each subcommand from env vars.
///
/// `FAKE_BUILD_ERR` goes to stderr with `FAKE_BUILD_EXIT` for
/// `contract invoke`; `FAKE_SIM_ERR` / `FAKE_SIM_EXIT` for `tx simulate`.
fn fake_stellar(dir: &std::path::Path) -> PathBuf {
    let path = dir.join("stellar");
    std::fs::write(
        &path,
        r#"#!/bin/sh
case "$1 $2" in
  "contract invoke")
      if [ -n "$FAKE_BUILD_ERR" ]; then printf '%s' "$FAKE_BUILD_ERR" >&2; fi
      if [ -n "$FAKE_BUILD_OUT" ]; then printf '%s' "$FAKE_BUILD_OUT"; fi
      exit "${FAKE_BUILD_EXIT:-0}" ;;
  "tx simulate")
      if [ -n "$FAKE_SIM_ERR" ]; then printf '%s' "$FAKE_SIM_ERR" >&2; fi
      if [ -n "$FAKE_SIM_OUT" ]; then printf '%s' "$FAKE_SIM_OUT"; fi
      exit "${FAKE_SIM_EXIT:-0}" ;;
  "tx decode")
      if [ -n "$FAKE_DECODE_ERR" ]; then printf '%s' "$FAKE_DECODE_ERR" >&2; fi
      if [ -n "$FAKE_DECODE_OUT" ]; then printf '%s' "$FAKE_DECODE_OUT"; fi
      exit "${FAKE_DECODE_EXIT:-0}" ;;
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

/// Run `simulate` against the fake CLI and return the parsed report.
fn simulate_with_fake(
    build_err: &str,
    build_exit: &str,
    sim_err: &str,
    sim_exit: &str,
) -> serde_json::Value {
    let dir = tempfile::tempdir().expect("tempdir");
    let fake = fake_stellar(dir.path());

    let scenario = dir.path().join("scenario.yml");
    std::fs::write(
        &scenario,
        r#"schema_version: "1.0"
name: probe
network: testnet
source: deployer
timeout_seconds: 30
steps:
  - action: simulate_invoke
    contract_id: CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC
    function: balance
    args: ["--id", "deployer"]
"#,
    )
    .expect("write scenario");

    let mut cmd = Command::new(bin());
    cmd.arg("simulate")
        .arg(scenario.to_str().unwrap())
        .env("STELLAR_CLI", fake.to_str().unwrap())
        .env("FAKE_BUILD_ERR", build_err)
        .env("FAKE_BUILD_EXIT", build_exit)
        .env("FAKE_BUILD_OUT", "FAKEENVELOPE")
        .env("FAKE_SIM_ERR", sim_err)
        .env("FAKE_SIM_EXIT", sim_exit);
    let output = cmd.output().expect("run simulate");
    String::from_utf8_lossy(&output.stdout)
        .parse()
        .expect("simulate stdout is valid JSON")
}

fn first(report: &serde_json::Value) -> &serde_json::Value {
    &report["results"][0]
}

// ---------------------------------------------------- build-stage failures

#[test]
fn contract_not_found_is_classified() {
    let r = simulate_with_fake(
        "❌ error: contract not found: CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC",
        "1",
        "",
        "0",
    );
    let res = first(&r);
    assert_eq!(res["status"], "build_failed");
    assert_eq!(res["diagnostic_category"], "contract_not_found");
    assert!(res["remediation"]
        .as_str()
        .unwrap()
        .contains("contract info interface"));
}

#[test]
fn function_not_found_is_classified() {
    let r = simulate_with_fake(
        "error: unrecognized subcommand 'nosuchfn'\n\nUsage: stellar contract invoke ...\n",
        "2",
        "",
        "0",
    );
    let res = first(&r);
    assert_eq!(res["status"], "build_failed");
    assert_eq!(res["diagnostic_category"], "function_not_found");
    assert!(res["remediation"].as_str().unwrap().contains("inspect"));
}

#[test]
fn missing_argument_is_classified() {
    let r = simulate_with_fake(
        "❌ error: Missing required argument 'id' of type address (G... for account, C... for contract, or identity name)\n\nSuggestions:\n- Add the argument: --id <value>",
        "1",
        "",
        "0",
    );
    let res = first(&r);
    assert_eq!(res["diagnostic_category"], "missing_argument");
}

#[test]
fn invalid_argument_is_classified() {
    let r = simulate_with_fake(
        "❌ error: Failed to parse argument 'id': invalid type: number, expected string or map\n\nContext: Expected type address, but received: '12345'",
        "1",
        "",
        "0",
    );
    let res = first(&r);
    assert_eq!(res["diagnostic_category"], "invalid_argument");
}

#[test]
fn network_not_configured_is_classified() {
    let r = simulate_with_fake(
        "❌ error: Failed to find config network for nosuchnet",
        "1",
        "",
        "0",
    );
    let res = first(&r);
    assert_eq!(res["diagnostic_category"], "network_not_configured");
    assert!(res["remediation"].as_str().unwrap().contains("network add"));
}

#[test]
fn identity_not_configured_is_classified() {
    let r = simulate_with_fake(
        "❌ error: Failed to find config identity for nosuchidentity",
        "1",
        "",
        "0",
    );
    let res = first(&r);
    assert_eq!(res["diagnostic_category"], "identity_not_configured");
    assert!(res["remediation"]
        .as_str()
        .unwrap()
        .contains("keys generate"));
}

// -------------------------------------------------- simulation-stage failures

#[test]
fn unsupported_operation_keeps_its_status() {
    let r = simulate_with_fake("", "0", "❌ error: transaction simulation failed: transaction contains unsupported operation type: OperationTypePayment", "1");
    let res = first(&r);
    assert_eq!(res["status"], "unsupported_operation");
    assert_eq!(res["diagnostic_category"], "unsupported_operation");
    assert_eq!(r["status"], "unsupported_operation");
}

#[test]
fn host_error_is_classified() {
    let r = simulate_with_fake(
        "",
        "0",
        "❌ error: transaction simulation failed: HostError: Error(Storage, MissingValue)\n\nEvent log (newest first):\n   0: [Diagnostic Event] topics:[fn_call, CAAA, admin]",
        "1",
    );
    let res = first(&r);
    assert_eq!(res["status"], "simulate_failed");
    assert_eq!(res["diagnostic_category"], "host_error");
}

#[test]
fn rpc_unreachable_is_classified() {
    let r = simulate_with_fake("", "0", "❌ error: client error (Connect)", "1");
    let res = first(&r);
    assert_eq!(res["diagnostic_category"], "rpc_unreachable");
}

#[test]
fn empty_simulation_output_is_classified() {
    let r = simulate_with_fake("", "0", "", "0");
    let res = first(&r);
    assert_eq!(res["status"], "simulate_failed");
    assert_eq!(res["diagnostic_category"], "empty_output");
}

#[test]
fn empty_build_output_is_classified() {
    // No FAKE_BUILD_OUT, so the build stage succeeds with empty stdout.
    let dir = tempfile::tempdir().expect("tempdir");
    let fake = fake_stellar(dir.path());
    let scenario = dir.path().join("scenario.yml");
    std::fs::write(
        &scenario,
        r#"schema_version: "1.0"
name: probe
network: testnet
source: deployer
steps:
  - action: simulate_invoke
    contract_id: CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC
    function: decimals
"#,
    )
    .expect("write scenario");

    let mut cmd = Command::new(bin());
    cmd.arg("simulate")
        .arg(scenario.to_str().unwrap())
        .env("STELLAR_CLI", fake.to_str().unwrap());
    let output = cmd.output().expect("run simulate");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = stdout.parse().unwrap();
    let res = first(&v);
    assert_eq!(res["status"], "build_failed");
    assert_eq!(res["diagnostic_category"], "empty_output");
}

// ------------------------------------------------------------- unknown errors

#[test]
fn unknown_error_keeps_the_original_message() {
    let weird = "❌ error: some entirely new failure mode appeared";
    let r = simulate_with_fake(weird, "1", "", "0");
    let res = first(&r);
    assert_eq!(res["status"], "build_failed");
    assert_eq!(res["diagnostic_category"], "unknown");
    assert_eq!(
        res["diagnostic"],
        format!("failed to build envelope: {weird}")
    );
    assert!(res["remediation"]
        .as_str()
        .unwrap()
        .contains("not recognized"));
}

#[test]
fn a_message_mentioning_contract_is_not_misclassified() {
    let r = simulate_with_fake(
        "❌ error: contract invocation succeeded but returned nothing",
        "1",
        "",
        "0",
    );
    let res = first(&r);
    assert_eq!(res["diagnostic_category"], "unknown");
}

// ---------------------------------------------------------------- redaction

#[test]
fn diagnostic_redacts_secret_seeds() {
    let seed = format!("S{}", "A".repeat(55));
    let r = simulate_with_fake(
        &format!("❌ error: contract not found: CAAA (key {seed})"),
        "1",
        "",
        "0",
    );
    let res = first(&r);
    let diag = res["diagnostic"].as_str().unwrap();
    assert!(!diag.contains(&seed), "seed leaked into diagnostic: {diag}");
    assert!(diag.contains("<redacted-secret>"), "diag: {diag}");
}

#[test]
fn remediation_is_static_and_cannot_leak_input() {
    // The remediation is a fixed string per category, so no CLI text can
    // reach it.
    let seed = format!("S{}", "A".repeat(55));
    let r = simulate_with_fake(
        &format!("❌ error: contract not found: {seed}"),
        "1",
        "",
        "0",
    );
    let res = first(&r);
    let fix = res["remediation"].as_str().unwrap();
    assert!(!fix.contains(&seed), "seed leaked into remediation: {fix}");
}

// -------------------------------------------------------------- report shape

#[test]
fn success_has_no_diagnostic_category() {
    // A successful simulation must not carry a category, so a consumer that
    // treats its presence as a failure is not misled.
    let dir = tempfile::tempdir().expect("tempdir");
    let fake = fake_stellar(dir.path());
    let scenario = dir.path().join("scenario.yml");
    std::fs::write(
        &scenario,
        r#"schema_version: "1.0"
name: probe
network: testnet
source: deployer
steps:
  - action: simulate_invoke
    contract_id: CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC
    function: decimals
"#,
    )
    .expect("write scenario");

    let mut cmd = Command::new(bin());
    cmd.arg("simulate")
        .arg(scenario.to_str().unwrap())
        .env("STELLAR_CLI", fake.to_str().unwrap())
        .env("FAKE_BUILD_OUT", "FAKEENVELOPE")
        .env("FAKE_SIM_OUT", "SIMULATEDENVELOPE");
    let output = cmd.output().expect("run simulate");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = stdout.parse().unwrap();
    let res = first(&v);
    assert_eq!(res["status"], "simulated");
    assert!(
        res.get("diagnostic_category").is_none(),
        "success must not carry a category: {stdout}"
    );
    assert!(res.get("remediation").is_none(), "stdout: {stdout}");
}

#[test]
fn text_output_shows_the_remediation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fake = fake_stellar(dir.path());
    let scenario = dir.path().join("scenario.yml");
    std::fs::write(
        &scenario,
        r#"schema_version: "1.0"
name: probe
network: testnet
source: deployer
steps:
  - action: simulate_invoke
    contract_id: CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC
    function: decimals
"#,
    )
    .expect("write scenario");

    let mut cmd = Command::new(bin());
    cmd.arg("simulate")
        .arg(scenario.to_str().unwrap())
        .arg("--output")
        .arg("text")
        .env("STELLAR_CLI", fake.to_str().unwrap())
        .env(
            "FAKE_BUILD_ERR",
            "❌ error: contract not found: CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC",
        )
        .env("FAKE_BUILD_EXIT", "1");
    let output = cmd.output().expect("run simulate");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("contract_not_found"), "stderr: {stderr}");
    assert!(
        stderr.contains("contract info interface"),
        "stderr: {stderr}"
    );
}
