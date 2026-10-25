//! Balance assertion model, execution, and scenario-level reporting.
//!
//! This module implements the read-only execution capabilities:
//! `assert_balance` and `assert_asset`. Both run `stellar token balance`
//! against a network, parse the result, and report it.
//!
//! # Safety
//!
//! - No transaction is submitted.
//! - No account is funded.
//! - No secret key is accepted as input.
//! - The only subprocess spawned is `stellar token balance`, which is a
//!   read-only RPC call.
//!
//! # What a successful assertion proves
//!
//! A `pass` result proves that, at the moment the RPC read returned, the
//! named account held at least `min_balance` of the named asset on the named
//! network. It does not prove the balance is still that value later, and it
//! is not a transaction confirmation.

use std::str::FromStr;

use crate::runner;
use crate::scenario::{Scenario, Step};

/// What an `assert_balance` or `assert_asset` step asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertionMode {
    /// `assert_balance` — require `actual >= min_balance`.
    MinBalance,
    /// `assert_asset` — require that a balance entry exists at all.
    Exists,
}

impl AssertionMode {
    /// Mode for a given action identifier.
    pub fn for_action(action: &str) -> Option<Self> {
        match action {
            "assert_balance" => Some(Self::MinBalance),
            "assert_asset" => Some(Self::Exists),
            _ => None,
        }
    }
}

/// Result of a single balance assertion.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AssertBalanceResult {
    /// Scenario name.
    pub scenario: String,
    /// Zero-based step index.
    pub step_index: usize,
    /// Action that produced this result (`assert_balance` or `assert_asset`).
    pub action: String,
    /// Which comparison rule was applied.
    pub mode: AssertionMode,
    /// Account that was queried.
    pub account: String,
    /// Network profile used.
    pub network: String,
    /// Asset identifier (`native`, `CODE:ISSUER`, or contract id).
    pub asset: String,
    /// Expected minimum balance in the asset's smallest unit. `None` for
    /// existence-only assertions.
    pub expected_min_balance: Option<String>,
    /// Actual balance returned by the CLI, when the query succeeded.
    pub actual_balance: Option<String>,
    /// Pass/fail/error outcome.
    pub status: AssertStatus,
    /// Human-readable diagnostic. Never contains secret material.
    pub diagnostic: String,
    /// Classified failure, when the cause was proven.
    ///
    /// Omitted on success, and on failures whose cause is not recognized.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic_category: Option<crate::diagnostic::DiagnosticCategory>,
    /// The remediation for [`Self::diagnostic_category`], when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

/// Outcome of a balance assertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertStatus {
    /// The assertion held.
    Pass,
    /// The query succeeded but the assertion did not hold.
    Fail,
    /// The balance query itself failed (CLI error, parse error, timeout).
    Error,
}

/// Context for building an `AssertBalanceResult`.
#[derive(Debug, Clone)]
struct AssertCtx<'a> {
    scenario: &'a Scenario,
    step_index: usize,
    action: &'a str,
    mode: AssertionMode,
    account: String,
    network: String,
    asset: String,
    expected_min_balance: Option<String>,
}

impl<'a> AssertCtx<'a> {
    fn from_step(scenario: &'a Scenario, step_index: usize, step: &'a Step) -> Self {
        let account = step
            .params
            .get("identity")
            .and_then(|v| v.as_str())
            .unwrap_or("<missing>")
            .to_string();
        let asset = step
            .params
            .get("asset")
            .and_then(|v| v.as_str())
            .unwrap_or("native")
            .to_string();
        let mode = AssertionMode::for_action(&step.action).unwrap_or(AssertionMode::MinBalance);
        let expected_min_balance = if mode == AssertionMode::MinBalance {
            Some(
                step.params
                    .get("min_balance")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0")
                    .to_string(),
            )
        } else {
            None
        };

        Self {
            scenario,
            step_index,
            action: step.action.as_str(),
            mode,
            account,
            network: scenario.network.clone(),
            asset,
            expected_min_balance,
        }
    }

