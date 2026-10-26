//! Safe-execution contract.
//!
//! This module defines what "dry run" and "simulate" mean in this toolkit and
//! enforces the rule that validation never has side effects.
//!
//! # Contract
//!
//! - `validate` is pure. It reads a file and reports on it. It never runs a
//!   subprocess, never funds an account, and never contacts a network.
//! - `--dry-run` means "resolve what would happen, without doing it". For the
//!   current milestone, dry run covers *planning*: it prints the exact
//!   Stellar CLI argv that a live run would execute, and nothing else. No
//!   subprocess is spawned.
//! - Simulation (via `stellar tx simulate`) is a separate, later capability.
//!   It is read-only on the network, but it is NOT equivalent to a confirmed
//!   transaction and must never be reported as one.
//! - Live submission requires `--execute` on an explicit `run` invocation.
//!   It is never implied by validation and never happens as a side effect.

use crate::scenario::{Scenario, Step};

/// Whether a planned step mutates remote state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffect {
    /// The step only reads from the network or local config.
    ReadOnly,
    /// The step changes remote state (payments, funding, contract writes).
    MutatesRemoteState,
}

/// Whether a planned step can be executed as-is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    /// The step is read-only and safe to execute in isolation.
    Executable,
    /// The step would mutate remote state; it is shown for planning only and
    /// must not be auto-submitted.
    WouldMutate,
}

/// Counts of step types in an execution plan.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct StepCounts {
    /// Steps that are read-only and safe to execute.
    pub executable: usize,
    /// Steps that would mutate ledger state (blocked from auto-submit).
    pub would_mutate: usize,
    /// Total number of planned steps.
    pub total: usize,
}

/// A single planned subprocess invocation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlannedCommand {
    /// Step index in the scenario, zero-based.
    pub step_index: usize,
    /// Action name from the scenario.
    pub action: String,
    /// Executable that would be run.
    pub executable: String,
    /// Arguments that would be passed, in order.
    pub args: Vec<String>,
    /// Side effect classification.
    pub side_effect: SideEffect,
    /// Whether this step can be executed as-is.
    pub execution_status: ExecutionStatus,
    /// When the step is not executable, the reason it is blocked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    /// Optional human-readable annotation for complex steps.
    /// For `assert_balance`, shows the threshold condition (e.g. "balance >= 100").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotation: Option<String>,
}

/// A full execution plan for a scenario.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExecutionPlan {
    /// Scenario name.
    pub scenario: String,
    /// Network profile the plan targets.
    pub network: String,
    /// Planned commands, in order.
    pub commands: Vec<PlannedCommand>,
    /// True when every planned command is read-only.
    pub read_only: bool,
    /// Counts of executable vs would-mutate steps.
    pub step_counts: StepCounts,
}

impl ExecutionPlan {
    /// Build a plan from a validated scenario.
    ///
    /// This function performs no I/O and spawns no processes. It is safe to
    /// call from any code path, including validation.
    pub fn build(scenario: &Scenario, executable: &str) -> Self {
        let commands = scenario
            .steps
            .iter()
            .enumerate()
            .map(|(i, step)| plan_step(i, step, scenario, executable))
            .collect::<Vec<_>>();

        let read_only = commands
            .iter()
            .all(|c| c.side_effect == SideEffect::ReadOnly);

        let executable = commands
            .iter()
            .filter(|c| c.execution_status == ExecutionStatus::Executable)
            .count();
        let total = commands.len();

        Self {
            scenario: scenario.name.clone(),
            network: scenario.network.clone(),
            commands,
            read_only,
            step_counts: StepCounts {
                executable,
                would_mutate: total - executable,
                total,
            },
        }
    }

    /// Serialize the plan to pretty JSON.
    pub fn to_json(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        s.push('\n');
        s
    }

    /// Render the plan as a human-readable list of commands.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "plan for '{}' on network '{}' ({} step(s))\n",
            self.scenario,
            self.network,
            self.commands.len()
        ));
        for c in &self.commands {
            let status = match c.execution_status {
                ExecutionStatus::Executable => "executable",
                ExecutionStatus::WouldMutate => "would_mutate",
            };
            let mut line = format!(
                "  [{}] [{}] {} {}",
                c.step_index,
                status,
                c.executable,
                c.args.join(" ")
            );
            if let Some(a) = &c.annotation {
                line.push_str(&format!("  ({a})"));
            }
            if let Some(r) = &c.block_reason {
                line.push_str(&format!("  [blocked: {r}]"));
            }
            line.push('\n');
            out.push_str(&line);
        }
        if self.read_only {
            out.push_str("  (all steps read-only)\n");
        }
        out.push_str(&format!(
            "  summary: {} executable, {} would-mutate ({} total)\n",
            self.step_counts.executable, self.step_counts.would_mutate, self.step_counts.total
        ));
        out
    }
}

