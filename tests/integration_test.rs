//! Integration tests for the `stellar-scenario validate` CLI.
//!
//! These tests exercise the compiled binary, not the library, so that the
//! argument parsing, exit codes, and output streams are all covered.
//!
//! Exit code contract under test:
//!   0 — valid
//!   1 — semantic validation failure
//!   2 — I/O or parse failure

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;
use std::process::Command as StdCommand;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn build_bin() -> PathBuf {
    let mut bin = project_root();
    bin.push("target/debug/stellar-scenario");
    if !bin.exists() {
        let mut cargo = StdCommand::new(env!("CARGO"));
        cargo.current_dir(project_root());
        cargo.args(["build", "--bin", "stellar-scenario"]);
        let output = cargo.output().expect("cargo build failed");
        assert!(output.status.success(), "build failed: {output:?}");
    }
    bin
}

fn fixture(name: &str) -> String {
    let mut path = project_root();
    path.push("tests/scenarios");
    path.push(name);
    path.to_str().expect("fixture path").to_string()
}

fn example(name: &str) -> String {
    let mut path = project_root();
    path.push("examples");
    path.push(name);
    path.to_str().expect("example path").to_string()
}

fn validate_cmd(path: &str) -> Command {
    let mut cmd = Command::new(build_bin());
    cmd.arg("validate").arg(path);
    cmd
}

// ---------------------------------------------------------------- valid files

#[test]
fn valid_payment_scenario_exits_zero() {
    validate_cmd(&fixture("valid_payment.yml"))
        .assert()
        .success()
        .stdout(predicate::str::contains("\"result\": \"valid\""));
}

#[test]
fn valid_minimal_scenario_exits_zero() {
    validate_cmd(&fixture("valid_minimal.yml"))
        .assert()
        .success()
        .stdout(predicate::str::contains("\"result\": \"valid\""));
}

#[test]
fn example_payment_scenario_validates() {
    validate_cmd(&example("payment.yml"))
        .assert()
        .success()
        .stdout(predicate::str::contains("\"result\": \"valid\""))
        .stdout(predicate::str::contains("\"steps\": 3"));
}

#[test]
fn example_auth_setup_scenario_validates() {
    validate_cmd(&example("auth_setup.yml"))
        .assert()
        .success()
        .stdout(predicate::str::contains("\"result\": \"valid\""));
}

// ------------------------------------------------- semantic failures (exit 1)