    fn pass(self, actual: &str, expected: &str) -> AssertBalanceResult {
        let d = format!("balance {actual} >= expected {expected}");
        self.build(actual, AssertStatus::Pass, &d)
    }

    fn fail(self, actual: &str, expected: &str) -> AssertBalanceResult {
        let d = format!("balance {actual} < expected {expected}");
        self.build(actual, AssertStatus::Fail, &d)
    }

    fn exists_pass(self, actual: &str) -> AssertBalanceResult {
        let d = format!(
            "balance entry exists for {} on {}",
            self.asset, self.account
        );
        self.build(actual, AssertStatus::Pass, &d)
    }

    fn error(self, diagnostic: &str) -> AssertBalanceResult {
        self.error_with(diagnostic, None)
    }

    fn error_with(
        self,
        diagnostic: &str,
        diag: Option<crate::diagnostic::Diagnostic>,
    ) -> AssertBalanceResult {
        let (category, remediation) = match diag {
            Some(d) => (Some(d.category), Some(d.remediation)),
            None => (None, None),
        };
        AssertBalanceResult {
            scenario: self.scenario.name.clone(),
            step_index: self.step_index,
            action: self.action.to_string(),
            mode: self.mode,
            account: self.account,
            network: self.network,
            asset: self.asset,
            expected_min_balance: self.expected_min_balance,
            actual_balance: None,
            status: AssertStatus::Error,
            diagnostic: crate::simulate::redact(diagnostic),
            diagnostic_category: category,
            remediation,
        }
    }

    fn build(self, actual: &str, status: AssertStatus, diagnostic: &str) -> AssertBalanceResult {
        AssertBalanceResult {
            scenario: self.scenario.name.clone(),
            step_index: self.step_index,
            action: self.action.to_string(),
            mode: self.mode,
            account: self.account,
            network: self.network,
            asset: self.asset,
            expected_min_balance: self.expected_min_balance,
            actual_balance: Some(actual.to_string()),
            status,
            diagnostic: crate::simulate::redact(diagnostic),
            diagnostic_category: None,
            remediation: None,
        }
    }
}

