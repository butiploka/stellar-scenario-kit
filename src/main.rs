//! Binary entry point for `stellar-scenario`.

use clap::Parser;
use std::path::Path;
use std::process::ExitCode;

use stellar_scenario::assert::{
    AssertBalanceResult, AssertionMode, ScenarioReport, ScenarioStatus, SkipReason, SkippedStep,
};
use stellar_scenario::cli::{Cli, Command, OutputFormat};
use stellar_scenario::inspect::{InspectReport, InspectReportStatus, InspectResult};
use stellar_scenario::plan::{ExecutionPlan, ExecutionStatus, SideEffect};
use stellar_scenario::reporter::ValidationReport;
use stellar_scenario::runner::find_stellar_cli;
use stellar_scenario::scaffold::{ScaffoldResult, ScaffoldStatus};
use stellar_scenario::scenario::Scenario;
use stellar_scenario::simulate::{SimulationReport, SimulationReportStatus, SimulationResult};
use stellar_scenario::stellar;

fn main() -> ExitCode {
    let args = Cli::parse();

    match args.command {
        Command::Validate { file, output } => run_validate(&file, output),
        Command::Plan { file, output } => run_plan(&file, output),
        Command::Assert { file, output } => run_assert(&file, output),
        Command::Simulate { file, output } => run_simulate(&file, output),
        Command::Inspect { file, output } => run_inspect(&file, output),
        Command::Scaffold {
            file,
            contract_id,
            function,
            output,
        } => run_scaffold(&file, contract_id.as_deref(), function, output),
        Command::Doctor { output } => run_doctor(output),
    }
}

fn run_validate(file: &Path, output: OutputFormat) -> ExitCode {
    let (report, code) = match Scenario::from_file(file) {
        Ok(scenario) => (ValidationReport::valid(file, &scenario), 0),
        Err(e) => (ValidationReport::invalid(file, &e), e.exit_code()),
    };

    match output {
        OutputFormat::Json | OutputFormat::Jsonl => println!("{}", report.to_json()),
        OutputFormat::Text => eprintln!("{}", report.to_text()),
    }

    ExitCode::from(code as u8)
}

fn run_plan(file: &Path, output: OutputFormat) -> ExitCode {
    let scenario = match Scenario::from_file(file) {
        Ok(s) => s,
        Err(e) => {
            let report = ValidationReport::invalid(file, &e);
            match output {
                OutputFormat::Json | OutputFormat::Jsonl => println!("{}", report.to_json()),
                OutputFormat::Text => eprintln!("{}", report.to_text()),
            }
            return ExitCode::from(e.exit_code() as u8);
        }
    };

    let executable = find_stellar_cli(None)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "stellar".to_string());

    let plan = ExecutionPlan::build(&scenario, &executable);

    match output {
        OutputFormat::Json | OutputFormat::Jsonl => println!("{}", plan.to_json()),
        OutputFormat::Text => eprintln!("{}", plan.to_text()),
    }

    ExitCode::SUCCESS
}