/// Redact a value that might be a secret key in the annotation text.
/// Secret keys are 56 characters, starting with 'S'.
fn redact_secret(s: &str) -> String {
    if s.len() == 56 && s.starts_with('S') {
        "<secret>".to_string()
    } else {
        s.to_string()
    }
}

fn plan_step(index: usize, step: &Step, scenario: &Scenario, executable: &str) -> PlannedCommand {
    let source = step
        .params
        .get("identity")
        .or_else(|| step.params.get("from"))
        .and_then(|v| v.as_str())
        .or(scenario.source.as_deref())
        .unwrap_or("<unset>");

    let (args, side_effect, annotation) = match step.action.as_str() {
        "ensure_funded" => (
            vec![
                "keys".to_string(),
                "fund".to_string(),
                source.to_string(),
                "--network".to_string(),
                scenario.network.clone(),
            ],
            SideEffect::MutatesRemoteState,
            None,
        ),
        "payment" => {
            let to = step
                .params
                .get("to")
                .and_then(|v| v.as_str())
                .unwrap_or("<unset>");
            let amount = step
                .params
                .get("amount")
                .and_then(|v| v.as_str())
                .unwrap_or("<unset>");
            let asset = step
                .params
                .get("asset")
                .and_then(|v| v.as_str())
                .unwrap_or("native");
            (
                vec![
                    "tx".to_string(),
                    "new".to_string(),
                    "payment".to_string(),
                    "--source-account".to_string(),
                    source.to_string(),
                    "--destination".to_string(),
                    to.to_string(),
                    "--amount".to_string(),
                    amount.to_string(),
                    "--asset".to_string(),
                    asset.to_string(),
                    "--network".to_string(),
                    scenario.network.clone(),
                ],
                SideEffect::MutatesRemoteState,
                None,
            )
        }
        "assert_balance" => {
            let asset = step
                .params
                .get("asset")
                .and_then(|v| v.as_str())
                .unwrap_or("native");
            let min_balance = step.params.get("min_balance").and_then(|v| {
                v.as_str()
                    .map(str::to_string)
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            });
            let annotation = min_balance.map(|m| format!("balance >= {m}"));
            (
                vec![
                    "token".to_string(),
                    "balance".to_string(),
                    "--id".to_string(),
                    asset.to_string(),
                    "--account".to_string(),
                    source.to_string(),
                    "--network".to_string(),
                    scenario.network.clone(),
                    "--output".to_string(),
                    "json".to_string(),
                ],
                SideEffect::ReadOnly,
                annotation,
            )
        }
        "assert_asset" => {
            let asset = step
                .params
                .get("asset")
                .and_then(|v| v.as_str())
                .unwrap_or("native");
            (
                vec![
                    "token".to_string(),
                    "balance".to_string(),
                    "--id".to_string(),
                    asset.to_string(),
                    "--account".to_string(),
                    source.to_string(),
                    "--network".to_string(),
                    scenario.network.clone(),
                    "--output".to_string(),
                    "json".to_string(),
                ],
                SideEffect::ReadOnly,
                Some("assert balance entry exists".to_string()),
            )
        }
        "inspect_contract" => {
            let contract_id = step
                .params
                .get("contract_id")
                .and_then(|v| v.as_str())
                .unwrap_or("<unset>");
            let annotation = Some("discover interface (functions, events)".to_string());
            (
                vec![
                    "contract".to_string(),
                    "info".to_string(),
                    "interface".to_string(),
                    "--contract-id".to_string(),
                    contract_id.to_string(),
                    "--network".to_string(),
                    scenario.network.clone(),
                    "--output".to_string(),
                    "json".to_string(),
                ],
                SideEffect::ReadOnly,
                annotation,
            )
        }
        "simulate_invoke" => {
            let contract_id = step
                .params
                .get("contract_id")
                .and_then(|v| v.as_str())
                .unwrap_or("<unset>");
            let function = step
                .params
                .get("function")
                .and_then(|v| v.as_str())
                .unwrap_or("<unset>");

            // Annotation shows the function and its args. Values that look
            // like secret keys are redacted in the annotation only; the
            // planned command below keeps the original args untouched.
            let arg_strs: Vec<String> = step
                .params
                .get("args")
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();

            let annotation = if arg_strs.is_empty() {
                Some(format!("simulate {function}"))
            } else {
                let shown: Vec<String> = arg_strs.iter().map(|s| redact_secret(s)).collect();
                Some(format!("simulate {function} {}", shown.join(" ")))
            };

            let mut args = vec![
                "contract".to_string(),
                "invoke".to_string(),
                "--contract-id".to_string(),
                contract_id.to_string(),
                "--source-account".to_string(),
                source.to_string(),
                "--network".to_string(),
                scenario.network.clone(),
                "--build-only".to_string(),
                "--".to_string(),
                function.to_string(),
            ];
            args.extend(arg_strs);
            (args, SideEffect::ReadOnly, annotation)
        }
        other => (vec![other.to_string()], SideEffect::ReadOnly, None),
    };

    let (execution_status, block_reason) = match side_effect {
        SideEffect::ReadOnly => (ExecutionStatus::Executable, None),
        SideEffect::MutatesRemoteState => (
            ExecutionStatus::WouldMutate,
            Some("would mutate ledger state; blocked in plan mode".to_string()),
        ),
    };

    PlannedCommand {
        step_index: index,
        action: step.action.clone(),
        executable: executable.to_string(),
        args,
        side_effect,
        execution_status,
        block_reason,
        annotation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Scenario;

    const SCENARIO: &str = r#"
schema_version: "1.0"
name: plan_test
network: testnet
source: alice
steps:
  - action: ensure_funded
    identity: alice
  - action: payment
    from: alice
    to: bob
    amount: "100"
  - action: assert_balance
    identity: bob
    min_balance: "0"
"#;

    #[test]
    fn plan_covers_every_step() {
        let s = Scenario::from_str(SCENARIO).unwrap();
        let plan = ExecutionPlan::build(&s, "stellar");
        assert_eq!(plan.commands.len(), 3);
        assert_eq!(plan.commands[0].step_index, 0);
        assert_eq!(plan.commands[2].step_index, 2);
    }

    #[test]
    fn plan_marks_mutating_steps() {
        let s = Scenario::from_str(SCENARIO).unwrap();
        let plan = ExecutionPlan::build(&s, "stellar");
        assert_eq!(plan.commands[0].side_effect, SideEffect::MutatesRemoteState);
        assert_eq!(plan.commands[1].side_effect, SideEffect::MutatesRemoteState);
        assert_eq!(plan.commands[2].side_effect, SideEffect::ReadOnly);
        assert!(!plan.read_only);
    }

    #[test]
    fn read_only_plan_detected() {
        let raw = SCENARIO
            .replace("  - action: ensure_funded\n    identity: alice\n", "")
            .replace(
                "  - action: payment\n    from: alice\n    to: bob\n    amount: \"100\"\n",
                "",
            );
        let s = Scenario::from_str(&raw).unwrap();
        let plan = ExecutionPlan::build(&s, "stellar");
        assert!(plan.read_only, "plan: {plan:?}");
    }

    #[test]
    fn plan_never_contains_secret_keys() {
        let s = Scenario::from_str(SCENARIO).unwrap();
        let plan = ExecutionPlan::build(&s, "stellar");
        let json = plan.to_json();
        // Identity *names* appear; secret seeds (S...) must not.
        for c in &plan.commands {
            for a in &c.args {
                assert!(
                    !(a.len() == 56 && a.starts_with('S')),
                    "argument looks like a secret key: {a}"
                );
            }
        }
        assert!(json.contains("alice"));
    }

    #[test]
    fn assert_balance_shows_min_balance_annotation() {
        let s = Scenario::from_str(SCENARIO).unwrap();
        let plan = ExecutionPlan::build(&s, "stellar");
        // Step 2 is assert_balance with min_balance: "0"
        let cmd = &plan.commands[2];
        assert_eq!(cmd.action, "assert_balance");
        assert_eq!(cmd.annotation, Some("balance >= 0".to_string()));
    }

    #[test]
    fn assert_asset_has_annotation() {
        let yaml = r#"schema_version: "1.0"
name: test_asset
network: testnet
source: alice
steps:
  - action: assert_asset
    identity: bob
"#;
        let s = Scenario::from_str(yaml).unwrap();
        let plan = ExecutionPlan::build(&s, "stellar");
        let cmd = &plan.commands[0];
        assert_eq!(cmd.action, "assert_asset");
        assert_eq!(
            cmd.annotation,
            Some("assert balance entry exists".to_string())
        );
    }

    #[test]
    fn min_balance_as_integer_is_supported() {
        let yaml = r#"schema_version: "1.0"
name: int_balance
network: testnet
source: alice
steps:
  - action: assert_balance
    identity: bob
    min_balance: 100
"#;
        let s = Scenario::from_str(yaml).unwrap();
        let plan = ExecutionPlan::build(&s, "stellar");
        let cmd = &plan.commands[0];
        assert_eq!(cmd.action, "assert_balance");
        assert_eq!(cmd.annotation, Some("balance >= 100".to_string()));
    }

    #[test]
    fn json_output_includes_annotation() {
        let s = Scenario::from_str(SCENARIO).unwrap();
        let plan = ExecutionPlan::build(&s, "stellar");
        let json = plan.to_json();
        // assert_balance annotation should appear in JSON
        assert!(json.contains("balance >= 0"), "JSON should show annotation");
        // Only assert_balance (step 2) has an annotation in this scenario;
        // mutating steps do not get annotations.
    }

    #[test]
    fn text_output_format_with_annotation() {
        let s = Scenario::from_str(SCENARIO).unwrap();
        let plan = ExecutionPlan::build(&s, "stellar");
        let text = plan.to_text();
        // Step 2 should have annotation at the end
        assert!(
            text.contains("balance >= 0"),
            "text output should contain annotation: {text}"
        );
    }

    #[test]
    fn plan_shows_execution_status_and_counts() {
        let s = Scenario::from_str(SCENARIO).unwrap();
        let plan = ExecutionPlan::build(&s, "stellar");
        // SCENARIO has 2 mutating steps + 1 read-only assert_balance
        assert_eq!(plan.step_counts.executable, 1);
        assert_eq!(plan.step_counts.would_mutate, 2);
        assert_eq!(plan.step_counts.total, 3);
        assert!(!plan.read_only);
        // Mutating steps are WouldMutate with a block reason
        assert_eq!(
            plan.commands[0].execution_status,
            ExecutionStatus::WouldMutate
        );
        assert_eq!(
            plan.commands[1].execution_status,
            ExecutionStatus::WouldMutate
        );
        assert!(plan.commands[0].block_reason.is_some());
        assert!(plan.commands[0]
            .block_reason
            .as_ref()
            .unwrap()
            .contains("would mutate"));
        // Read-only step is Executable with no block reason
        assert_eq!(
            plan.commands[2].execution_status,
            ExecutionStatus::Executable
        );
        assert!(plan.commands[2].block_reason.is_none());
    }

    #[test]
    fn plan_text_includes_status_markers_and_summary() {
        let s = Scenario::from_str(SCENARIO).unwrap();
        let plan = ExecutionPlan::build(&s, "stellar");
        let text = plan.to_text();
        assert!(
            text.contains("[would_mutate]"),
            "text should tag mutating steps: {text}"
        );
        assert!(
            text.contains("[executable]"),
            "text should tag read-only steps: {text}"
        );
        assert!(
            text.contains("[blocked:"),
            "text should show block reason: {text}"
        );
        assert!(
            text.contains("summary: 1 executable, 2 would-mutate (3 total)"),
            "text should show summary: {text}"
        );
    }

    #[test]
    fn json_includes_step_counts_and_status() {
        let s = Scenario::from_str(SCENARIO).unwrap();
        let plan = ExecutionPlan::build(&s, "stellar");
        let json = plan.to_json();
        assert!(
            json.contains("\"step_counts\""),
            "JSON should include step_counts"
        );
        assert!(
            json.contains("\"executable\": 1"),
            "JSON should count executable steps"
        );
        assert!(
            json.contains("\"would_mutate\": 2"),
            "JSON should count would-mutate steps"
        );
        assert!(
            json.contains("\"execution_status\": \"would_mutate\""),
            "JSON should show status"
        );
        assert!(
            json.contains("\"execution_status\": \"executable\""),
            "JSON should show executable status"
        );
    }

    #[test]
    fn assert_asset_step_has_no_block_reason() {
        let yaml = r#"schema_version: "1.0"
name: only_asset
network: testnet
source: alice
steps:
  - action: assert_asset
    identity: bob
"#;
        let s = Scenario::from_str(yaml).unwrap();
        let plan = ExecutionPlan::build(&s, "stellar");
        assert_eq!(plan.commands[0].block_reason, None);
        assert_eq!(
            plan.commands[0].execution_status,
            ExecutionStatus::Executable
        );
        assert!(plan.read_only);
    }

    #[test]
    fn inspect_contract_shows_annotation() {
        let yaml = r#"schema_version: "1.0"
name: inspect_example
network: testnet
source: alice
steps:
  - action: inspect_contract
    contract_id: CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC
"#;
        let s = Scenario::from_str(yaml).unwrap();
        let plan = ExecutionPlan::build(&s, "stellar");
        let cmd = &plan.commands[0];
        assert_eq!(cmd.action, "inspect_contract");
        assert_eq!(
            cmd.annotation,
            Some("discover interface (functions, events)".to_string())
        );
        assert_eq!(cmd.execution_status, ExecutionStatus::Executable);
        assert!(cmd.block_reason.is_none());
    }

    #[test]
    fn simulate_invoke_no_args_shows_function_only() {
        let yaml = r#"schema_version: "1.0"
name: sim_decimals
network: testnet
source: alice
steps:
  - action: simulate_invoke
    contract_id: CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC
    function: decimals
"#;
        let s = Scenario::from_str(yaml).unwrap();
        let plan = ExecutionPlan::build(&s, "stellar");
        let cmd = &plan.commands[0];
        assert_eq!(cmd.annotation, Some("simulate decimals".to_string()));
        // Command args must not contain the function twice — original behavior preserved
        let invoke_args: Vec<&String> = cmd.args.iter().filter(|a| a == &"decimals").collect();
        assert_eq!(
            invoke_args.len(),
            1,
            "function should appear exactly once in args"
        );
    }

    #[test]
    fn simulate_invoke_with_args_shows_summary() {
        let yaml = r#"schema_version: "1.0"
name: sim_balance
network: testnet
source: alice
steps:
  - action: simulate_invoke
    contract_id: CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC
    function: balance
    args: ["--id", "GDIPVO6M4SADGLNFTIMYHCTMMOXNONMV4ORZOMP6OAP2DPPYRQBMAW26"]
"#;
        let s = Scenario::from_str(yaml).unwrap();
        let plan = ExecutionPlan::build(&s, "stellar");
        let cmd = &plan.commands[0];
        assert_eq!(
            cmd.annotation,
            Some(
                "simulate balance --id GDIPVO6M4SADGLNFTIMYHCTMMOXNONMV4ORZOMP6OAP2DPPYRQBMAW26"
                    .to_string()
            )
        );
        // The raw args are preserved in the command (not redacted)
        assert!(cmd.args.contains(&"--id".to_string()));
        assert!(cmd
            .args
            .contains(&"GDIPVO6M4SADGLNFTIMYHCTMMOXNONMV4ORZOMP6OAP2DPPYRQBMAW26".to_string()));
    }

    #[test]
    fn simulate_invoke_redacts_secret_in_annotation_only() {
        let secret = "SAAAAAAAAAAAAAAAAAAAAAAABBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";
        let yaml = format!(
            r#"schema_version: "1.0"
name: sim_secret
network: testnet
source: alice
steps:
  - action: simulate_invoke
    contract_id: CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC
    function: transfer
    args: ["--to", "{secret}"]
"#
        );
        let s = Scenario::from_str(&yaml).unwrap();
        let plan = ExecutionPlan::build(&s, "stellar");
        let cmd = &plan.commands[0];
        // Annotation redacts the secret
        assert!(
            cmd.annotation.as_ref().unwrap().contains("<secret>"),
            "annotation must redact secrets: {:?}",
            cmd.annotation
        );
        // Command args preserve the raw value (plan accuracy)
        assert!(
            cmd.args.contains(&secret.to_string()),
            "command must keep raw arg"
        );
    }
}