impl AssertBalanceResult {
    /// Execute an `assert_balance` or `assert_asset` step.
    ///
    /// Returns a structured result regardless of success or failure. The
    /// caller decides what to do with it; nothing here exits the process.
    pub fn execute(
        scenario: &Scenario,
        step_index: usize,
        step: &Step,
        executable: &std::path::Path,
    ) -> Self {
        let ctx = AssertCtx::from_step(scenario, step_index, step);

        let args = vec![
            std::ffi::OsStr::new("token"),
            std::ffi::OsStr::new("balance"),
            std::ffi::OsStr::new("--id"),
            std::ffi::OsStr::new(&ctx.asset),
            std::ffi::OsStr::new("--account"),
            std::ffi::OsStr::new(&ctx.account),
            std::ffi::OsStr::new("--network"),
            std::ffi::OsStr::new(&scenario.network),
            std::ffi::OsStr::new("--output"),
            std::ffi::OsStr::new("json"),
        ];

        let run_result =
            runner::run_capture(executable.as_os_str(), &args, scenario.timeout_duration());

        match run_result {
            // A non-zero exit is a CLI failure, not a balance. Report it with
            // whatever context the CLI gave, before trying to parse stdout:
            // a failed command may emit nothing, or a partial document, and
            // reading that as "malformed JSON" hides the real error.
            Ok(output) if !output.is_success() => {
                let detail = first_non_empty(&[&output.stderr, &output.stdout])
                    .unwrap_or_else(|| "<no output>".to_string());
                // Classify from the raw CLI message, before we wrap it in the
                // exit-code prefix. The wrapper would push the key text past
                // the `starts_with` checks in the classifier.
                let raw_diag = crate::diagnostic::Diagnostic::classify(&detail);
                let diagnostic = crate::simulate::redact(&format!(
                    "stellar CLI exited with code {}: {detail}",
                    output.exit_code
                ));
                ctx.error_with(&diagnostic, Some(raw_diag))
            }
            Ok(output) => match parse_balance_json(&output.stdout) {
                Ok(actual) => match validate_balance_string(&actual) {
                    Err(e) => ctx.error(&format!("invalid balance value: {e}")),
                    Ok(()) => match ctx.mode {
                        AssertionMode::MinBalance => {
                            let expected = ctx.expected_min_balance.clone().unwrap_or_default();
                            match validate_balance_string(&expected) {
                                Err(e) => ctx.error(&format!("invalid expected value: {e}")),
                                Ok(()) => match compare_balances(&actual, &expected) {
                                    std::cmp::Ordering::Greater | std::cmp::Ordering::Equal => {
                                        ctx.pass(&actual, &expected)
                                    }
                                    std::cmp::Ordering::Less => ctx.fail(&actual, &expected),
                                },
                            }
                        }
                        AssertionMode::Exists => ctx.exists_pass(&actual),
                    },
                },
                // The balance query reports failures as a JSON error object
                // on stdout with exit code 0, so the cause is in the
                // structured `type` field rather than in CLI text.
                Err(BalanceError::Structured { typ, message }) => {
                    let diag = crate::diagnostic::Diagnostic::classify_balance(&typ, &message);
                    let diagnostic =
                        crate::simulate::redact(&format!("stellar error {typ}: {}", diag.detail));
                    ctx.error_with(&diagnostic, Some(diag))
                }
                Err(e) => ctx.error(&format!("failed to parse balance JSON: {e}")),
            },
            // Classify on the RunnerError variant, so a local timeout is
            // reported as a timeout rather than as an RPC failure.
            Err(e) => {
                let diag = crate::diagnostic::Diagnostic::classify_runner(&e);
                ctx.error_with(&format!("stellar CLI error: {e}"), Some(diag))
            }
        }
    }
}

/// Return the first non-blank string, trimmed, or `None` if all are blank.
pub fn first_non_empty(candidates: &[&String]) -> Option<String> {
    candidates
        .iter()
        .map(|s| s.trim())
        .find(|s| !s.is_empty())
        .map(String::from)
}

/// A balance-query failure.
///
/// The CLI reports these as a JSON error object on stdout with exit code 0,
/// so the cause lives in the structured `type` field rather than in CLI text.
/// Keeping them separate lets the caller classify from the type instead of
/// guessing from a message.
#[derive(Debug)]
enum BalanceError {
    /// A structured `{"error": {"type": ..., "message": ...}}` object.
    Structured { typ: String, message: String },
    /// The output was not valid JSON, or had no `balance` field.
    Malformed(String),
}

impl std::fmt::Display for BalanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Structured { typ, message } => write!(f, "stellar error {typ}: {message}"),
            Self::Malformed(m) => write!(f, "{m}"),
        }
    }
}

/// Parse `{"balance":"<value>"}` or `{"error":{...}}` from stdout.
///
/// There is deliberately no success path that yields "no balance": a balance
/// query either returns a balance or fails. Collapsing "no balance" into a
/// success would let a missing account read as a zero balance.
fn parse_balance_json(stdout: &str) -> Result<String, BalanceError> {
    let v: serde_json::Value =
        serde_json::from_str(stdout).map_err(|e| BalanceError::Malformed(e.to_string()))?;
    if let Some(error) = v.get("error") {
        let typ = error
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("unknown")
            .to_string();
        let message = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("no message")
            .to_string();
        return Err(BalanceError::Structured { typ, message });
    }
    let balance = v.get("balance").and_then(|b| b.as_str()).ok_or_else(|| {
        BalanceError::Malformed("missing 'balance' field in JSON output".to_string())
    })?;
    if balance.trim().is_empty() {
        return Err(BalanceError::Malformed("balance is empty".to_string()));
    }
    Ok(balance.to_string())
}

