//! Contract interface inspection.
//!
//! This module implements `stellar-scenario inspect`: read a Soroban
//! contract's interface and turn it into a form a scenario author can use
//! directly. It answers "what functions does this contract take, and what
//! do they return?" without the author having to fetch a spec by hand.
//!
//! # Verified interface (Stellar CLI 28.1.0)
//!
//! ```text
//! stellar contract info interface --contract-id <C...> --network <n> --output json
//! ```
//!
//! Output is a single-line JSON array of `SCSpecEntry` values on stdout, with
//! progress chatter on stderr. Two entry kinds appear in practice:
//! `function_v0` and `event_v0`.
//!
//! # Safety
//!
//! Read-only. The command downloads the contract spec over RPC and changes
//! nothing. No transaction is built, no key is used.

use crate::runner;
use crate::scenario::Scenario;

/// One function or event signature from a contract interface.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Signature {
    /// Entry kind: `function` or `event`.
    pub kind: String,
    /// Function or event name.
    pub name: String,
    /// Parameters, in declaration order.
    pub inputs: Vec<Param>,
    /// Return types. Empty for events and for functions returning nothing.
    pub outputs: Vec<String>,
    /// Documentation string from the spec, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
}

/// A single function or event parameter.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Param {
    /// Parameter name.
    pub name: String,
    /// Soroban type, e.g. `address`, `i128`, `u32`, `string`. Composite
    /// types are rendered in a readable form, e.g. `option<u64>`.
    #[serde(rename = "type")]
    pub param_type: String,
}

/// A contract's full interface.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ContractInterface {
    /// Contract id that was inspected.
    pub contract_id: String,
    /// Network profile used.
    pub network: String,
    /// Functions, in the order the spec listed them.
    pub functions: Vec<Signature>,
    /// Events, in the order the spec listed them.
    pub events: Vec<Signature>,
}

/// Outcome of an inspection attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectStatus {
    /// The interface was fetched and parsed.
    Inspected,
    /// The spec download or the CLI call failed.
    FetchFailed,
    /// The output was not valid JSON.
    ParseFailed,
    /// The JSON was valid but not the expected shape.
    UnexpectedShape,
    /// A subprocess-level error (missing CLI, timeout, spawn failure).
    Error,
}

/// Result of inspecting one contract.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct InspectResult {
    /// Contract id that was requested.
    pub contract_id: String,
    /// Network profile used.
    pub network: String,
    /// Outcome.
    pub status: InspectStatus,
    /// The parsed interface. Present only when `status` is `inspected`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<ContractInterface>,
    /// Human-readable diagnostic. Never contains secret material.
    pub diagnostic: String,
}

impl InspectResult {
    /// Inspect a contract by id.
    pub fn execute(
        contract_id: &str,
        network: &str,
        executable: &std::path::Path,
        timeout: std::time::Duration,
    ) -> Self {
        let args: Vec<&std::ffi::OsStr> = vec![
            std::ffi::OsStr::new("contract"),
            std::ffi::OsStr::new("info"),
            std::ffi::OsStr::new("interface"),
            std::ffi::OsStr::new("--contract-id"),
            std::ffi::OsStr::new(contract_id),
            std::ffi::OsStr::new("--network"),
            std::ffi::OsStr::new(network),
            std::ffi::OsStr::new("--output"),
            std::ffi::OsStr::new("json"),
        ];

        let base = Self {
            contract_id: contract_id.to_string(),
            network: network.to_string(),
            status: InspectStatus::Error,
            interface: None,
            diagnostic: String::new(),
        };

        let output = match runner::run_capture(executable.as_os_str(), &args, timeout) {
            Ok(out) => out,
            Err(e) => return base.with(InspectStatus::Error, format!("stellar CLI error: {e}")),
        };

        if !output.is_success() {
            let detail = crate::assert::first_non_empty(&[&output.stderr, &output.stdout])
                .unwrap_or_else(|| "<no output>".to_string());
            return base.with(
                InspectStatus::FetchFailed,
                format!(
                    "stellar CLI exited with code {}: {detail}",
                    output.exit_code
                ),
            );
        }

        match parse_interface(&output.stdout) {
            Ok(entries) => {
                let functions = entries
                    .iter()
                    .filter(|e| e.kind == "function")
                    .cloned()
                    .collect::<Vec<_>>();
                let events = entries
                    .iter()
                    .filter(|e| e.kind == "event")
                    .cloned()
                    .collect::<Vec<_>>();
                let interface = ContractInterface {
                    contract_id: contract_id.to_string(),
                    network: network.to_string(),
                    functions,
                    events,
                };
                let mut result = base.with(
                    InspectStatus::Inspected,
                    format!(
                        "{} function(s), {} event(s)",
                        interface.functions.len(),
                        interface.events.len()
                    ),
                );
                result.interface = Some(interface);
                result
            }
            Err(e) => {
                let status = if e.starts_with("not JSON") {
                    InspectStatus::ParseFailed
                } else {
                    InspectStatus::UnexpectedShape
                };
                base.with(status, e)
            }
        }
    }

