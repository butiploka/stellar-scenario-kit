//! Generate a `simulate_invoke` step from a contract interface.
//!
//! This module implements `stellar-scenario scaffold`: read a contract's
//! interface, pick one function, and print a YAML step an author can paste
//! into a scenario.
//!
//! # Why placeholders, not values
//!
//! The generator knows parameter *names* and *types*, not values. It never
//! invents an address, an amount, or a ledger number, because a plausible
//! looking value would read as ready-to-run and fail — or worse, succeed
//! against the wrong account. Every argument is emitted as an explicit
//! `<placeholder>` with a comment saying it must be replaced.
//!
//! # Supported argument types
//!
//! Only types whose CLI form is a single scalar are generated: `address`,
//! `muxed_address`, integers, `bool`, `string`, `symbol`, and `bytes`.
//! Container types (`vec<...>`, `map<...>`, `option<...>`) and unknown
//! composite types are refused with an explicit error naming the type,
//! rather than emitted with a placeholder that cannot be filled in correctly.
//!
//! # Safety
//!
//! The generated YAML describes a `simulate_invoke` step, which is read-only.
//! Nothing here runs anything: the command inspects a contract and prints
//! text. No transaction is built, signed, or submitted.

use std::path::Path;

use crate::inspect::{ContractInterface, InspectResult, InspectStatus, Signature};
use crate::scenario::Scenario;

/// Outcome of a scaffold attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaffoldStatus {
    /// A step was generated.
    Generated,
    /// The scenario has no `inspect_contract` step to take a contract from.
    NoContractInScenario,
    /// More than one contract matched and no `--contract-id` disambiguated.
    AmbiguousContract,
    /// The requested function is not in the contract interface.
    FunctionNotFound,
    /// The function takes an argument whose type cannot be expressed.
    UnsupportedType,
    /// The interface could not be fetched.
    InspectFailed,
    /// A subprocess-level error (missing CLI, timeout, spawn failure).
    Error,
}

/// One generated argument line.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct GeneratedArg {
    /// Parameter name from the contract spec.
    pub name: String,
    /// Soroban type from the contract spec.
    #[serde(rename = "type")]
    pub param_type: String,
    /// CLI flag, e.g. `--id`.
    pub flag: String,
    /// Placeholder the author must replace, e.g. `<address>`.
    pub placeholder: String,
}

/// Result of a scaffold attempt.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ScaffoldResult {
    /// Scenario the contract was taken from.
    pub scenario: String,
    /// Network profile used.
    pub network: String,
    /// Contract the function belongs to.
    pub contract_id: String,
    /// Function that was selected.
    pub function: String,
    /// Outcome.
    pub status: ScaffoldStatus,
    /// Arguments the step will carry, in declaration order.
    pub args: Vec<GeneratedArg>,
    /// The generated YAML step. Present only when `status` is `generated`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yaml: Option<String>,
    /// Human-readable diagnostic. Never contains secret material.
    pub diagnostic: String,
}