fn run_assert(file: &Path, output: OutputFormat) -> ExitCode {
    let scenario = match Scenario::from_file(file) {
        Ok(s) => s,
        Err(e) => {
            let report = ValidationReport::invalid(file, &e);
            match output {
                OutputFormat::Json => println!("{}", report.to_json()),
                // JSONL keeps stdout for the step event stream; a scenario
                // that never parsed produced no events, so the error report
                // goes to the stderr aggregate channel instead.
                OutputFormat::Jsonl => eprintln!("{}", report.to_compact_json()),
                OutputFormat::Text => eprintln!("{}", report.to_text()),
            }
            return ExitCode::from(e.exit_code() as u8);
        }
    };

    let executable = match find_stellar_cli(None) {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("stellar CLI unavailable: {e}");
            match output {
                OutputFormat::Json => println!("{{\"error\":\"{msg}\"}}"),
                // No steps ran, so nothing belongs on the stdout event stream.
                OutputFormat::Jsonl => eprintln!("{{\"error\":\"{msg}\"}}"),
                OutputFormat::Text => eprintln!("error: {msg}"),
            }
            return ExitCode::from(2);
        }
    };

    let plan = ExecutionPlan::build(&scenario, &executable.display().to_string());

    let mut results: Vec<AssertBalanceResult> = Vec::new();
    let mut skipped: Vec<SkippedStep> = Vec::new();

    for (idx, step) in scenario.steps.iter().enumerate() {
        let planned = &plan.commands[idx];
        if planned.side_effect != SideEffect::ReadOnly {
            skipped.push(SkippedStep {
                step_index: idx,
                action: step.action.clone(),
                reason: SkipReason::WouldMutate,
                execution_status: ExecutionStatus::WouldMutate,
                block_reason: Some("would mutate ledger state; blocked in plan mode".to_string()),
            });
            continue;
        }
        if AssertionMode::for_action(&step.action).is_none() {
            skipped.push(SkippedStep {
                step_index: idx,
                action: step.action.clone(),
                reason: SkipReason::NotExecutable,
                execution_status: ExecutionStatus::Executable,
                block_reason: None,
            });
            continue;
        }
        results.push(AssertBalanceResult::execute(
            &scenario,
            idx,
            step,
            &executable,
        ));
    }

    let report = ScenarioReport::new(&scenario, results, skipped);

    match output {
        OutputFormat::Json => println!("{}", report.to_json()),
        // stdout gets the per-step JSONL event stream; the aggregate report
        // (with step_counts) is emitted on stderr.
        OutputFormat::Jsonl => {
            print!("{}", report.to_jsonl_events());
            eprintln!("{}", report.to_compact_json());
        }
        OutputFormat::Text => eprintln!("{}", report.to_text()),
    }

    match report.status {
        ScenarioStatus::Errored => ExitCode::from(2),
        ScenarioStatus::Failed => ExitCode::from(1),
        ScenarioStatus::Passed | ScenarioStatus::NoAssertions => ExitCode::SUCCESS,
    }
}

fn run_inspect(file: &Path, output: OutputFormat) -> ExitCode {
    let scenario = match Scenario::from_file(file) {
        Ok(s) => s,
        Err(e) => {
            let report = ValidationReport::invalid(file, &e);
            match output {
                OutputFormat::Json | OutputFormat::Jsonl => println!("{}", report.to_json()),
                OutputFormat::Text => eprintln!("{}", report.to_text()),
            }
            return ExitCode::from(e.exit_code() as u8);
        }
    };

    let executable = match find_stellar_cli(None) {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("stellar CLI unavailable: {e}");
            match output {
                OutputFormat::Json | OutputFormat::Jsonl => println!("{{\"error\":\"{msg}\"}}"),
                OutputFormat::Text => eprintln!("error: {msg}"),
            }
            return ExitCode::from(2);
        }
    };

    let plan = ExecutionPlan::build(&scenario, &executable.display().to_string());
    let timeout = scenario.timeout_duration();

    let mut results: Vec<InspectResult> = Vec::new();
    let mut skipped: usize = 0;

    for (idx, step) in scenario.steps.iter().enumerate() {
        let planned = &plan.commands[idx];
        if planned.side_effect != SideEffect::ReadOnly {
            skipped += 1;
            continue;
        }
        if step.action != "inspect_contract" {
            skipped += 1;
            continue;
        }
        let contract_id = step
            .params
            .get("contract_id")
            .and_then(|v| v.as_str())
            .unwrap_or("<missing>");
        results.push(InspectResult::execute(
            contract_id,
            &scenario.network,
            &executable,
            timeout,
        ));
    }

    let report = InspectReport::new(&scenario, results, skipped);

    match output {
        OutputFormat::Json | OutputFormat::Jsonl => println!("{}", report.to_json()),
        OutputFormat::Text => eprintln!("{}", report.to_text()),
    }

    match report.status {
        InspectReportStatus::FetchFailed => ExitCode::from(2),
        InspectReportStatus::ParseFailed => ExitCode::from(2),
        InspectReportStatus::Inspected => ExitCode::SUCCESS,
        // No contract was inspected. That is a success only if the scenario
        // had no inspect step to begin with; if it did and none ran, the CLI
        // was missing and the caller needs to know.
        InspectReportStatus::NoContracts => {
            if report.total_steps > 0 && report.results.is_empty() && skipped > 0 {
                ExitCode::from(2)
            } else {
                ExitCode::SUCCESS
            }
        }
    }
}