    fn with(mut self, status: InspectStatus, diagnostic: String) -> Self {
        self.status = status;
        self.diagnostic = crate::simulate::redact(&diagnostic);
        self
    }
}

/// Render a Soroban spec type.
///
/// Most types are plain strings (`address`, `i128`, `u32`). Composite types
/// are JSON objects, e.g. `{"option":{"value_type":"u64"}}`. Rendering them
/// as a readable string keeps a parameter's type visible instead of blank;
/// an unknown shape falls back to `unknown` rather than an empty string,
/// because a blank type reads as "no type".
fn render_type(value: Option<&serde_json::Value>) -> String {
    match value {
        None => "unknown".to_string(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Object(obj)) => {
            if let Some(inner) = obj.get("option") {
                return format!("option<{}>", render_type(inner.get("value_type")));
            }
            if let Some(inner) = obj.get("vec") {
                return format!("vec<{}>", render_type(inner.get("element_type")));
            }
            if let Some(inner) = obj.get("map") {
                return format!(
                    "map<{}, {}>",
                    render_type(inner.get("key_type")),
                    render_type(inner.get("value_type"))
                );
            }
            // Unknown composite: keep the JSON so the author can see the shape.
            serde_json::to_string(obj).unwrap_or_else(|_| "unknown".to_string())
        }
        Some(other) => other.to_string(),
    }
}

/// Parse the `SCSpecEntry` array emitted by `stellar contract info interface`.
fn parse_interface(stdout: &str) -> Result<Vec<Signature>, String> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Err("not JSON: empty output".to_string());
    }

    let value: serde_json::Value =
        serde_json::from_str(trimmed).map_err(|e| format!("not JSON: {e}"))?;

    let array = value
        .as_array()
        .ok_or_else(|| "expected a JSON array of spec entries".to_string())?;

    let mut out = Vec::new();
    for entry in array {
        let Some((kind, body)) = split_entry(entry) else {
            continue;
        };
        out.push(entry_to_signature(kind, body)?);
    }
    Ok(out)
}

/// Split an `SCSpecEntry` into its kind and body.
///
/// Only `function_v0` and `event_v0` are recognized. Any other variant is
/// skipped rather than guessed at, so a future spec version does not produce
/// silently wrong signatures.
fn split_entry(entry: &serde_json::Value) -> Option<(&'static str, &serde_json::Value)> {
    let obj = entry.as_object()?;
    for (key, kind) in [("function_v0", "function"), ("event_v0", "event")] {
        if let Some(body) = obj.get(key) {
            return Some((kind, body));
        }
    }
    None
}

