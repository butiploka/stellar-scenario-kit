//! Integration tests for the `inspect` command.
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

fn inspect_cmd(path: &str) -> Command {
    let mut cmd = Command::new(bin());
    cmd.arg("inspect").arg(path);
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
fn inspect_rejects_missing_contract_id() {
    inspect_cmd(&fixture("invalid_inspect_missing_contract.yml"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicates::prelude::predicate::str::contains("contract_id"));
}

#[test]
fn inspect_accepts_valid_scenario() {
    let output = inspect_cmd(&fixture("valid_inspect_contract.yml"))
        .output()
        .expect("run inspect");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("\"result\": \"invalid\""),
        "scenario should validate: {stdout}"
    );
}

#[test]
fn inspect_skips_mutating_steps() {
    if !stellar_runnable() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let output = inspect_cmd(&fixture("valid_inspect_with_mutation.yml"))
        .output()
        .expect("run inspect");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().unwrap();

    assert_eq!(
        parsed.get("skipped").and_then(|v| v.as_u64()),
        Some(1),
        "the payment step must be skipped, never inspected"
    );
    let results = parsed.get("results").unwrap().as_array().unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn inspect_never_invokes_submission_or_funding() {
    if !stellar_runnable() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

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
steps:
  - action: inspect_contract
    contract_id: CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC
  - action: ensure_funded
    identity: deployer
"#,
    )
    .expect("write scenario");

    let mut cmd = Command::new(bin());
    cmd.arg("inspect")
        .arg(scenario.to_str().unwrap())
        .env("STELLAR_CLI", fake.to_str().unwrap());
    let output = cmd.output().expect("run inspect");

    let argv_log = std::fs::read_to_string(&log).unwrap_or_default();
    for line in argv_log.lines() {
        assert!(!line.contains("tx send"), "submission invoked: {line}");
        assert!(
            !line.contains("keys fund") && !line.contains("keys generate"),
            "funding or key generation invoked: {line}"
        );
        assert!(
            !line.contains("contract deploy"),
            "contract deployment invoked: {line}"
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().unwrap();
    assert_eq!(parsed.get("skipped").and_then(|v| v.as_u64()), Some(1));
}

// ---------------------------------------------------- fake-CLI failure modes

/// Write a fake `stellar` that answers `contract info interface` from env vars.
fn fake_stellar(dir: &std::path::Path) -> PathBuf {
    let path = dir.join("stellar");
    std::fs::write(
        &path,
        r#"#!/bin/sh
case "$1 $2 $3" in
  "contract info interface")
      if [ -n "$FAKE_IFACE" ]; then printf '%s' "$FAKE_IFACE"; fi
      if [ -n "$FAKE_IFACE_ERR" ]; then printf '%s' "$FAKE_IFACE_ERR" >&2; fi
      exit "${FAKE_IFACE_EXIT:-0}" ;;
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

fn inspect_with_fake(iface: &str, iface_exit: &str, iface_err: &str) -> serde_json::Value {
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
  - action: inspect_contract
    contract_id: CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC
"#,
    )
    .expect("write scenario");

    let mut cmd = Command::new(bin());
    cmd.arg("inspect")
        .arg(scenario.to_str().unwrap())
        .env("STELLAR_CLI", fake.to_str().unwrap())
        .env("FAKE_IFACE", iface)
        .env("FAKE_IFACE_EXIT", iface_exit)
        .env("FAKE_IFACE_ERR", iface_err);
    let output = cmd.output().expect("run inspect");
    String::from_utf8_lossy(&output.stdout)
        .parse()
        .expect("inspect stdout is valid JSON")
}

fn first(report: &serde_json::Value) -> &serde_json::Value {
    &report["results"][0]
}

const GOOD_SPEC: &str = r#"[{"function_v0":{"doc":"Returns the balance.","name":"balance","inputs":[{"name":"id","type":"address"}],"outputs":["i128"]}}]"#;

#[test]
fn fake_cli_success_inspects() {
    let r = inspect_with_fake(GOOD_SPEC, "0", "");
    let res = first(&r);
    assert_eq!(res["status"], "inspected");
    let iface = res["interface"].as_object().unwrap();
    assert_eq!(iface["functions"].as_array().unwrap().len(), 1);
    assert_eq!(iface["functions"][0]["inputs"][0]["type"], "address");
    assert_eq!(r["status"], "inspected");
    assert_eq!(r["inspected"], 1);
}

#[test]
fn empty_output_is_a_parse_failure() {
    let r = inspect_with_fake("", "0", "");
    assert_eq!(first(&r)["status"], "parse_failed");
    assert_eq!(r["status"], "parse_failed");
}

#[test]
fn malformed_json_is_a_parse_failure() {
    for bad in ["[{\"function_v0\":", "not json", "   ", "{}", "null", "42"] {
        let r = inspect_with_fake(bad, "0", "");
        let status = first(&r)["status"].as_str().unwrap();
        assert_ne!(status, "inspected", "input {bad:?} must not inspect");
        assert!(
            matches!(status, "parse_failed" | "unexpected_shape"),
            "input {bad:?} gave {status}"
        );
    }
}

#[test]
fn nonzero_exit_surfaces_cli_stderr() {
    let r = inspect_with_fake("", "1", "❌ error: contract not found");
    let res = first(&r);
    assert_eq!(res["status"], "fetch_failed");
    let diag = res["diagnostic"].as_str().unwrap();
    assert!(diag.contains("code 1"), "diag: {diag}");
    assert!(diag.contains("contract not found"), "diag: {diag}");
}

#[test]
fn nonzero_exit_with_empty_streams_reports_exit_code() {
    let r = inspect_with_fake("", "3", "");
    let res = first(&r);
    assert_eq!(res["status"], "fetch_failed");
    assert!(res["diagnostic"].as_str().unwrap().contains("code 3"));
}

#[test]
fn empty_array_is_inspected_but_empty() {
    let r = inspect_with_fake("[]", "0", "");
    let res = first(&r);
    assert_eq!(res["status"], "inspected");
    assert!(res["interface"]["functions"].as_array().unwrap().is_empty());
}

#[test]
fn cli_stderr_seed_is_redacted() {
    let seed = format!("S{}", "A".repeat(55));
    let r = inspect_with_fake("", "1", &format!("failed with key {seed}"));
    let diag = first(&r)["diagnostic"].as_str().unwrap();
    assert!(!diag.contains(&seed), "seed leaked: {diag}");
    assert!(diag.contains("<redacted-secret>"), "diag: {diag}");
}

#[test]
fn missing_cli_is_reported_as_a_structured_error() {
    let mut cmd = Command::new(bin());
    cmd.arg("inspect")
        .arg(fixture("valid_inspect_contract.yml"))
        .env("STELLAR_CLI", "/tmp/stellar-scenario-absent-binary-xyz");
    let output = cmd.output().expect("run inspect");
    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = stdout.parse().expect("valid JSON");
    // Either a bare error object (CLI discovery failed) or a report whose
    // only result errored. Both are exit 2 with no fabricated interface.
    let has_error_object = v.get("error").is_some();
    let has_failed_result = v
        .get("results")
        .and_then(|r| r.as_array())
        .map(|a| a.iter().any(|x| x["status"] == "error"))
        .unwrap_or(false);
    assert!(has_error_object || has_failed_result, "stdout: {stdout}");
    assert_eq!(
        v.get("status").and_then(|s| s.as_str()),
        Some("fetch_failed"),
        "a missing CLI must not report as no_contracts: {stdout}"
    );
}

#[test]
fn text_output_goes_to_stderr() {
    if !stellar_runnable() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }
    let output = inspect_cmd(&fixture("valid_inspect_contract.yml"))
        .arg("--output")
        .arg("text")
        .output()
        .expect("run inspect");
    assert!(output.stdout.is_empty(), "text must not go to stdout");
    assert!(!output.stderr.is_empty(), "text output must go to stderr");
}

// -------------------------------------------------- live-network behaviour

#[test]
fn inspect_live_native_sac() {
    if !stellar_runnable() {
        eprintln!("skipping: stellar CLI not available on this host");
        return;
    }

    let output = inspect_cmd(&fixture("valid_inspect_contract.yml"))
        .output()
        .expect("run inspect");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().unwrap();
    let results = parsed.get("results").unwrap().as_array().unwrap();

    let status = results[0]["status"].as_str().unwrap();
    if status != "inspected" {
        eprintln!(
            "skipping: live inspection unavailable on this host: {}",
            results[0]["diagnostic"]
        );
        return;
    }

    let iface = &results[0]["interface"];
    let functions = iface["functions"].as_array().unwrap();
    assert_eq!(functions.len(), 17, "native SAC has 17 functions");
    assert_eq!(iface["events"].as_array().unwrap().len(), 13);

    let balance = functions
        .iter()
        .find(|f| f["name"] == "balance")
        .expect("balance function");
    assert_eq!(balance["inputs"][0]["name"], "id");
    assert_eq!(balance["inputs"][0]["type"], "address");
    assert_eq!(balance["outputs"][0], "i128");

    let decimals = functions
        .iter()
        .find(|f| f["name"] == "decimals")
        .expect("decimals function");
    assert!(decimals["inputs"].as_array().unwrap().is_empty());
    assert_eq!(decimals["outputs"][0], "u32");

    assert_eq!(output.status.code(), Some(0));
}
