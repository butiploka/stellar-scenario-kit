//! Error types with actionable, file-and-field-level diagnostics.

use std::io;

use thiserror::Error;

/// A single validation problem found in a scenario file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ValidationIssue {
    /// Dotted/indexed path to the offending field, e.g. `steps[0].amount`.
    pub field: String,
    /// Human-readable explanation of what is wrong.
    pub message: String,
    /// 1-based line number in the source file, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

/// Errors produced while loading or validating a scenario.
#[derive(Debug, Error)]
pub enum ScenarioError {
    /// The file could not be read.
    #[error("cannot read '{path}': {source}")]
    Io {
        /// Path that failed to read.
        path: String,
        /// Underlying OS error.
        #[source]
        source: io::Error,
    },

    /// The file is not valid YAML for this schema.
    #[error("{}", .message)]
    Parse {
        /// 1-based line number, when the parser reported one.
        line: Option<usize>,
        /// Parser message.
        message: String,
    },

    /// The file parsed but failed semantic validation.
    #[error("scenario failed validation with {} issue(s)", .issues.len())]
    Invalid {
        /// All issues found, in document order.
        issues: Vec<ValidationIssue>,
    },
}

impl ScenarioError {
    /// Exit code for this error class.
    ///
    /// `1` for validation failures, `2` for everything else (I/O, parse,
    /// unexpected). This mapping is part of the documented CLI contract.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Invalid { .. } => 1,
            Self::Io { .. } | Self::Parse { .. } => 2,
        }
    }

    /// Issues attached to this error, if any.
    pub fn issues(&self) -> &[ValidationIssue] {
        match self {
            Self::Invalid { issues } => issues,
            _ => &[],
        }
    }

    /// Line number attached to this error, if any.
    pub fn line(&self) -> Option<usize> {
        match self {
            Self::Parse { line, .. } => *line,
            _ => None,
        }
    }

    /// One-line summary used for the `error` field of JSON output.
    pub fn summary(&self) -> String {
        match self {
            Self::Invalid { issues } => format!("{} validation issue(s)", issues.len()),
            other => other.to_string(),
        }
    }
}