fn entry_to_signature(kind: &str, body: &serde_json::Value) -> Result<Signature, String> {
    let name = body
        .get("name")
        .and_then(|n| n.as_str())
        .ok_or_else(|| format!("{kind} entry has no name"))?
        .to_string();

    let doc = body
        .get("doc")
        .and_then(|d| d.as_str())
        .filter(|d| !d.is_empty())
        .map(String::from);

    // Functions carry `inputs`; events carry `params`. Reading only `inputs`
    // silently drops every event parameter, which renders as `event X()` and
    // misleads an author into thinking the event takes no arguments.
    let raw_params = body
        .get("inputs")
        .or_else(|| body.get("params"))
        .and_then(|i| i.as_array());

    let inputs = raw_params
        .map(|arr| {
            arr.iter()
                .map(|p| Param {
                    name: p
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string(),
                    param_type: render_type(p.get("type")),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let outputs = body
        .get("outputs")
        .and_then(|o| o.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(Signature {
        kind: kind.to_string(),
        name,
        inputs,
        outputs,
        doc,
    })
}

// --------------------------------------------------------------------------
// Scenario-level report
// --------------------------------------------------------------------------

/// Overall status of an `inspect` run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectReportStatus {
    /// Every requested contract was inspected.
    Inspected,
    /// At least one contract failed to fetch.
    FetchFailed,
    /// At least one contract produced unparseable or unexpected output.
    ParseFailed,
    /// No contract was inspected.
    NoContracts,
}

/// Aggregate report for an `inspect` run.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct InspectReport {
    /// Scenario name.
    pub scenario: String,
    /// Schema version declared by the scenario file.
    pub schema_version: String,
    /// Network profile the run targeted.
    pub network: String,
    /// Contracts inspected successfully.
    pub inspected: usize,
    /// Contracts that failed to fetch.
    pub failed: usize,
    /// Steps not executed (mutating, or not an inspect step).
    pub skipped: usize,
    /// Total steps declared in the scenario.
    pub total_steps: usize,
    /// Overall status.
    pub status: InspectReportStatus,
    /// Per-contract results, in step order.
    pub results: Vec<InspectResult>,
}

impl InspectReport {
    /// Build a report. Precedence:
    /// `fetch_failed` > `parse_failed` > `inspected` > `no_contracts`.
    ///
    /// A subprocess-level `error` (missing CLI, timeout, spawn failure) is
    /// counted with `fetch_failed`: in both cases the interface was not
    /// obtained, and reporting that as `no_contracts` would hide it.
    pub fn new(scenario: &Scenario, results: Vec<InspectResult>, skipped: usize) -> Self {
        let inspected = results
            .iter()
            .filter(|r| r.status == InspectStatus::Inspected)
            .count();
        let fetch_failed = results
            .iter()
            .filter(|r| matches!(r.status, InspectStatus::FetchFailed | InspectStatus::Error))
            .count();
        let parse_failed = results
            .iter()
            .filter(|r| {
                matches!(
                    r.status,
                    InspectStatus::ParseFailed | InspectStatus::UnexpectedShape
                )
            })
            .count();

        let status = if fetch_failed > 0 {
            InspectReportStatus::FetchFailed
        } else if parse_failed > 0 {
            InspectReportStatus::ParseFailed
        } else if inspected > 0 {
            InspectReportStatus::Inspected
        } else {
            InspectReportStatus::NoContracts
        };

        Self {
            scenario: scenario.name.clone(),
            schema_version: scenario.schema_version.clone(),
            network: scenario.network.clone(),
            inspected,
            failed: results.len() - inspected,
            skipped,
            total_steps: scenario.steps.len(),
            status,
            results,
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
            "{} on {}: {} ({} inspected, {} failed)\n",
            self.scenario,
            self.network,
            status_label(self.status),
            self.inspected,
            self.failed
        ));
        for r in &self.results {
            let mark = match r.status {
                InspectStatus::Inspected => "OK",
                _ => "FAILED",
            };
            out.push_str(&format!("  {mark}\t{}\t{}\n", r.contract_id, r.diagnostic));
            if let Some(iface) = &r.interface {
                for f in &iface.functions {
                    out.push_str(&format!("    fn {}\n", render_signature(f)));
                }
                for e in &iface.events {
                    out.push_str(&format!("    event {}\n", render_signature(e)));
                }
            }
        }
        out
    }
}

fn status_label(status: InspectReportStatus) -> &'static str {
    match status {
        InspectReportStatus::Inspected => "inspected",
        InspectReportStatus::FetchFailed => "fetch failed",
        InspectReportStatus::ParseFailed => "parse failed",
        InspectReportStatus::NoContracts => "no contracts",
    }
}

fn render_signature(sig: &Signature) -> String {
    let params = sig
        .inputs
        .iter()
        .map(|p| format!("{}: {}", p.name, p.param_type))
        .collect::<Vec<_>>()
        .join(", ");
    if sig.outputs.is_empty() {
        format!("{}({params})", sig.name)
    } else {
        format!("{}({params}) -> {}", sig.name, sig.outputs.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed excerpt of real `stellar contract info interface --output json`
    /// output for the native SAC on testnet. Note the shape difference:
    /// functions carry `inputs`, events carry `params`.
    const REAL_SPEC: &str = r#"[{"function_v0":{"doc":"Returns the balance of `id`.","name":"balance","inputs":[{"doc":"","name":"id","type":"address"}],"outputs":["i128"]}},{"function_v0":{"doc":"Returns the number of decimals used to represent amounts of this token.","name":"decimals","inputs":[],"outputs":["u32"]}},{"function_v0":{"doc":"Transfer `amount` from `from` to `to`.","name":"transfer","inputs":[{"doc":"","name":"from","type":"address"},{"doc":"","name":"to","type":"muxed_address"},{"doc":"","name":"amount","type":"i128"}],"outputs":[]}},{"event_v0":{"doc":"","lib":"","name":"Approve","prefix_topics":["approve"],"params":[{"doc":"","name":"from","type":"address","location":"topic_list"},{"doc":"","name":"spender","type":"address","location":"topic_list"},{"doc":"","name":"amount","type":"i128","location":"data"}]}}]"#;

    #[test]
    fn parses_functions_and_events() {
        let entries = parse_interface(REAL_SPEC).expect("real spec should parse");
        let functions: Vec<_> = entries.iter().filter(|e| e.kind == "function").collect();
        let events: Vec<_> = entries.iter().filter(|e| e.kind == "event").collect();
        assert_eq!(functions.len(), 3);
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn parses_parameter_names_and_types() {
        let entries = parse_interface(REAL_SPEC).unwrap();
        let transfer = entries
            .iter()
            .find(|e| e.name == "transfer" && e.kind == "function")
            .expect("transfer function");
        assert_eq!(
            transfer.inputs,
            vec![
                Param {
                    name: "from".into(),
                    param_type: "address".into()
                },
                Param {
                    name: "to".into(),
                    param_type: "muxed_address".into()
                },
                Param {
                    name: "amount".into(),
                    param_type: "i128".into()
                },
            ]
        );
        assert!(transfer.outputs.is_empty(), "transfer returns nothing");
    }

    #[test]
    fn parses_zero_arg_function() {
        let entries = parse_interface(REAL_SPEC).unwrap();
        let decimals = entries.iter().find(|e| e.name == "decimals").unwrap();
        assert!(decimals.inputs.is_empty());
        assert_eq!(decimals.outputs, vec!["u32".to_string()]);
    }

    #[test]
    fn captures_doc_string() {
        let entries = parse_interface(REAL_SPEC).unwrap();
        let balance = entries.iter().find(|e| e.name == "balance").unwrap();
        assert_eq!(balance.doc.as_deref(), Some("Returns the balance of `id`."));
    }

    #[test]
    fn empty_output_is_a_parse_error() {
        let err = parse_interface("").unwrap_err();
        assert!(err.starts_with("not JSON"), "err: {err}");
    }

    #[test]
    fn whitespace_only_output_is_a_parse_error() {
        let err = parse_interface("   \n  ").unwrap_err();
        assert!(err.starts_with("not JSON"), "err: {err}");
    }

    #[test]
    fn malformed_json_is_a_parse_error() {
        let err = parse_interface("[{\"function_v0\":").unwrap_err();
        assert!(err.starts_with("not JSON"), "err: {err}");
    }

    #[test]
    fn non_array_json_is_unexpected_shape() {
        let err = parse_interface("{\"function_v0\":{}}").unwrap_err();
        assert!(!err.starts_with("not JSON"), "err: {err}");
    }

    #[test]
    fn unknown_entry_kinds_are_skipped_not_guessed() {
        // A future spec version adds `function_v1`. It must be skipped rather
        // than parsed with the wrong shape.
        let spec = r#"[{"function_v1":{"name":"x","inputs":[],"outputs":[]}},{"function_v0":{"name":"balance","inputs":[{"name":"id","type":"address"}],"outputs":["i128"]}}]"#;
        let entries = parse_interface(spec).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "balance");
    }

    #[test]
    fn entry_without_name_is_an_error() {
        let spec = r#"[{"function_v0":{"inputs":[],"outputs":[]}}]"#;
        let err = parse_interface(spec).unwrap_err();
        assert!(err.contains("no name"), "err: {err}");
    }

    #[test]
    fn empty_array_is_valid_but_empty() {
        let entries = parse_interface("[]").unwrap();
        assert!(entries.is_empty());
    }

    fn scenario_with(raw: &str) -> Scenario {
        Scenario::from_str(raw).unwrap()
    }

    fn result(scenario: &Scenario, contract_id: &str, status: InspectStatus) -> InspectResult {
        InspectResult {
            contract_id: contract_id.to_string(),
            network: scenario.network.clone(),
            status,
            interface: None,
            diagnostic: "d".into(),
        }
    }

    const TWO: &str = r#"schema_version: "1.0"
name: insp
network: testnet
steps:
  - action: inspect_contract
    contract_id: CABC
  - action: inspect_contract
    contract_id: CDEF
"#;

    #[test]
    fn report_all_inspected() {
        let s = scenario_with(TWO);
        let rep = InspectReport::new(
            &s,
            vec![
                result(&s, "CABC", InspectStatus::Inspected),
                result(&s, "CDEF", InspectStatus::Inspected),
            ],
            0,
        );
        assert_eq!(rep.status, InspectReportStatus::Inspected);
        assert_eq!(rep.inspected, 2);
        assert_eq!(rep.failed, 0);
    }

    #[test]
    fn report_fetch_failed_beats_inspected() {
        let s = scenario_with(TWO);
        let rep = InspectReport::new(
            &s,
            vec![
                result(&s, "CABC", InspectStatus::Inspected),
                result(&s, "CDEF", InspectStatus::FetchFailed),
            ],
            0,
        );
        assert_eq!(rep.status, InspectReportStatus::FetchFailed);
        assert_eq!(rep.inspected, 1);
        assert_eq!(rep.failed, 1);
    }

    #[test]
    fn report_parse_failed_beats_inspected() {
        let s = scenario_with(TWO);
        let rep = InspectReport::new(
            &s,
            vec![
                result(&s, "CABC", InspectStatus::Inspected),
                result(&s, "CDEF", InspectStatus::ParseFailed),
            ],
            0,
        );
        assert_eq!(rep.status, InspectReportStatus::ParseFailed);
    }

    #[test]
    fn report_no_contracts_when_nothing_ran() {
        let s = scenario_with(TWO);
        let rep = InspectReport::new(&s, vec![], 0);
        assert_eq!(rep.status, InspectReportStatus::NoContracts);
    }

    #[test]
    fn parses_event_params_using_the_params_key() {
        // Events carry `params`, not `inputs`. Reading only `inputs` renders
        // every event as `event X()` and hides its arguments.
        let entries = parse_interface(REAL_SPEC).unwrap();
        let approve = entries
            .iter()
            .find(|e| e.kind == "event" && e.name == "Approve")
            .expect("Approve event");
        assert_eq!(
            approve.inputs,
            vec![
                Param {
                    name: "from".into(),
                    param_type: "address".into()
                },
                Param {
                    name: "spender".into(),
                    param_type: "address".into()
                },
                Param {
                    name: "amount".into(),
                    param_type: "i128".into()
                },
            ]
        );
    }

    #[test]
    fn event_with_no_params_stays_empty() {
        let spec = r#"[{"event_v0":{"name":"Empty","params":[]}}]"#;
        let entries = parse_interface(spec).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].inputs.is_empty());
    }

    #[test]
    fn renders_composite_types() {
        assert_eq!(render_type(Some(&serde_json::json!("i128"))), "i128");
        assert_eq!(
            render_type(Some(&serde_json::json!({"option": {"value_type": "u64"}}))),
            "option<u64>"
        );
        assert_eq!(
            render_type(Some(
                &serde_json::json!({"vec": {"element_type": "address"}})
            )),
            "vec<address>"
        );
        assert_eq!(
            render_type(Some(&serde_json::json!({
                "map": {"key_type": "address", "value_type": "i128"}
            }))),
            "map<address, i128>"
        );
        // A missing type must not render as an empty string.
        assert_eq!(render_type(None), "unknown");
        // An unrecognized composite keeps its JSON rather than going blank.
        let weird = render_type(Some(&serde_json::json!({"udt": {"name": "Foo"}})));
        assert!(weird.contains("Foo"), "got {weird}");
    }

    #[test]
    fn event_with_composite_type_keeps_a_visible_type() {
        // Real native SAC shape: `to_muxed_id` is `{"option": {"value_type": "u64"}}`.
        let spec = r#"[{"event_v0":{"name":"Transfer","params":[{"name":"to_muxed_id","type":{"option":{"value_type":"u64"}},"location":"data"}]}}]"#;
        let entries = parse_interface(spec).unwrap();
        assert_eq!(entries[0].inputs[0].param_type, "option<u64>");
    }

    #[test]
    fn report_text_renders_signatures() {
        let s = scenario_with(TWO);
        let entries = parse_interface(REAL_SPEC).unwrap();
        let mut r = result(&s, "CABC", InspectStatus::Inspected);
        r.interface = Some(ContractInterface {
            contract_id: "CABC".into(),
            network: s.network.clone(),
            functions: entries
                .iter()
                .filter(|e| e.kind == "function")
                .cloned()
                .collect(),
            events: entries
                .iter()
                .filter(|e| e.kind == "event")
                .cloned()
                .collect(),
        });
        let rep = InspectReport::new(&s, vec![r], 0);
        let text = rep.to_text();
        assert!(
            text.contains("fn balance(id: address) -> i128"),
            "text: {text}"
        );
        assert!(text.contains("fn decimals() -> u32"), "text: {text}");
        assert!(
            text.contains("fn transfer(from: address, to: muxed_address, amount: i128)"),
            "text: {text}"
        );
        assert!(
            text.contains("event Approve(from: address, spender: address, amount: i128)"),
            "text: {text}"
        );
    }

    #[test]
    fn report_json_marks_status() {
        let s = scenario_with(TWO);
        let rep = InspectReport::new(&s, vec![result(&s, "CABC", InspectStatus::Inspected)], 0);
        let json = rep.to_json();
        assert!(json.contains("\"status\": \"inspected\""));
        assert!(json.contains("\"inspected\": 1"));
    }
}