/// Validate that a balance string is a non-negative decimal integer.
///
/// The Stellar CLI reports raw balances as decimal integer strings in the
/// asset's smallest unit. Anything else — a decimal point, a sign, a
/// scientific-notation exponent, non-ASCII digits — is rejected rather than
/// coerced, because silently reading a malformed value as zero would turn a
/// parser failure into a passing assertion.
fn validate_balance_string(balance: &str) -> Result<(), String> {
    if balance.is_empty() {
        return Err("balance is empty".to_string());
    }
    if !balance.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "balance '{balance}' is not a non-negative decimal integer"
        ));
    }
    if u128::from_str(balance).is_err() {
        return Err(format!("balance '{balance}' exceeds the u128 range"));
    }
    Ok(())
}

/// Compare two balance strings as unsigned integers.
///
/// Both values must be decimal integers. No floating-point arithmetic is
/// used. Returns `Ordering` of `actual` vs `expected`.
///
/// Callers should validate both strings with [`validate_balance_string`]
/// first; this function falls back to `0` for unparseable input so it never
/// panics, but a fallback zero is not a meaningful comparison.
fn compare_balances(actual: &str, expected: &str) -> std::cmp::Ordering {
    let a = u128::from_str(actual).unwrap_or(0);
    let e = u128::from_str(expected).unwrap_or(0);
    a.cmp(&e)
}

// --------------------------------------------------------------------------
// Scenario-level aggregate report
// --------------------------------------------------------------------------

/// Overall status of a scenario run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioStatus {
    /// Every executed assertion passed.
    Passed,
    /// At least one assertion failed, none errored.
    Failed,
    /// At least one assertion errored.
    Errored,
    /// No assertion was executed (all steps skipped or none executable).
    NoAssertions,
}

/// Aggregate report for one scenario run.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ScenarioReport {
    /// Scenario name.
    pub scenario: String,
    /// Schema version declared by the scenario file.
    pub schema_version: String,
    /// Network profile the run targeted.
    pub network: String,
    /// Total steps declared in the scenario.
    pub total_steps: usize,
    /// Assertions that passed.
    pub passed: usize,
    /// Assertions that failed.
    pub failed: usize,
    /// Assertions that errored.
    pub errors: usize,
    /// Steps skipped because they would mutate remote state.
    pub skipped_mutating: usize,
    /// Steps skipped because they are read-only but not yet executable.
    pub skipped_not_executable: usize,
    /// Overall status.
    pub status: ScenarioStatus,
    /// Per-step assertion results, in step order.
    pub results: Vec<AssertBalanceResult>,
    /// Per-step skip records, in step order.
    pub skipped: Vec<SkippedStep>,
}

/// A step that was not executed.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SkippedStep {
    /// Zero-based step index.
    pub step_index: usize,
    /// Action name.
    pub action: String,
    /// Why the step was skipped.
    pub reason: SkipReason,
}

/// Why a step was not executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// The step would mutate remote state and is out of scope for `assert`.
    WouldMutate,
    /// The step is read-only but has no executor yet.
    NotExecutable,
}

