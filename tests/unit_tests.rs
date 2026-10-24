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
