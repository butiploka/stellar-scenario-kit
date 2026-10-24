//! Integration tests for the `simulate` command.
//!
//! These tests never submit a transaction. The only subprocesses spawned are
//! `stellar contract invoke --build-only`, `stellar tx simulate`, and
//! `stellar tx decode`, all of which are read-only. Tests that need a live
//! network are gated on the Stellar CLI being present; the rest run offline.

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

fn simulate_cmd(path: &str) -> Command {
    let mut cmd = Command::new(bin());
    cmd.arg("simulate").arg(path);
    cmd
}

fn stellar_available() -> bool {
    stellar_scenario::runner::find_stellar_cli(None).is_ok()
}

// ------------------------------------------------------- offline behaviour

#[test]
fn simulate_rejects_missing_contract_id() {
    simulate_cmd(&fixture("invalid_simulate_missing_contract.yml"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("contract_id"));
}

#[test]
fn simulate_rejects_missing_function() {
    simulate_cmd(&fixture("invalid_simulate_missing_function.yml"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("function"));
}

#[test]
fn simulate_rejects_non_string_args() {
    simulate_cmd(&fixture("invalid_simulate_bad_args.yml"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("args"));
}

#[test]
fn simulate_rejects_non_string_arg_element() {
    simulate_cmd(&fixture("invalid_simulate_arg_element.yml"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("each arg must be a string"));
}

#[test]
fn simulate_reports_wrong_arg_type_as_build_failure() {
    if !stellar_available() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    // The CLI type-checks Soroban arguments against the contract spec. A
    // number where an Address is expected must surface as a build failure
    // with the CLI's own message, not be silently coerced.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("wrong_type.yml");
    std::fs::write(
        &path,
        r#"schema_version: "1.0"
name: wrong_arg_type
network: testnet
source: GDIPVO6M4SADGLNFTIMYHCTMMOXNONMV4ORZOMP6OAP2DPPYRQBMAW26
steps:
  - action: simulate_invoke
    contract_id: CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC
    function: balance
    args: ["--id", "12345"]
"#,
    )
    .expect("write scenario");

    let output = simulate_cmd(path.to_str().unwrap())
        .output()
        .expect("run simulate");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().unwrap();
    let results = parsed.get("results").unwrap().as_array().unwrap();

    let status = results[0].get("status").and_then(|v| v.as_str()).unwrap();
    // Either the build fails (type rejected by the CLI) or the network is
    // unavailable. Both are non-success; the assertion is that a wrong type
    // never becomes a successful simulation.
    assert_ne!(
        status, "simulated",
        "a wrong argument type must not simulate successfully: {stdout}"
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        parsed.get("submitted").and_then(|v| v.as_bool()),
        Some(false)
    );
}

#[test]
fn simulate_accepts_valid_scenario() {
    // Validation must accept `simulate_invoke`. Whether the live simulation
    // succeeds depends on network availability, so this only asserts that
    // the command did not fail validation.
    let output = simulate_cmd(&fixture("valid_simulate_invoke.yml"))
        .output()
        .expect("run simulate");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("\"result\": \"invalid\""),
        "scenario should validate: {stdout}"
    );
}

#[test]
fn simulate_report_marks_not_submitted() {
    if !stellar_available() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let output = simulate_cmd(&fixture("valid_simulate_invoke.yml"))
        .output()
        .expect("run simulate");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().expect("valid JSON");

    assert_eq!(
        parsed.get("submitted").and_then(|v| v.as_bool()),
        Some(false),
        "report must state submitted=false"
    );
    assert!(parsed.get("results").is_some());
    assert!(parsed.get("status").is_some());
}

#[test]
fn simulate_skips_mutating_steps() {
    if !stellar_available() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let output = simulate_cmd(&fixture("valid_simulate_with_mutation.yml"))
        .output()
        .expect("run simulate");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().unwrap();

    assert_eq!(
        parsed.get("skipped").and_then(|v| v.as_u64()),
        Some(1),
        "the payment step must be skipped, never simulated"
    );
    let results = parsed.get("results").unwrap().as_array().unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn simulate_never_invokes_submission() {
    if !stellar_available() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    // A scenario whose only steps are ensure_funded and payment must produce
    // zero results. If the runner ever reached a submission command, this
    // test would see results and fail.
    let output = simulate_cmd(&fixture("valid_mutating_only.yml"))
        .output()
        .expect("run simulate");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().unwrap();

    let results = parsed.get("results").unwrap().as_array().unwrap();
    assert!(
        results.is_empty(),
        "no step should be executed: {results:?}"
    );
    assert_eq!(parsed.get("skipped").and_then(|v| v.as_u64()), Some(2));
}

#[test]
fn simulate_never_invokes_submission_or_funding_commands() {
    if !stellar_available() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    // A fake `stellar` on PATH records every argv it is given, then exits 0
    // with empty output. Any attempt to run `tx send`, `keys fund`,
    // `keys generate`, or `contract deploy` would be captured here.
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("argv.log");
    let fake = dir.path().join("stellar");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\nexit 0\n",
            log.display()
        ),
    )
    .expect("write fake stellar");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&fake).expect("stat fake").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake, perms).expect("chmod");
    }

    let scenario = dir.path().join("scenario.yml");
    std::fs::write(
        &scenario,
        r#"schema_version: "1.0"
name: argv_probe
network: testnet
source: GDIPVO6M4SADGLNFTIMYHCTMMOXNONMV4ORZOMP6OAP2DPPYRQBMAW26
timeout_seconds: 30
steps:
  - action: simulate_invoke
    contract_id: CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC
    function: decimals
  - action: ensure_funded
    identity: GDIPVO6M4SADGLNFTIMYHCTMMOXNONMV4ORZOMP6OAP2DPPYRQBMAW26
"#,
    )
    .expect("write scenario");

    let mut cmd = Command::new(bin());
    cmd.arg("simulate")
        .arg(scenario.to_str().unwrap())
        .env("STELLAR_CLI", fake.to_str().unwrap());
    let output = cmd.output().expect("run simulate");

    let argv_log = std::fs::read_to_string(&log).unwrap_or_default();
    let lines: Vec<&str> = argv_log.lines().collect();

    assert!(!lines.is_empty(), "fake stellar was never invoked");
    for line in &lines {
        assert!(
            !line.contains("tx send"),
            "submission command invoked: {line}"
        );
        assert!(
            !line.contains("keys fund") && !line.contains("keys generate"),
            "funding or key generation invoked: {line}"
        );
        assert!(
            !line.contains("contract deploy"),
            "contract deployment invoked: {line}"
        );
    }

    // The mutating step must be skipped, so only the simulate_invoke step
    // should have produced any subprocess calls.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().unwrap();
    assert_eq!(parsed.get("skipped").and_then(|v| v.as_u64()), Some(1));
    assert_eq!(
        parsed.get("submitted").and_then(|v| v.as_bool()),
        Some(false)
    );
}