impl ScenarioReport {
    /// Build a report from a scenario, its assertion results, and its skips.
    ///
    /// Aggregation is deterministic and order-independent:
    /// - `errored` wins over everything (at least one error → `errored`)
    /// - then `failed` (at least one failure, no errors → `failed`)
    /// - then `passed` (at least one pass, no failures or errors)
    /// - otherwise `no_assertions`
    ///
    /// A skipped step is never counted as a pass. An error is never counted
    /// as a pass.
    pub fn new(
        scenario: &Scenario,
        results: Vec<AssertBalanceResult>,
        skipped: Vec<SkippedStep>,
    ) -> Self {
        let passed = results
            .iter()
            .filter(|r| r.status == AssertStatus::Pass)
            .count();
        let failed = results
            .iter()
            .filter(|r| r.status == AssertStatus::Fail)
            .count();
        let errors = results
            .iter()
            .filter(|r| r.status == AssertStatus::Error)
            .count();
        let skipped_mutating = skipped
            .iter()
            .filter(|s| s.reason == SkipReason::WouldMutate)
            .count();
        let skipped_not_executable = skipped
            .iter()
            .filter(|s| s.reason == SkipReason::NotExecutable)
            .count();

        let status = if errors > 0 {
            ScenarioStatus::Errored
        } else if failed > 0 {
            ScenarioStatus::Failed
        } else if passed > 0 {
            ScenarioStatus::Passed
        } else {
            ScenarioStatus::NoAssertions
        };

        Self {
            scenario: scenario.name.clone(),
            schema_version: scenario.schema_version.clone(),
            network: scenario.network.clone(),
            total_steps: scenario.steps.len(),
            passed,
            failed,
            errors,
            skipped_mutating,
            skipped_not_executable,
            status,
            results,
            skipped,
        }
    }

    /// Serialize to pretty JSON with a trailing newline.
    pub fn to_json(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        s.push('\n');
        s
    }

    /// Render a human-readable summary.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "{} on {}: {} ({} step(s), {} passed, {} failed, {} error(s), {} skipped)\n",
            self.scenario,
            self.network,
            status_label(self.status),
            self.total_steps,
            self.passed,
            self.failed,
            self.errors,
            self.skipped_mutating + self.skipped_not_executable
        ));
        for r in &self.results {
            let mark = match r.status {
                AssertStatus::Pass => "PASS",
                AssertStatus::Fail => "FAIL",
                AssertStatus::Error => "ERROR",
            };
            out.push_str(&format!(
                "  {mark}\t[{}]\t{} on {}\t{}\n",
                r.step_index, r.account, r.network, r.diagnostic
            ));
        }
        for s in &self.skipped {
            let reason = match s.reason {
                SkipReason::WouldMutate => "would mutate remote state",
                SkipReason::NotExecutable => "no executor yet",
            };
            out.push_str(&format!(
                "  SKIP\t[{}]\t{}\t{}\n",
                s.step_index, s.action, reason
            ));
        }
        out
    }
}

