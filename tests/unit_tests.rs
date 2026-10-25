//! Unit tests for scenario parsing and validation.

use stellar_scenario::error::ScenarioError;
use stellar_scenario::scenario::{Scenario, SCHEMA_VERSION};

const VALID: &str = r#"
schema_version: "1.0"
name: test
network: testnet
steps:
  - action: ensure_funded
    identity: alice
"#;

#[test]
fn parses_valid_scenario() {
    let s = Scenario::from_str(VALID).expect("valid scenario");
    assert_eq!(s.name, "test");
    assert_eq!(s.steps.len(), 1);
    assert_eq!(s.schema_version, SCHEMA_VERSION);
}

#[test]
fn missing_schema_version_is_rejected() {
    let raw = "name: x\nnetwork: testnet\nsteps:\n  - action: ensure_funded\n    identity: a\n";
    let err = Scenario::from_str(raw).unwrap_err();
    assert!(matches!(err, ScenarioError::Parse { .. }));
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn unsupported_schema_version_is_rejected() {
    let raw = VALID.replace("\"1.0\"", "\"2.0\"");
    let err = Scenario::from_str(&raw).unwrap_err();
    match &err {
        ScenarioError::Invalid { issues } => {
            assert!(issues.iter().any(|i| i.field == "schema_version"));
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert_eq!(err.exit_code(), 1);
}

#[test]
fn unknown_top_level_field_is_rejected() {
    let raw = VALID.replace("name: test", "name: test\nbogus: 1");
    let err = Scenario::from_str(&raw).unwrap_err();
    assert!(matches!(err, ScenarioError::Parse { .. }));
}

#[test]
fn unknown_action_is_rejected() {
    let raw = VALID.replace("ensure_funded", "teleport");
    match Scenario::from_str(&raw).unwrap_err() {
        ScenarioError::Invalid { issues } => {
            assert!(issues.iter().any(|i| i.message.contains("unknown action")));
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn missing_required_step_field_is_rejected() {
    let raw = VALID.replace("    identity: alice\n", "");
    match Scenario::from_str(&raw).unwrap_err() {
        ScenarioError::Invalid { issues } => {
            assert!(issues.iter().any(|i| i.field == "steps[0].identity"));
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn empty_steps_is_rejected() {
    let raw = VALID.replace(
        "steps:\n  - action: ensure_funded\n    identity: alice\n",
        "steps: []\n",
    );
    match Scenario::from_str(&raw).unwrap_err() {
        ScenarioError::Invalid { issues } => {
            assert!(issues.iter().any(|i| i.field == "steps"));
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn empty_name_is_rejected() {
    let raw = VALID.replace("name: test", "name: \"\"");
    match Scenario::from_str(&raw).unwrap_err() {
        ScenarioError::Invalid { issues } => {
            assert!(issues.iter().any(|i| i.field == "name"));
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn bad_min_balance_type_is_rejected() {
    let raw = VALID.replace("identity: alice", "identity: alice\n    min_balance: nope");
    match Scenario::from_str(&raw).unwrap_err() {
        ScenarioError::Invalid { issues } => {
            assert!(issues.iter().any(|i| i.field == "steps[0].min_balance"));
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn malformed_yaml_reports_line_number() {
    let raw = "schema_version: \"1.0\"\nname: x\nnetwork: t\nsteps:\n  - action: [unclosed\n";
    match Scenario::from_str(raw).unwrap_err() {
        ScenarioError::Parse { line, .. } => {
            assert!(line.is_some(), "expected a line number");
        }
        other => panic!("expected Parse, got {other:?}"),
    }
}

#[test]
fn action_parser_recognises_known_actions() {
    use stellar_scenario::scenario::Action;
    assert_eq!(Action::parse("ensure_funded"), Some(Action::EnsureFunded));
    assert_eq!(Action::parse("payment"), Some(Action::Payment));
    assert_eq!(Action::parse("assert_balance"), Some(Action::AssertBalance));
    assert_eq!(Action::parse("nope"), None);
}

// ------------------------------------------------------- input robustness

/// Every malformed input must produce a `ScenarioError`, never a panic and
/// never a silently-accepted scenario.
#[test]
fn malformed_inputs_are_rejected_not_panicked() {
    let cases: &[(&str, &str)] = &[
        ("empty document", ""),
        ("only a comment", "# nothing here\n"),
        ("only whitespace", "   \n\t\n"),
        ("root sequence", "- a\n- b\n"),
        ("root scalar", "just a string\n"),
        ("root null", "null\n"),
        ("root integer", "42\n"),
        ("multiple documents", "schema_version: \"1.0\"\nname: a\nnetwork: t\nsteps:\n  - action: assert_asset\n    identity: x\n---\nname: b\n"),
        ("unclosed flow sequence", "schema_version: \"1.0\"\nname: a\nnetwork: t\nsteps:\n  - action: [unclosed\n"),
        ("tab indentation", "schema_version: \"1.0\"\nname: a\nnetwork: t\nsteps:\n\t- action: assert_asset\n"),
        ("duplicate keys", "schema_version: \"1.0\"\nschema_version: \"2.0\"\nname: a\nnetwork: t\nsteps:\n  - action: assert_asset\n    identity: x\n"),
        ("anchor bomb", "a: &x [1,2,3]\nb: *x\n"),
    ];

    for (label, raw) in cases {
        let result = std::panic::catch_unwind(|| Scenario::from_str(raw));
        match result {
            Ok(Err(_)) => {} // rejected: correct
            Ok(Ok(_)) => panic!("{label}: input was accepted but should be rejected"),
            Err(_) => panic!("{label}: input caused a panic"),
        }
    }
}

/// Extreme numeric values must not panic or silently coerce.
#[test]
fn extreme_numeric_values_are_handled() {
    let cases: &[(&str, &str)] = &[
        ("timeout u64 max", "timeout_seconds: 18446744073709551615"),
        ("timeout negative", "timeout_seconds: -1"),
        ("timeout zero", "timeout_seconds: 0"),
        (
            "min_balance over u128",
            "min_balance: \"340282366920938463463374607431768211456\"",
        ),
        (
            "min_balance u128 max",
            "min_balance: \"340282366920938463463374607431768211455\"",
        ),
        ("min_balance float", "min_balance: \"1.5\""),
        ("min_balance negative", "min_balance: \"-1\""),
        ("min_balance empty", "min_balance: \"\""),
        ("min_balance hex", "min_balance: \"0x10\""),
    ];

    for (label, field) in cases {
        let raw = format!(
            "schema_version: \"1.0\"\nname: a\nnetwork: t\nsteps:\n  - action: assert_balance\n    identity: x\n    {field}\n"
        );
        let result = std::panic::catch_unwind(|| Scenario::from_str(&raw));
        match result {
            Ok(_) => {} // either accepted or rejected; both are fine
            Err(_) => panic!("{label}: caused a panic"),
        }
    }
}

/// Identifiers with spaces, quotes, dashes, and newlines must survive the
/// round trip. They are passed to the CLI as separate argv entries, so
/// metacharacters are data, not syntax — but they must not corrupt parsing.
#[test]
fn odd_identifier_characters_survive_parsing() {
    let odd = [
        "with space",
        "with\"quote",
        "with'squote",
        "--leading-dash",
        "-",
        "--",
        "semi;colon",
        "dollar$sign",
        "back`tick",
        "tab\there",
    ];

    for value in odd {
        let raw = format!(
            "schema_version: \"1.0\"\nname: a\nnetwork: testnet\nsource: {}\nsteps:\n  - action: assert_asset\n    identity: {}\n",
            serde_yaml::to_string(value).unwrap().trim(),
            serde_yaml::to_string(value).unwrap().trim(),
        );
        let scenario =
            Scenario::from_str(&raw).unwrap_or_else(|e| panic!("value {value:?} rejected: {e}"));
        assert_eq!(scenario.source.as_deref(), Some(value));
    }
}

/// A very long identifier must not be truncated or rejected arbitrarily.
#[test]
fn very_long_identifier_is_accepted() {
    let long = "G".repeat(10_000);
    let raw = format!(
        "schema_version: \"1.0\"\nname: a\nnetwork: testnet\nsteps:\n  - action: assert_asset\n    identity: {}\n",
        long
    );
    let scenario = Scenario::from_str(&raw).expect("long identifier should parse");
    assert_eq!(
        scenario.steps[0].params["identity"].as_str(),
        Some(long.as_str())
    );
}

/// Deeply nested YAML must not overflow the stack.
#[test]
fn deeply_nested_yaml_does_not_overflow() {
    // Build `args: [[[[...]]]]` nesting. serde_yaml has a recursion limit and
    // returns an error rather than overflowing, which is the correct outcome.
    let depth = 200;
    let nested = format!(
        "schema_version: \"1.0\"\nname: a\nnetwork: testnet\nsteps:\n  - action: simulate_invoke\n    contract_id: C\n    function: f\n    args: {}\n",
        "[".repeat(depth) + &"]".repeat(depth)
    );
    let result = std::panic::catch_unwind(|| Scenario::from_str(&nested));
    match result {
        Ok(_) => {}
        Err(_) => panic!("deeply nested YAML caused a panic"),
    }
}

/// A large but valid scenario must parse without excessive cost.
#[test]
fn large_scenario_parses() {
    let mut raw = String::from("schema_version: \"1.0\"\nname: big\nnetwork: testnet\nsteps:\n");
    for i in 0..2000 {
        raw.push_str(&format!(
            "  - action: assert_asset\n    identity: acct{i}\n"
        ));
    }
    let scenario = Scenario::from_str(&raw).expect("large scenario should parse");
    assert_eq!(scenario.steps.len(), 2000);
}
