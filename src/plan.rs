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
    /// Whether this command changes remote state.
    pub side_effect: SideEffect,
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

        Self {
            scenario: scenario.name.clone(),
            network: scenario.network.clone(),
            commands,
            read_only,
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
            out.push_str(&format!(
                "  [{}] {} {}\n",
                c.step_index,
                c.executable,
                c.args.join(" ")
            ));
        }
        if self.read_only {
            out.push_str("  (all steps read-only)\n");
        }
        out
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

    let (args, side_effect) = match step.action.as_str() {
        "ensure_funded" => (
            vec![
                "keys".to_string(),
                "fund".to_string(),
                source.to_string(),
                "--network".to_string(),
                scenario.network.clone(),
            ],
            SideEffect::MutatesRemoteState,
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
            )
        }
        "assert_balance" | "assert_asset" => {
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
            )
        }
        "inspect_contract" => {
            let contract_id = step
                .params
                .get("contract_id")
                .and_then(|v| v.as_str())
                .unwrap_or("<unset>");
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
            if let Some(seq) = step.params.get("args").and_then(|v| v.as_sequence()) {
                for item in seq {
                    if let Some(s) = item.as_str() {
                        args.push(s.to_string());
                    }
                }
            }
            (args, SideEffect::ReadOnly)
        }
        other => (vec![other.to_string()], SideEffect::ReadOnly),
    };

    PlannedCommand {
        step_index: index,
        action: step.action.clone(),
        executable: executable.to_string(),
        args,
        side_effect,
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
}
