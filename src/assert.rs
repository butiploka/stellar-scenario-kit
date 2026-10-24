//! Balance assertion model and execution.
//!
//! This module implements the first read-only execution capability:
//! `assert_balance`. It runs `stellar token balance --id <asset> --account
//! <identity> --network <n> --output json`, parses the result, and compares
//! it against an expected minimum.
//!
//! # Safety
//!
//! - No transaction is submitted.
//! - No account is funded.
//! - No secret key is accepted as input.
//! - The only subprocess spawned is `stellar token balance`, which is a
//!   read-only RPC call.

use std::str::FromStr;

use crate::runner;
use crate::scenario::{Scenario, Step};

/// Result of a single balance assertion.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AssertBalanceResult {
    /// Scenario name.
    pub scenario: String,
    /// Zero-based step index.
    pub step_index: usize,
    /// Account that was queried.
    pub account: String,
    /// Network profile used.
    pub network: String,
    /// Asset identifier (`native`, `CODE:ISSUER`, or contract id).
    pub asset: String,
    /// Expected minimum balance in the asset's smallest unit (stroops for
    /// native XLM, or the token's smallest unit).
    pub expected_min_balance: String,
    /// Actual balance returned by the CLI, when the query succeeded.
    pub actual_balance: Option<String>,
    /// Pass/fail/error outcome.
    pub status: AssertStatus,
    /// Human-readable diagnostic. Never contains secret material.
    pub diagnostic: String,
}

/// Outcome of a balance assertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertStatus {
    /// Actual balance >= expected minimum.
    Pass,
    /// Query succeeded but balance is below the expected minimum.
    Fail,
    /// The balance query itself failed (CLI error, parse error, timeout).
    Error,
}

impl AssertBalanceResult {
    /// Execute an `assert_balance` step.
    ///
    /// Returns a structured result regardless of success or failure.
    pub fn execute(
        scenario: &Scenario,
        step_index: usize,
        step: &Step,
        executable: &std::path::Path,
    ) -> Self {
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

        let expected_min_balance = step
            .params
            .get("min_balance")
            .and_then(|v| v.as_str())
            .unwrap_or("0")
            .to_string();

        let args = vec![
            std::ffi::OsStr::new("token"),
            std::ffi::OsStr::new("balance"),
            std::ffi::OsStr::new("--id"),
            std::ffi::OsStr::new(&asset),
            std::ffi::OsStr::new("--account"),
            std::ffi::OsStr::new(&account),
            std::ffi::OsStr::new("--network"),
            std::ffi::OsStr::new(&scenario.network),
            std::ffi::OsStr::new("--output"),
            std::ffi::OsStr::new("json"),
        ];

        let run_result =
            runner::run_capture(executable.as_os_str(), &args, runner::DEFAULT_TIMEOUT);

        match run_result {
            Ok(output) => match parse_balance_json(&output.stdout) {
                Ok(Some(balance_str)) => {
                    let actual = balance_str;
                    let expected = expected_min_balance.clone();
                    match compare_balances(&actual, &expected) {
                        std::cmp::Ordering::Greater | std::cmp::Ordering::Equal => {
                            let diag = format!("balance {} >= expected {}", actual, expected);
                            Self {
                                scenario: scenario.name.clone(),
                                step_index,
                                account,
                                network: scenario.network.clone(),
                                asset,
                                expected_min_balance: expected,
                                actual_balance: Some(actual),
                                status: AssertStatus::Pass,
                                diagnostic: diag,
                            }
                        }
                        std::cmp::Ordering::Less => {
                            let diag = format!("balance {} < expected {}", actual, expected);
                            Self {
                                scenario: scenario.name.clone(),
                                step_index,
                                account,
                                network: scenario.network.clone(),
                                asset,
                                expected_min_balance: expected,
                                actual_balance: Some(actual),
                                status: AssertStatus::Fail,
                                diagnostic: diag,
                            }
                        }
                    }
                }
                Ok(None) => Self::error(
                    scenario,
                    step_index,
                    &account,
                    &asset,
                    &expected_min_balance,
                    "balance field missing in JSON output",
                ),
                Err(e) => Self::error(
                    scenario,
                    step_index,
                    &account,
                    &asset,
                    &expected_min_balance,
                    &format!("failed to parse balance JSON: {e}"),
                ),
            },
            Err(e) => Self::error(
                scenario,
                step_index,
                &account,
                &asset,
                &expected_min_balance,
                &format!("stellar CLI error: {e}"),
            ),
        }
    }