#[test]
fn simulate_result_never_contains_secret_material() {
    if !stellar_available() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let output = simulate_cmd(&fixture("valid_simulate_invoke.yml"))
        .output()
        .expect("run simulate");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    for token in combined.split(|c: char| !c.is_ascii_alphanumeric()) {
        assert!(
            !(token.len() == 56 && token.starts_with('S')),
            "simulate output leaked a secret-looking token: {token}"
        );
    }
}

#[test]
fn simulate_text_output_goes_to_stderr() {
    if !stellar_available() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    simulate_cmd(&fixture("valid_simulate_invoke.yml"))
        .arg("--output")
        .arg("text")
        .assert()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("not submitted"));
}

// -------------------------------------------------- live-network behaviour

/// The native Stellar Asset Contract on testnet. Deployed by the network
/// itself, so it always exists and needs no funding to read.
const NATIVE_SAC_TESTNET: &str = "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC";

#[test]
fn simulate_live_native_sac_read() {
    if !stellar_available() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    // Uses only a pre-existing funded testnet account and the native SAC.
    // No funding, no account creation, no submission.
    let raw = format!(
        r#"schema_version: "1.0"
name: live_sim_probe
network: testnet
source: GDIPVO6M4SADGLNFTIMYHCTMMOXNONMV4ORZOMP6OAP2DPPYRQBMAW26
timeout_seconds: 60
steps:
  - action: simulate_invoke
    contract_id: {NATIVE_SAC_TESTNET}
    function: balance
    args: ["--id", "GDIPVO6M4SADGLNFTIMYHCTMMOXNONMV4ORZOMP6OAP2DPPYRQBMAW26"]
"#
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("live_sim.yml");
    std::fs::write(&path, raw).expect("write scenario");

    let output = simulate_cmd(path.to_str().unwrap())
        .output()
        .expect("run simulate");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().unwrap();
    let results = parsed.get("results").unwrap().as_array().unwrap();
    assert_eq!(results.len(), 1);

    let status = results[0].get("status").and_then(|v| v.as_str()).unwrap();
    if status != "simulated" {
        let diag = results[0]
            .get("diagnostic")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        eprintln!("skipping: live simulation unavailable on this host: {diag}");
        return;
    }

    assert_eq!(
        results[0].get("submitted").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(
        results[0].get("simulated").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert!(results[0].get("envelope_sha256").is_some());
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn simulate_live_missing_contract_fails() {
    if !stellar_available() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let raw = r#"schema_version: "1.0"
name: live_sim_missing
network: testnet
source: GDIPVO6M4SADGLNFTIMYHCTMMOXNONMV4ORZOMP6OAP2DPPYRQBMAW26
steps:
  - action: simulate_invoke
    contract_id: CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC
    function: balance
    args: ["--id", "GDIPVO6M4SADGLNFTIMYHCTMMOXNONMV4ORZOMP6OAP2DPPYRQBMAW26"]
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("live_sim_missing.yml");
    std::fs::write(&path, raw).expect("write scenario");

    let output = simulate_cmd(path.to_str().unwrap())
        .output()
        .expect("run simulate");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().unwrap();
    let results = parsed.get("results").unwrap().as_array().unwrap();

    let status = results[0].get("status").and_then(|v| v.as_str()).unwrap();
    assert_eq!(status, "build_failed");
    assert_eq!(output.status.code(), Some(2));
}
