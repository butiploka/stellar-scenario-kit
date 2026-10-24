//! Deterministic JSON and text reporting for scenario validation.

use serde::Serialize;
use std::path::Path;

use crate::error::{ScenarioError, ValidationIssue};
use crate::scenario::{Scenario, SCHEMA_VERSION};

/// Result of validating a scenario file.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    /// Schema version of the report format itself.
    pub schema_version: String,
    /// `"valid"` or `"invalid"`. No other values are produced.
    pub result: String,
    /// Scenario name, when the file parsed far enough to read it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scenario: Option<String>,
    /// Number of steps declared, when the file parsed far enough to read it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<usize>,
    /// One-line summary of the failure. Present only when `result` is `"invalid"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Structured issues. Empty when `result` is `"valid"`.
    pub errors: Vec<ValidationIssue>,
    /// File path that was validated.
    pub file: String,
}

impl ValidationReport {
    /// Build a report for a successfully validated scenario.
    pub fn valid(path: &Path, scenario: &Scenario) -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            result: "valid".to_string(),
            scenario: Some(scenario.name.clone()),
            steps: Some(scenario.steps.len()),
            error: None,
            errors: Vec::new(),
            file: path.display().to_string(),
        }
    }

    /// Build a report for a failed validation.
    pub fn invalid(path: &Path, err: &ScenarioError) -> Self {
        let mut errors = err.issues().to_vec();
        if errors.is_empty() {
            // Parse and I/O failures carry no field-level issues. Surface the
            // message as a single issue so consumers never get an empty list
            // with an "invalid" result.
            errors.push(ValidationIssue {
                field: "file".to_string(),
                message: err.to_string(),
                line: err.line(),
            });
        }

        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            result: "invalid".to_string(),
            scenario: None,
            steps: None,
            error: Some(err.summary()),
            errors,
            file: path.display().to_string(),
        }
    }

    /// Serialize to pretty-printed JSON with a trailing newline.
    pub fn to_json(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        s.push('\n');
        s
    }

    /// Serialize to a compact human-readable summary.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("{}: {}\n", self.file, self.result));
        if let Some(name) = &self.scenario {
            out.push_str(&format!("  scenario: {name}\n"));
        }
        if let Some(steps) = self.steps {
            out.push_str(&format!("  steps: {steps}\n"));
        }
        for issue in &self.errors {
            match issue.line {
                Some(line) => out.push_str(&format!(
                    "  {}:{}: {} ({})\n",
                    self.file, line, issue.message, issue.field
                )),
                None => out.push_str(&format!("  {}: {}\n", issue.field, issue.message)),
            }
        }
        out
    }
}