fn status_label(status: ScenarioStatus) -> &'static str {
    match status {
        ScenarioStatus::Passed => "passed",
        ScenarioStatus::Failed => "failed",
        ScenarioStatus::Errored => "errored",
        ScenarioStatus::NoAssertions => "no assertions",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_success_json() {
        let out = parse_balance_json("{\"balance\":\"12345\"}\n").unwrap();
        assert_eq!(out, "12345");
    }

    #[test]
    fn parse_error_json() {
        // A structured error keeps the CLI's `type` and `message` separate,
        // so the caller can classify from the type instead of guessing.
        match parse_balance_json(
            "{\"error\":{\"type\":\"invalid_address\",\"message\":\"not found\"}}\n",
        )
        .unwrap_err()
        {
            BalanceError::Structured { typ, message } => {
                assert_eq!(typ, "invalid_address");
                assert_eq!(message, "not found");
            }
            BalanceError::Malformed(m) => panic!("expected structured error, got {m}"),
        }
    }

    #[test]
    fn parse_error_json_with_no_message() {
        match parse_balance_json("{\"error\":{\"type\":\"sac_not_deployed\"}}\n").unwrap_err() {
            BalanceError::Structured { typ, message } => {
                assert_eq!(typ, "sac_not_deployed");
                assert_eq!(message, "no message");
            }
            BalanceError::Malformed(m) => panic!("expected structured error, got {m}"),
        }
    }

    #[test]
    fn parse_missing_balance_field() {
        let err = parse_balance_json("{\"decimals\":7}\n").unwrap_err();
        assert!(err.to_string().contains("missing 'balance'"));
    }

    #[test]
    fn parse_malformed_json() {
        let err = parse_balance_json("not json").unwrap_err();
        assert!(
            err.to_string().contains("json") || err.to_string().contains("expected"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn compare_balances_integers() {
        use std::cmp::Ordering;
        assert_eq!(compare_balances("100", "100"), Ordering::Equal);
        assert_eq!(compare_balances("200", "100"), Ordering::Greater);
        assert_eq!(compare_balances("50", "100"), Ordering::Less);
        assert_eq!(compare_balances("0", "0"), Ordering::Equal);
    }

    #[test]
    fn compare_balances_boundary_values() {
        use std::cmp::Ordering;
        // u128 max, well beyond any real balance.
        let max = u128::MAX.to_string();
        assert_eq!(compare_balances(&max, "1"), Ordering::Greater);
        assert_eq!(compare_balances(&max, &max), Ordering::Equal);
        assert_eq!(compare_balances("1", &max), Ordering::Less);
    }

    #[test]
    fn compare_balances_malformed_falls_back_to_zero() {
        use std::cmp::Ordering;
        assert_eq!(compare_balances("nope", "0"), Ordering::Equal);
        assert_eq!(compare_balances("", "1"), Ordering::Less);
    }

    #[test]
    fn validate_balance_string_accepts_integers() {
        assert!(validate_balance_string("0").is_ok());
        assert!(validate_balance_string("99282512062").is_ok());
        assert!(validate_balance_string(&u128::MAX.to_string()).is_ok());
    }

    #[test]
    fn validate_balance_string_rejects_non_integers() {
        for bad in [
            "", "12.5", "-1", "+1", "1e10", " 1", "1 ", "0x10", "١٢٣", "1_000", "1,000",
        ] {
            assert!(
                validate_balance_string(bad).is_err(),
                "expected {bad:?} to be rejected"
            );
        }
    }

    #[test]
    fn validate_balance_string_rejects_above_u128() {
        let too_big = format!("1{}", "0".repeat(40));
        assert!(validate_balance_string(&too_big).is_err());
    }

    #[test]
    fn timeout_duration_defaults_to_120() {
        let s = Scenario::from_str(
            r#"schema_version: "1.0"
name: t
network: testnet
steps:
  - action: assert_asset
    identity: a
"#,
        )
        .unwrap();
        assert_eq!(s.timeout_duration(), std::time::Duration::from_secs(120));
    }

    #[test]
    fn timeout_duration_uses_scenario_value() {
        let s = Scenario::from_str(
            r#"schema_version: "1.0"
name: t
network: testnet
timeout_seconds: 45
steps:
  - action: assert_asset
    identity: a
"#,
        )
        .unwrap();
        assert_eq!(s.timeout_duration(), std::time::Duration::from_secs(45));
    }

    #[test]
    fn timeout_zero_is_rejected() {
        let err = scenario_with_err(
            r#"schema_version: "1.0"
name: t
network: testnet
timeout_seconds: 0
steps:
  - action: assert_asset
    identity: a
"#,
        );
        assert!(err.iter().any(|i| i.field == "timeout_seconds"));
    }

    #[test]
    fn timeout_above_max_is_rejected() {
        let err = scenario_with_err(
            r#"schema_version: "1.0"
name: t
network: testnet
timeout_seconds: 3601
steps:
  - action: assert_asset
    identity: a
"#,
        );
        assert!(err.iter().any(|i| i.field == "timeout_seconds"));
    }

    #[test]
    fn timeout_boundaries_are_accepted() {
        for secs in [1u64, 3600] {
            let raw = format!(
                r#"schema_version: "1.0"
name: t
network: testnet
timeout_seconds: {secs}
steps:
  - action: assert_asset
    identity: a
"#
            );
            assert!(
                Scenario::from_str(&raw).is_ok(),
                "timeout {secs} should be valid"
            );
        }
    }

    fn scenario_with_err(raw: &str) -> Vec<crate::error::ValidationIssue> {
        match Scenario::from_str(raw) {
            Ok(_) => panic!("expected validation failure"),
            Err(crate::error::ScenarioError::Invalid { issues }) => issues,
            Err(other) => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn mode_for_action() {
        assert_eq!(
            AssertionMode::for_action("assert_balance"),
            Some(AssertionMode::MinBalance)
        );
        assert_eq!(
            AssertionMode::for_action("assert_asset"),
            Some(AssertionMode::Exists)
        );
        assert_eq!(AssertionMode::for_action("payment"), None);
    }

    fn scenario_with(raw: &str) -> Scenario {
        Scenario::from_str(raw).unwrap()
    }

    fn result(
        scenario: &Scenario,
        idx: usize,
        action: &str,
        mode: AssertionMode,
        status: AssertStatus,
        actual: Option<&str>,
    ) -> AssertBalanceResult {
        AssertBalanceResult {
            scenario: scenario.name.clone(),
            step_index: idx,
            action: action.to_string(),
            mode,
            account: "alice".into(),
            network: scenario.network.clone(),
            asset: "native".into(),
            expected_min_balance: if mode == AssertionMode::MinBalance {
                Some("1".into())
            } else {
                None
            },
            actual_balance: actual.map(String::from),
            status,
            diagnostic: "d".into(),
            diagnostic_category: None,
            remediation: None,
        }
    }

    #[test]
    fn report_all_pass() {
        let s = scenario_with(
            r#"schema_version: "1.0"
name: r
network: testnet
steps:
  - action: assert_balance
    identity: alice
    min_balance: "1"
"#,
        );
        let rep = ScenarioReport::new(
            &s,
            vec![result(
                &s,
                0,
                "assert_balance",
                AssertionMode::MinBalance,
                AssertStatus::Pass,
                Some("5"),
            )],
            vec![],
        );
        assert_eq!(rep.status, ScenarioStatus::Passed);
        assert_eq!(rep.passed, 1);
        assert_eq!(rep.failed, 0);
        assert_eq!(rep.errors, 0);
        assert_eq!(rep.total_steps, 1);
    }

    #[test]
    fn report_error_beats_failure() {
        let s = scenario_with(
            r#"schema_version: "1.0"
name: r
network: testnet
steps:
  - action: assert_balance
    identity: alice
    min_balance: "1"
  - action: assert_balance
    identity: bob
    min_balance: "1"
"#,
        );
        let results = vec![
            result(
                &s,
                0,
                "assert_balance",
                AssertionMode::MinBalance,
                AssertStatus::Fail,
                Some("0"),
            ),
            result(
                &s,
                1,
                "assert_balance",
                AssertionMode::MinBalance,
                AssertStatus::Error,
                None,
            ),
        ];
        let rep = ScenarioReport::new(&s, results, vec![]);
        assert_eq!(rep.status, ScenarioStatus::Errored);
        assert_eq!(rep.failed, 1);
        assert_eq!(rep.errors, 1);
    }

    #[test]
    fn report_failure_beats_pass() {
        let s = scenario_with(
            r#"schema_version: "1.0"
name: r
network: testnet
steps:
  - action: assert_balance
    identity: alice
    min_balance: "1"
  - action: assert_balance
    identity: bob
    min_balance: "1"
"#,
        );
        let results = vec![
            result(
                &s,
                0,
                "assert_balance",
                AssertionMode::MinBalance,
                AssertStatus::Pass,
                Some("5"),
            ),
            result(
                &s,
                1,
                "assert_balance",
                AssertionMode::MinBalance,
                AssertStatus::Fail,
                Some("0"),
            ),
        ];
        let rep = ScenarioReport::new(&s, results, vec![]);
        assert_eq!(rep.status, ScenarioStatus::Failed);
        assert_eq!(rep.passed, 1);
        assert_eq!(rep.failed, 1);
    }

    #[test]
    fn report_skipped_is_not_a_pass() {
        let s = scenario_with(
            r#"schema_version: "1.0"
name: r
network: testnet
steps:
  - action: ensure_funded
    identity: alice
  - action: payment
    from: alice
    to: bob
    amount: "1"
"#,
        );
        let skipped = vec![
            SkippedStep {
                step_index: 0,
                action: "ensure_funded".into(),
                reason: SkipReason::WouldMutate,
            },
            SkippedStep {
                step_index: 1,
                action: "payment".into(),
                reason: SkipReason::WouldMutate,
            },
        ];
        let rep = ScenarioReport::new(&s, vec![], skipped);
        assert_eq!(rep.status, ScenarioStatus::NoAssertions);
        assert_eq!(rep.passed, 0);
        assert_eq!(rep.skipped_mutating, 2);
    }

    #[test]
    fn report_no_assertions_when_nothing_executed() {
        let s = scenario_with(
            r#"schema_version: "1.0"
name: r
network: testnet
steps:
  - action: ensure_funded
    identity: alice
"#,
        );
        let rep = ScenarioReport::new(
            &s,
            vec![],
            vec![SkippedStep {
                step_index: 0,
                action: "ensure_funded".into(),
                reason: SkipReason::WouldMutate,
            }],
        );
        assert_eq!(rep.status, ScenarioStatus::NoAssertions);
    }

    #[test]
    fn report_serializes_stably() {
        let s = scenario_with(
            r#"schema_version: "1.0"
name: r
network: testnet
steps:
  - action: assert_balance
    identity: alice
    min_balance: "1"
"#,
        );
        let rep = ScenarioReport::new(
            &s,
            vec![result(
                &s,
                0,
                "assert_balance",
                AssertionMode::MinBalance,
                AssertStatus::Pass,
                Some("5"),
            )],
            vec![],
        );
        let json = rep.to_json();
        assert!(json.contains("\"status\": \"passed\""));
        assert!(json.contains("\"passed\": 1"));
        assert!(json.contains("\"total_steps\": 1"));
        assert!(json.contains("\"schema_version\": \"1.0\""));
        // Field order is deterministic: struct declaration order.
        let scenario_pos = json.find("\"scenario\"").unwrap();
        let status_pos = json.find("\"status\"").unwrap();
        assert!(scenario_pos < status_pos);
    }

    #[test]
    fn report_text_lists_skips() {
        let s = scenario_with(
            r#"schema_version: "1.0"
name: r
network: testnet
steps:
  - action: ensure_funded
    identity: alice
"#,
        );
        let rep = ScenarioReport::new(
            &s,
            vec![],
            vec![SkippedStep {
                step_index: 0,
                action: "ensure_funded".into(),
                reason: SkipReason::WouldMutate,
            }],
        );
        let text = rep.to_text();
        assert!(text.contains("SKIP"));
        assert!(text.contains("ensure_funded"));
        assert!(text.contains("no assertions"));
    }

    #[test]
    fn result_error_has_no_actual_balance() {
        let s = scenario_with(
            r#"schema_version: "1.0"
name: x
network: testnet
steps:
  - action: assert_balance
    identity: alice
    min_balance: "1"
"#,
        );
        let r = AssertCtx::from_step(&s, 0, &s.steps[0]).error("timeout");
        assert_eq!(r.status, AssertStatus::Error);
        assert!(r.actual_balance.is_none());
        assert!(r.diagnostic.contains("timeout"));
        assert_eq!(r.expected_min_balance.as_deref(), Some("1"));
    }

    #[test]
    fn exists_mode_omits_expected_balance() {
        let s = scenario_with(
            r#"schema_version: "1.0"
name: x
network: testnet
steps:
  - action: assert_asset
    identity: alice
"#,
        );
        let r = AssertCtx::from_step(&s, 0, &s.steps[0]).exists_pass("0");
        assert_eq!(r.status, AssertStatus::Pass);
        assert!(r.expected_min_balance.is_none());
        assert_eq!(r.actual_balance.as_deref(), Some("0"));
    }
}