impl ScaffoldResult {
    /// Run the generator.
    ///
    /// `contract_filter` is the optional `--contract-id` value used to
    /// disambiguate a scenario that inspects more than one contract.
    pub fn execute(
        scenario: &Scenario,
        file: &Path,
        contract_filter: Option<&str>,
        function: &str,
        executable: &std::path::Path,
    ) -> Self {
        let base = Self {
            scenario: scenario.name.clone(),
            network: scenario.network.clone(),
            contract_id: String::new(),
            function: function.to_string(),
            status: ScaffoldStatus::Error,
            args: Vec::new(),
            yaml: None,
            diagnostic: String::new(),
        };

        // Collect the contract ids the scenario inspects, in step order.
        let mut contract_ids: Vec<String> = scenario
            .steps
            .iter()
            .filter(|s| s.action == "inspect_contract")
            .filter_map(|s| s.params.get("contract_id").and_then(|v| v.as_str()))
            .map(String::from)
            .collect();

        if let Some(filter) = contract_filter {
            contract_ids.retain(|id| id == filter);
            if contract_ids.is_empty() {
                return base.with(
                    ScaffoldStatus::NoContractInScenario,
                    format!(
                        "no inspect_contract step matches --contract-id {filter}; \
                         add one to {} or drop the flag",
                        file.display()
                    ),
                );
            }
        }

        contract_ids.dedup();
        match contract_ids.len() {
            0 => {
                return base.with(
                    ScaffoldStatus::NoContractInScenario,
                    format!(
                        "{} has no inspect_contract step; add one to select a contract",
                        file.display()
                    ),
                )
            }
            1 => {}
            _ => {
                return base.with(
                    ScaffoldStatus::AmbiguousContract,
                    format!(
                        "{} inspects {} contracts; pass --contract-id to pick one: {}",
                        file.display(),
                        contract_ids.len(),
                        contract_ids.join(", ")
                    ),
                )
            }
        }

        let contract_id = contract_ids.remove(0);
        let mut result = base;
        result.contract_id = contract_id.clone();

        let inspected = InspectResult::execute(
            &contract_id,
            &scenario.network,
            executable,
            scenario.timeout_duration(),
        );

        let interface: ContractInterface = match inspected.status {
            InspectStatus::Inspected => match inspected.interface {
                Some(i) => i,
                None => {
                    return result.with(
                        ScaffoldStatus::InspectFailed,
                        "inspect reported success but returned no interface".to_string(),
                    )
                }
            },
            InspectStatus::FetchFailed | InspectStatus::Error => {
                return result.with(
                    ScaffoldStatus::InspectFailed,
                    format!("could not inspect {contract_id}: {}", inspected.diagnostic),
                )
            }
            other => {
                return result.with(
                    ScaffoldStatus::InspectFailed,
                    format!(
                        "could not inspect {contract_id}: {other:?}: {}",
                        inspected.diagnostic
                    ),
                )
            }
        };

        let signature = match interface.functions.iter().find(|f| f.name == function) {
            Some(s) => s,
            None => {
                let mut names: Vec<&str> = interface
                    .functions
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect();
                names.sort_unstable();
                return result.with(
                    ScaffoldStatus::FunctionNotFound,
                    format!(
                        "no function '{function}' in {contract_id}; available: {}",
                        if names.is_empty() {
                            "<none>".to_string()
                        } else {
                            names.join(", ")
                        }
                    ),
                );
            }
        };

        let mut args = Vec::new();
        for input in &signature.inputs {
            let Some(placeholder) = placeholder_for(&input.param_type) else {
                return result.with(
                    ScaffoldStatus::UnsupportedType,
                    format!(
                        "function '{function}' takes '{}' of type '{}', which this \
                         generator does not support; write the step by hand",
                        input.name, input.param_type
                    ),
                );
            };
            args.push(GeneratedArg {
                name: input.name.clone(),
                param_type: input.param_type.clone(),
                flag: format!("--{}", input.name),
                placeholder: placeholder.to_string(),
            });
        }

        let yaml = render_step(&contract_id, function, &args);
        let mut done = result.with(
            ScaffoldStatus::Generated,
            format!(
                "generated step for {function} with {} argument(s); replace every \
                 <placeholder> before running",
                args.len()
            ),
        );
        done.args = args;
        done.yaml = Some(yaml);
        done
    }

    fn with(mut self, status: ScaffoldStatus, diagnostic: String) -> Self {
        self.status = status;
        self.diagnostic = crate::simulate::redact(&diagnostic);
        self
    }

    /// Render a human-readable summary. The YAML itself is the payload, so
    /// the text view prints the diagnostic and then the step.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "{}: {}\n",
            self.function,
            match self.status {
                ScaffoldStatus::Generated => "generated",
                ScaffoldStatus::NoContractInScenario => "no contract in scenario",
                ScaffoldStatus::AmbiguousContract => "ambiguous contract",
                ScaffoldStatus::FunctionNotFound => "function not found",
                ScaffoldStatus::UnsupportedType => "unsupported argument type",
                ScaffoldStatus::InspectFailed => "inspect failed",
                ScaffoldStatus::Error => "error",
            }
        ));
        out.push_str(&format!("  {}\n", self.diagnostic));
        if let Some(yaml) = &self.yaml {
            out.push('\n');
            out.push_str(yaml);
        }
        out
    }
}

/// The placeholder for a Soroban type, or `None` when the type cannot be
/// expressed as a single CLI argument.
fn placeholder_for(param_type: &str) -> Option<&'static str> {
    match param_type {
        "address" => Some("<address>"),
        "muxed_address" => Some("<muxed-address>"),
        "i32" | "i64" | "i128" | "u32" | "u64" | "u128" | "timepoint" | "duration" => {
            Some("<integer>")
        }
        "bool" => Some("<bool>"),
        "string" => Some("<string>"),
        "symbol" => Some("<symbol>"),
        "bytes" | "bytes_n" => Some("<hex-bytes>"),
        _ => None,
    }
}

/// Render the step as YAML.
///
/// The output is a single list item, so it can be pasted under an existing
/// `steps:` key. A leading comment states that the placeholders must be
/// replaced; without it the step looks runnable as printed.
fn render_step(contract_id: &str, function: &str, args: &[GeneratedArg]) -> String {
    let mut out = String::new();
    out.push_str("# Generated by `stellar-scenario scaffold`.\n");
    out.push_str("# Replace every <placeholder> before running.\n");
    out.push_str("- action: simulate_invoke\n");
    out.push_str(&format!("  contract_id: {}\n", yaml_scalar(contract_id)));
    out.push_str(&format!("  function: {}\n", yaml_scalar(function)));
    if !args.is_empty() {
        out.push_str("  args:\n");
        for a in args {
            out.push_str(&format!("    - \"{}\"\n", a.flag));
            out.push_str(&format!("    - \"{}\"\n", a.placeholder));
        }
    }
    out
}