/// Pick the single contract a scenario inspects.
///
/// Returns the contract id, or an actionable message when the scenario has
/// no inspect step, none matching `--contract-id`, or more than one.
fn select_contract(
    scenario: &Scenario,
    file: &Path,
    contract_id: Option<&str>,
) -> Result<String, String> {
    let mut ids: Vec<String> = scenario
        .steps
        .iter()
        .filter(|s| s.action == "inspect_contract")
        .filter_map(|s| s.params.get("contract_id").and_then(|v| v.as_str()))
        .map(String::from)
        .collect();

    if let Some(filter) = contract_id {
        ids.retain(|id| id == filter);
        if ids.is_empty() {
            return Err(format!(
                "no inspect_contract step matches --contract-id {filter}; add one to {}",
                file.display()
            ));
        }
    }

    ids.dedup();
    match ids.len() {
        0 => Err(format!(
            "{} has no inspect_contract step; add one to select a contract",
            file.display()
        )),
        1 => Ok(ids.remove(0)),
        _ => Err(format!(
            "{} inspects {} contracts; pass --contract-id to pick one: {}",
            file.display(),
            ids.len(),
            ids.join(", ")
        )),
    }
}

fn run_scaffold(
    file: &Path,
    contract_id: Option<&str>,
    function: String,
    output: OutputFormat,
) -> ExitCode {
    let scenario = match Scenario::from_file(file) {
        Ok(s) => s,
        Err(e) => {
            let report = ValidationReport::invalid(file, &e);
            match output {
                OutputFormat::Json | OutputFormat::Jsonl => println!("{}", report.to_json()),
                OutputFormat::Text => eprintln!("{}", report.to_text()),
            }
            return ExitCode::from(e.exit_code() as u8);
        }
    };

    // The scenario is the primary input, so its shape is checked before the
    // CLI is touched. Otherwise a scenario with no inspect step would be
    // reported as "stellar CLI unavailable" on a host without the CLI,
    // hiding the actual problem.
    let selected = match select_contract(&scenario, file, contract_id) {
        Ok(id) => id,
        Err(msg) => {
            let status = if contract_id.is_some() {
                "no_contract_in_scenario"
            } else if scenario
                .steps
                .iter()
                .any(|s| s.action == "inspect_contract")
            {
                "ambiguous_contract"
            } else {
                "no_contract_in_scenario"
            };
            match output {
                OutputFormat::Json | OutputFormat::Jsonl => println!(
                    "{}",
                    serde_json::json!({
                        "scenario": scenario.name,
                        "schema_version": scenario.schema_version,
                        "network": scenario.network,
                        "status": status,
                        "diagnostic": msg,
                        "args": [],
                    })
                ),
                OutputFormat::Text => eprintln!("error: {msg}"),
            }
            return ExitCode::from(1);
        }
    };

    let executable = match find_stellar_cli(None) {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("stellar CLI unavailable: {e}");
            // Classify the missing-CLI failure so the JSON carries the same
            // diagnostic_category and remediation the other commands report.
            let diag = stellar_scenario::diagnostic::Diagnostic::classify_runner(&e);
            let result = ScaffoldResult {
                scenario: scenario.name.clone(),
                network: scenario.network.clone(),
                contract_id: selected.clone(),
                function: function.clone(),
                status: ScaffoldStatus::InspectFailed,
                args: Vec::new(),
                yaml: None,
                diagnostic: stellar_scenario::simulate::redact(&msg),
                diagnostic_category: Some(diag.category.as_str().to_string()),
                remediation: Some(diag.remediation.clone()),
            };
            match output {
                OutputFormat::Json | OutputFormat::Jsonl => println!(
                    "{}",
                    serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string())
                ),
                OutputFormat::Text => eprintln!("error: {msg}"),
            }
            return ExitCode::from(2);
        }
    };

    let result = ScaffoldResult::execute(&scenario, file, Some(&selected), &function, &executable);

    match output {
        OutputFormat::Json | OutputFormat::Jsonl => {
            println!(
                "{}",
                serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string())
            );
        }
        OutputFormat::Text => {
            // The YAML is the payload, so it goes to stdout; the diagnostic
            // goes to stderr. That keeps `scaffold ... > step.yml` clean.
            if let Some(yaml) = &result.yaml {
                print!("{yaml}");
            }
            eprintln!("{}: {}", result.function, result.diagnostic);
        }
    }

    match result.status {
        ScaffoldStatus::Generated => ExitCode::SUCCESS,
        // The request was well-formed but the contract does not offer it.
        ScaffoldStatus::FunctionNotFound | ScaffoldStatus::UnsupportedType => ExitCode::from(1),
        // Ambiguity or a missing selection is a usage problem.
        ScaffoldStatus::NoContractInScenario | ScaffoldStatus::AmbiguousContract => {
            ExitCode::from(1)
        }
        // The interface could not be obtained.
        ScaffoldStatus::InspectFailed | ScaffoldStatus::Error => ExitCode::from(2),
    }
}