    fn error(
        scenario: &Scenario,
        step_index: usize,
        account: &str,
        asset: &str,
        expected: &str,
        diagnostic: &str,
    ) -> Self {
        Self {
            scenario: scenario.name.clone(),
            step_index,
            account: account.to_string(),
            network: scenario.network.clone(),
            asset: asset.to_string(),
            expected_min_balance: expected.to_string(),
            actual_balance: None,
            status: AssertStatus::Error,
            diagnostic: diagnostic.to_string(),
        }
    }
}

/// Parse `{"balance":"<value>"}` or `{"error":{...}}` from stdout.
///
/// Returns:
/// - `Ok(Some(balance_str))` on success
/// - `Ok(None)` when the JSON contains an error object
/// - `Err(...)` when the JSON is malformed or missing expected fields
fn parse_balance_json(stdout: &str) -> Result<Option<String>, String> {
    let v: serde_json::Value = serde_json::from_str(stdout).map_err(|e| e.to_string())?;
    if let Some(error) = v.get("error") {
        let typ = error
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("unknown");
        let msg = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("no message");
        return Err(format!("stellar error {typ}: {msg}"));
    }
    let balance = v
        .get("balance")
        .and_then(|b| b.as_str())
        .ok_or_else(|| "missing 'balance' field in JSON output".to_string())?;
    if balance.trim().is_empty() {
        return Err("balance is empty".to_string());
    }
    Ok(Some(balance.to_string()))
}

/// Compare two balance strings as unsigned integers.
///
/// Both values must be decimal integers. No floating-point arithmetic is
/// used. Returns `Ordering` of `actual` vs `expected`.
fn compare_balances(actual: &str, expected: &str) -> std::cmp::Ordering {
    let a = u128::from_str(actual).unwrap_or(0);
    let e = u128::from_str(expected).unwrap_or(0);
    a.cmp(&e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_success_json() {
        let out = parse_balance_json("{\"balance\":\"12345\"}\n").unwrap();
        assert_eq!(out, Some("12345".to_string()));
    }

    #[test]
    fn parse_error_json() {
        let err = parse_balance_json(
            "{\"error\":{\"type\":\"invalid_address\",\"message\":\"not found\"}}\n",
        )
        .unwrap_err();
        assert!(err.contains("invalid_address"));
        assert!(err.contains("not found"));
    }

    #[test]
    fn parse_missing_balance_field() {
        let err = parse_balance_json("{\"decimals\":7}\n").unwrap_err();
        assert!(err.contains("missing 'balance'"));
    }

    #[test]
    fn parse_malformed_json() {
        let err = parse_balance_json("not json").unwrap_err();
        // serde_json's message varies by version; assert on the stable part.
        assert!(
            err.contains("json") || err.contains("expected") || err.contains("line 1"),
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
    fn compare_balances_malformed_falls_back_to_zero() {
        use std::cmp::Ordering;
        assert_eq!(compare_balances("nope", "0"), Ordering::Equal);
        assert_eq!(compare_balances("", "1"), Ordering::Less);
    }

    #[test]
    fn result_serializes_to_json() {
        let r = AssertBalanceResult {
            scenario: "s".into(),
            step_index: 0,
            account: "GABC".into(),
            network: "testnet".into(),
            asset: "native".into(),
            expected_min_balance: "100".into(),
            actual_balance: Some("200".into()),
            status: AssertStatus::Pass,
            diagnostic: "ok".into(),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"status\":\"pass\""));
        assert!(json.contains("\"actual_balance\":\"200\""));
    }

    #[test]
    fn result_error_has_no_actual_balance() {
        let r = AssertBalanceResult::error(
            &Scenario::from_str(
                r#"schema_version: "1.0"
name: x
network: testnet
steps:
  - action: assert_balance
    identity: alice
"#,
            )
            .unwrap(),
            0,
            "alice",
            "native",
            "100",
            "timeout",
        );
        assert_eq!(r.status, AssertStatus::Error);
        assert!(r.actual_balance.is_none());
        assert!(r.diagnostic.contains("timeout"));
    }
}