/// Quote a scalar when it could otherwise be misread.
///
/// Contract ids and function names are alphanumeric in practice, but an
/// alias can contain characters that need quoting, so this checks rather
/// than assumes.
fn yaml_scalar(value: &str) -> String {
    let safe = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.');
    if safe {
        value.to_string()
    } else {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// Pick the function signatures a caller could scaffold, for help output.
pub fn scafoldable_functions(interface: &ContractInterface) -> Vec<&Signature> {
    interface
        .functions
        .iter()
        .filter(|f| {
            f.inputs
                .iter()
                .all(|i| placeholder_for(&i.param_type).is_some())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_of(pairs: &[(&str, &str)]) -> Vec<GeneratedArg> {
        pairs
            .iter()
            .map(|(name, ty)| GeneratedArg {
                name: name.to_string(),
                param_type: ty.to_string(),
                flag: format!("--{name}"),
                placeholder: placeholder_for(ty).unwrap().to_string(),
            })
            .collect()
    }

    #[test]
    fn placeholder_for_scalar_types() {
        assert_eq!(placeholder_for("address"), Some("<address>"));
        assert_eq!(placeholder_for("muxed_address"), Some("<muxed-address>"));
        assert_eq!(placeholder_for("i128"), Some("<integer>"));
        assert_eq!(placeholder_for("u32"), Some("<integer>"));
        assert_eq!(placeholder_for("bool"), Some("<bool>"));
        assert_eq!(placeholder_for("string"), Some("<string>"));
        assert_eq!(placeholder_for("symbol"), Some("<symbol>"));
        assert_eq!(placeholder_for("bytes"), Some("<hex-bytes>"));
    }

    #[test]
    fn placeholder_for_refuses_container_and_unknown_types() {
        for ty in [
            "option<u64>",
            "vec<address>",
            "map<address, i128>",
            "{\"udt\":{\"name\":\"Foo\"}}",
            "",
            "unknown",
        ] {
            assert_eq!(placeholder_for(ty), None, "type {ty:?} must be refused");
        }
    }

    #[test]
    fn renders_zero_arg_function() {
        let yaml = render_step("CABC", "decimals", &[]);
        assert!(yaml.contains("- action: simulate_invoke"));
        assert!(yaml.contains("  contract_id: CABC"));
        assert!(yaml.contains("  function: decimals"));
        assert!(!yaml.contains("args:"), "no args key when there are none");
    }

    #[test]
    fn renders_args_as_flag_value_pairs() {
        let args = args_of(&[("id", "address")]);
        let yaml = render_step("CABC", "balance", &args);
        assert!(
            yaml.contains("  args:\n    - \"--id\"\n    - \"<address>\"\n"),
            "{yaml}"
        );
    }

    #[test]
    fn renders_multiple_args_in_declaration_order() {
        let args = args_of(&[
            ("from", "address"),
            ("to", "muxed_address"),
            ("amount", "i128"),
        ]);
        let yaml = render_step("CABC", "transfer", &args);
        let from_pos = yaml.find("--from").unwrap();
        let to_pos = yaml.find("--to").unwrap();
        let amount_pos = yaml.find("--amount").unwrap();
        assert!(from_pos < to_pos && to_pos < amount_pos, "{yaml}");
    }

    #[test]
    fn generated_yaml_carries_a_replace_comment() {
        let yaml = render_step("CABC", "decimals", &[]);
        assert!(
            yaml.contains("Replace every <placeholder>"),
            "the step must say the placeholders are not values: {yaml}"
        );
    }

    #[test]
    fn yaml_scalar_quotes_when_needed() {
        assert_eq!(yaml_scalar("CABC"), "CABC");
        assert_eq!(yaml_scalar("my-alias_1"), "my-alias_1");
        assert_eq!(yaml_scalar("has space"), "\"has space\"");
        assert_eq!(yaml_scalar("has:colon"), "\"has:colon\"");
        assert_eq!(yaml_scalar(""), "\"\"");
        assert_eq!(yaml_scalar("quote\"inside"), "\"quote\\\"inside\"");
    }

    #[test]
    fn generated_step_parses_as_a_scenario_step() {
        // The strongest guarantee: the emitted text is valid YAML that the
        // real schema accepts, not just something that looks right.
        let args = args_of(&[("from", "address"), ("amount", "i128")]);
        let step = render_step("CABC", "transfer", &args);
        let scenario = format!(
            "schema_version: \"1.0\"\nname: generated\nnetwork: testnet\nsteps:\n{}",
            indent(&step)
        );
        let parsed = Scenario::from_str(&scenario)
            .unwrap_or_else(|e| panic!("generated step must validate: {e}\n{scenario}"));
        assert_eq!(parsed.steps.len(), 1);
        assert_eq!(parsed.steps[0].action, "simulate_invoke");
        assert_eq!(
            parsed.steps[0]
                .params
                .get("contract_id")
                .and_then(|v| v.as_str()),
            Some("CABC")
        );
        assert_eq!(
            parsed.steps[0]
                .params
                .get("function")
                .and_then(|v| v.as_str()),
            Some("transfer")
        );
        let args_seq = parsed.steps[0]
            .params
            .get("args")
            .and_then(|v| v.as_sequence())
            .expect("args is a sequence");
        assert_eq!(args_seq.len(), 4, "two flag/value pairs");
    }

    #[test]
    fn generated_step_with_zero_args_parses() {
        let step = render_step("CABC", "decimals", &[]);
        let scenario = format!(
            "schema_version: \"1.0\"\nname: generated\nnetwork: testnet\nsteps:\n{}",
            indent(&step)
        );
        let parsed = Scenario::from_str(&scenario).expect("zero-arg step must validate");
        assert_eq!(parsed.steps.len(), 1);
    }

    /// Indent every non-comment line by two spaces so the step nests under
    /// `steps:`. Comments keep their original indentation.
    fn indent(step: &str) -> String {
        step.lines()
            .map(|l| {
                if l.trim_start().starts_with('#') {
                    format!("{l}\n")
                } else {
                    format!("  {l}\n")
                }
            })
            .collect()
    }

    #[test]
    fn no_contract_in_scenario_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.yml");
        let scenario = Scenario::from_str(
            r#"schema_version: "1.0"
name: s
network: testnet
steps:
  - action: assert_asset
    identity: alice
"#,
        )
        .unwrap();
        let r = ScaffoldResult::execute(
            &scenario,
            &path,
            None,
            "balance",
            std::path::Path::new("/nonexistent/stellar"),
        );
        assert_eq!(r.status, ScaffoldStatus::NoContractInScenario);
        assert!(r.diagnostic.contains("no inspect_contract step"));
    }

    #[test]
    fn ambiguous_contract_lists_the_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.yml");
        let scenario = Scenario::from_str(
            r#"schema_version: "1.0"
name: s
network: testnet
steps:
  - action: inspect_contract
    contract_id: CAAA
  - action: inspect_contract
    contract_id: CBBB
"#,
        )
        .unwrap();
        let r = ScaffoldResult::execute(
            &scenario,
            &path,
            None,
            "balance",
            std::path::Path::new("/nonexistent/stellar"),
        );
        assert_eq!(r.status, ScaffoldStatus::AmbiguousContract);
        assert!(r.diagnostic.contains("CAAA"), "{}", r.diagnostic);
        assert!(r.diagnostic.contains("CBBB"), "{}", r.diagnostic);
    }

    #[test]
    fn contract_filter_that_matches_nothing_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.yml");
        let scenario = Scenario::from_str(
            r#"schema_version: "1.0"
name: s
network: testnet
steps:
  - action: inspect_contract
    contract_id: CAAA
"#,
        )
        .unwrap();
        let r = ScaffoldResult::execute(
            &scenario,
            &path,
            Some("CZZZ"),
            "balance",
            std::path::Path::new("/nonexistent/stellar"),
        );
        assert_eq!(r.status, ScaffoldStatus::NoContractInScenario);
        assert!(r.diagnostic.contains("CZZZ"));
    }

    #[test]
    fn contract_filter_disambiguates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.yml");
        let scenario = Scenario::from_str(
            r#"schema_version: "1.0"
name: s
network: testnet
steps:
  - action: inspect_contract
    contract_id: CAAA
  - action: inspect_contract
    contract_id: CBBB
"#,
        )
        .unwrap();
        let r = ScaffoldResult::execute(
            &scenario,
            &path,
            Some("CBBB"),
            "balance",
            std::path::Path::new("/nonexistent/stellar"),
        );
        // Reaches the inspect step, which fails because the CLI is absent.
        assert_eq!(r.status, ScaffoldStatus::InspectFailed);
        assert_eq!(r.contract_id, "CBBB");
    }

    #[test]
    fn missing_cli_is_reported_as_inspect_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.yml");
        let scenario = Scenario::from_str(
            r#"schema_version: "1.0"
name: s
network: testnet
steps:
  - action: inspect_contract
    contract_id: CAAA
"#,
        )
        .unwrap();
        let r = ScaffoldResult::execute(
            &scenario,
            &path,
            None,
            "balance",
            std::path::Path::new("/nonexistent/stellar"),
        );
        assert_eq!(r.status, ScaffoldStatus::InspectFailed);
        assert!(r.yaml.is_none(), "no YAML on failure");
    }
}