fn run_doctor(output: OutputFormat) -> ExitCode {
    let diagnostics = stellar::diagnose();
    let all_ok = diagnostics.iter().all(|d| d.ok);

    match output {
        OutputFormat::Json | OutputFormat::Jsonl => {
            println!(
                "{}",
                serde_json::to_string_pretty(&diagnostics).unwrap_or_default()
            );
        }
        OutputFormat::Text => {
            for d in &diagnostics {
                let mark = if d.ok { "ok" } else { "FAIL" };
                eprintln!("{mark}\t{}\t{}", d.name, d.detail);
            }
        }
    }

    if all_ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn run_simulate(file: &Path, output: OutputFormat) -> ExitCode {
    let scenario = match Scenario::from_file(file) {
        Ok(s) => s,
        Err(e) => {
            let report = ValidationReport::invalid(file, &e);
            match output {
                OutputFormat::Json => println!("{}", report.to_json()),
                OutputFormat::Jsonl => eprintln!("{}", report.to_compact_json()),
                OutputFormat::Text => eprintln!("{}", report.to_text()),
            }
            return ExitCode::from(e.exit_code() as u8);
        }
    };

    let executable = match find_stellar_cli(None) {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("stellar CLI unavailable: {e}");
            match output {
                OutputFormat::Json => println!("{{\"error\":\"{msg}\"}}"),
                OutputFormat::Jsonl => eprintln!("{{\"error\":\"{msg}\"}}"),
                OutputFormat::Text => eprintln!("error: {msg}"),
            }
            return ExitCode::from(2);
        }
    };

    let plan = ExecutionPlan::build(&scenario, &executable.display().to_string());

    let mut results: Vec<SimulationResult> = Vec::new();
    let mut skipped: usize = 0;

    for (idx, step) in scenario.steps.iter().enumerate() {
        let planned = &plan.commands[idx];
        if planned.side_effect != SideEffect::ReadOnly {
            skipped += 1;
            continue;
        }
        if step.action != "simulate_invoke" {
            skipped += 1;
            continue;
        }
        results.push(SimulationResult::execute(&scenario, idx, step, &executable));
    }

    let report = SimulationReport::new(&scenario, results, skipped);

    match output {
        OutputFormat::Jsonl => {
            print!("{}", report.to_jsonl_events());
            eprintln!("{}", report.to_compact_json());
        }
        OutputFormat::Json => println!("{}", report.to_json()),
        OutputFormat::Text => eprintln!("{}", report.to_text()),
    }

    match report.status {
        SimulationReportStatus::Failed => ExitCode::from(2),
        SimulationReportStatus::UnsupportedOperation => ExitCode::from(1),
        SimulationReportStatus::Simulated => ExitCode::SUCCESS,
        SimulationReportStatus::NoSimulations => ExitCode::SUCCESS,
    }
}
