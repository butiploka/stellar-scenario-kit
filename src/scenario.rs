//! Scenario file model and validation for the Stellar Scenario Kit.
//!
//! A scenario is a versioned, declarative description of a repeatable
//! Stellar/Soroban development flow. This module owns the on-disk format,
//! parsing, and validation. Execution lives in a separate module and is
//! intentionally not part of this file.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{ScenarioError, ValidationIssue};

/// Current schema version. Bump this when the format changes.
pub const SCHEMA_VERSION: &str = "1.0";

/// A parsed scenario file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    /// Format version. Required; must equal [`SCHEMA_VERSION`].
    pub schema_version: String,
    /// Human-readable scenario name. Used in reports and error messages.
    pub name: String,
    /// Optional longer description.
    #[serde(default)]
    pub description: Option<String>,
    /// Network profile name (e.g. `testnet`, `local`, `futurenet`).
    pub network: String,
    /// Default identity used by steps that do not specify one.
    #[serde(default)]
    pub source: Option<String>,
    /// Ordered list of steps to run.
    pub steps: Vec<Step>,
}

/// A single step in a scenario.
#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    /// Action identifier. See [`Action`] for supported values.
    pub action: String,
    /// Optional human-readable label for this step.
    #[serde(default)]
    pub label: Option<String>,
    /// Action-specific parameters.
    #[serde(flatten)]
    pub params: BTreeMap<String, serde_yaml::Value>,
}

impl Scenario {
    /// Parse and validate a scenario from a file path.
    pub fn from_file(path: &Path) -> Result<Self, ScenarioError> {
        let raw = std::fs::read_to_string(path).map_err(|e| ScenarioError::Io {
            path: path.display().to_string(),
            source: e,
        })?;
        Self::from_str(&raw)
    }

    /// Parse and validate a scenario from a YAML string.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(raw: &str) -> Result<Self, ScenarioError> {
        // Reject unknown top-level fields rather than silently dropping them.
        let scenario: Scenario = serde_yaml::from_str(raw).map_err(|e| {
            let line = e
                .location()
                .map(|loc| loc.line())
                .unwrap_or_else(|| line_of(raw, "schema_version").unwrap_or(1));
            ScenarioError::Parse {
                line: Some(line),
                message: e.to_string(),
            }
        })?;
        scenario.validate()?;
        Ok(scenario)
    }

    /// Run semantic validation on a parsed scenario.
    pub fn validate(&self) -> Result<(), ScenarioError> {
        let mut issues: Vec<ValidationIssue> = Vec::new();

        if self.schema_version != SCHEMA_VERSION {
            issues.push(ValidationIssue {
                field: "schema_version".into(),
                message: format!(
                    "unsupported schema_version '{}'; this build supports '{}'",
                    self.schema_version, SCHEMA_VERSION
                ),
                line: None,
            });
        }

        if self.name.trim().is_empty() {
            issues.push(ValidationIssue {
                field: "name".into(),
                message: "name must not be empty".into(),
                line: None,
            });
        }

        if self.network.trim().is_empty() {
            issues.push(ValidationIssue {
                field: "network".into(),
                message: "network must not be empty".into(),
                line: None,
            });
        }

        if self.steps.is_empty() {
            issues.push(ValidationIssue {
                field: "steps".into(),
                message: "steps must contain at least one step".into(),
                line: None,
            });
        }

        for (idx, step) in self.steps.iter().enumerate() {
            let field = format!("steps[{idx}].action");
            match step.action.as_str() {
                "ensure_funded" => validate_ensure_funded(step, idx, &mut issues),
                "payment" => validate_payment(step, idx, &mut issues),
                "assert_balance" => validate_assert_balance(step, idx, &mut issues),
                other => issues.push(ValidationIssue {
                    field,
                    message: format!(
                        "unknown action '{other}'; supported actions: ensure_funded, payment, assert_balance"
                    ),
                    line: None,
                }),
            }
        }

        if issues.is_empty() {
            Ok(())
        } else {
            Err(ScenarioError::Invalid { issues })
        }
    }
}

/// Supported action identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    EnsureFunded,
    Payment,
    AssertBalance,
}

impl Action {
    /// Parse an action identifier.
    #[allow(clippy::should_implement_trait)]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "ensure_funded" => Some(Self::EnsureFunded),
            "payment" => Some(Self::Payment),
            "assert_balance" => Some(Self::AssertBalance),
            _ => None,
        }
    }
}

fn validate_ensure_funded(step: &Step, idx: usize, issues: &mut Vec<ValidationIssue>) {
    require_string(step, idx, "identity", issues);
    if let Some(v) = step.params.get("min_balance") {
        if !is_u64(v) {
            issues.push(ValidationIssue {
                field: format!("steps[{idx}].min_balance"),
                message: "min_balance must be a non-negative integer".into(),
                line: None,
            });
        }
    }
}

fn validate_payment(step: &Step, idx: usize, issues: &mut Vec<ValidationIssue>) {
    require_string(step, idx, "from", issues);
    require_string(step, idx, "to", issues);
    require_string(step, idx, "amount", issues);
    // asset defaults to native XLM when omitted.
    if let Some(v) = step.params.get("asset") {
        if !v.is_string() {
            issues.push(ValidationIssue {
                field: format!("steps[{idx}].asset"),
                message: "asset must be a string (e.g. 'native' or 'USDC:GA...')".into(),
                line: None,
            });
        }
    }
}

fn validate_assert_balance(step: &Step, idx: usize, issues: &mut Vec<ValidationIssue>) {
    require_string(step, idx, "identity", issues);
    if let Some(v) = step.params.get("min_balance") {
        if !is_non_negative_integer(v) {
            issues.push(ValidationIssue {
                field: format!("steps[{idx}].min_balance"),
                message: "min_balance must be a non-negative integer, or a string of digits".into(),
                line: None,
            });
        }
    }
    if let Some(v) = step.params.get("asset") {
        if !v.is_string() {
            issues.push(ValidationIssue {
                field: format!("steps[{idx}].asset"),
                message: "asset must be a string (e.g. 'native' or 'CODE:ISSUER')".into(),
                line: None,
            });
        }
    }
}

fn require_string(step: &Step, idx: usize, key: &str, issues: &mut Vec<ValidationIssue>) {
    match step.params.get(key) {
        None => issues.push(ValidationIssue {
            field: format!("steps[{idx}].{key}"),
            message: format!("required field '{key}' is missing"),
            line: None,
        }),
        Some(v) if !v.is_string() => issues.push(ValidationIssue {
            field: format!("steps[{idx}].{key}"),
            message: format!("field '{key}' must be a string"),
            line: None,
        }),
        Some(v) if v.as_str().unwrap_or("").trim().is_empty() => issues.push(ValidationIssue {
            field: format!("steps[{idx}].{key}"),
            message: format!("field '{key}' must not be empty"),
            line: None,
        }),
        _ => {}
    }
}

fn is_u64(v: &serde_yaml::Value) -> bool {
    v.as_u64().is_some()
}

/// Accept a non-negative integer as either a YAML integer or a quoted string
/// of digits. Balances are compared as integers, and a quoted string keeps
/// large values from being mangled by YAML number parsing.
fn is_non_negative_integer(v: &serde_yaml::Value) -> bool {
    if let Some(s) = v.as_str() {
        return !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    }
    is_u64(v)
}

/// Find the 1-based line number of a top-level key in raw YAML text.
fn line_of(raw: &str, key: &str) -> Option<usize> {
    raw.lines()
        .position(|l| l.trim_start().starts_with(&format!("{key}:")))
        .map(|p| p + 1)
}
