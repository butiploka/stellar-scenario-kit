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