#[test]
fn unsupported_version_exits_one() {
    validate_cmd(&fixture("invalid_unsupported_version.yml"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("unsupported schema_version"));
}

#[test]
fn unknown_action_exits_one() {
    validate_cmd(&fixture("invalid_unknown_action.yml"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("unknown action"));
}

#[test]
fn missing_payment_field_exits_one() {
    validate_cmd(&fixture("invalid_missing_payment_field.yml"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains(
            "required field 'amount' is missing",
        ))
        .stdout(predicate::str::contains("steps[0].amount"));
}

#[test]
fn missing_payment_source_field_exits_one() {
    validate_cmd(&fixture("invalid_missing_payment_source.yml"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("required field 'from'"))
        .stdout(predicate::str::contains("steps[0].from"));
}

#[test]
fn bad_min_balance_type_exits_one() {
    validate_cmd(&fixture("invalid_bad_min_balance_type.yml"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains(
            "min_balance must be a non-negative integer",
        ));
}

#[test]
fn empty_steps_exits_one() {
    validate_cmd(&fixture("invalid_empty_steps.yml"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains(
            "steps must contain at least one step",
        ));
}

#[test]
fn empty_name_exits_one() {
    validate_cmd(&fixture("invalid_empty_name.yml"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("name must not be empty"));
}

// -------------------------------------------------- parse/IO failures (exit 2)

#[test]
fn missing_schema_version_exits_two() {
    validate_cmd(&fixture("invalid_missing_schema_version.yml"))
        .assert()
        .failure()
        .code(2)
        .stdout(predicate::str::contains("\"result\": \"invalid\""))
        .stdout(predicate::str::contains("schema_version"));
}

#[test]
fn unknown_top_level_field_exits_two() {
    validate_cmd(&fixture("invalid_unknown_field.yml"))
        .assert()
        .failure()
        .code(2)
        .stdout(predicate::str::contains("unexpected_field"));
}

#[test]
fn malformed_yaml_exits_two() {
    validate_cmd(&fixture("invalid_malformed_yaml.yml"))
        .assert()
        .failure()
        .code(2)
        .stdout(predicate::str::contains("\"result\": \"invalid\""));
}

#[test]
fn missing_file_exits_two() {
    validate_cmd("/tmp/stellar-scenario-does-not-exist-12345.yml")
        .assert()
        .failure()
        .code(2)
        .stdout(predicate::str::contains("cannot read"));
}

// ------------------------------------------------------------- output formats

#[test]
fn output_text_writes_to_stderr() {
    validate_cmd(&fixture("invalid_unsupported_version.yml"))
        .arg("--output")
        .arg("text")
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("invalid"))
        .stderr(predicate::str::contains("schema_version"));
}

#[test]
fn json_output_is_parseable() {
    let output = validate_cmd(&fixture("invalid_unsupported_version.yml"))
        .output()
        .expect("run validate");
    assert_eq!(output.status.code(), Some(1));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().expect("stdout is valid JSON");
    assert_eq!(
        parsed.get("result").and_then(|v| v.as_str()),
        Some("invalid")
    );
    assert_eq!(
        parsed.get("schema_version").and_then(|v| v.as_str()),
        Some("1.0")
    );
    assert_eq!(
        parsed
            .get("errors")
            .and_then(|v| v.as_array())
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn json_output_for_parse_error_is_parseable() {
    let output = validate_cmd(&fixture("invalid_missing_schema_version.yml"))
        .output()
        .expect("run validate");
    assert_eq!(output.status.code(), Some(2));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = stdout.parse().expect("stdout is valid JSON");
    assert_eq!(
        parsed.get("result").and_then(|v| v.as_str()),
        Some("invalid")
    );
    assert!(
        !parsed
            .get("errors")
            .and_then(|v| v.as_array())
            .expect("errors array")
            .is_empty(),
        "parse errors must still produce at least one structured issue"
    );
}

// ---------------------------------------------------------- fixture integrity

/// Every `examples/*.yml` must validate. Every `tests/scenarios/valid_*.yml`
/// must validate. Every `tests/scenarios/invalid_*.yml` must NOT validate.
///
/// This is the guard that stops a broken fixture from silently entering the
/// repository and weakening every other test.
#[test]
fn fixture_integrity() {
    let root = project_root();
    let mut checked_valid = 0usize;
    let mut checked_invalid = 0usize;

    for dir in [root.join("examples"), root.join("tests/scenarios")] {
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(&dir).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.extension().map(|e| e == "yml").unwrap_or(false) {
                let name = path.file_name().unwrap().to_str().unwrap().to_string();
                let raw = fs::read_to_string(&path).expect("read fixture");
                let result = stellar_scenario::scenario::Scenario::from_str(&raw);

                let should_be_valid = !name.starts_with("invalid_");
                if should_be_valid {
                    assert!(
                        result.is_ok(),
                        "fixture {name} should be valid but failed: {result:?}"
                    );
                    checked_valid += 1;
                } else {
                    assert!(
                        result.is_err(),
                        "fixture {name} should be invalid but parsed successfully"
                    );
                    checked_invalid += 1;
                }
            }
        }
    }

    assert!(
        checked_valid >= 4,
        "expected at least 4 valid fixtures, checked {checked_valid}"
    );
    assert!(
        checked_invalid >= 7,
        "expected at least 7 invalid fixtures, checked {checked_invalid}"
    );
}
