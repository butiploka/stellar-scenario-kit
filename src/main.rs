//! Binary entry point for `stellar-scenario`.

use clap::Parser;
use std::path::Path;
use std::process::ExitCode;

use stellar_scenario::assert::{
    AssertBalanceResult, AssertionMode, ScenarioReport, ScenarioStatus, SkipReason, SkippedStep,
};
use stellar_scenario::cli::{Cli, Command, OutputFormat};
use stellar_scenario::inspect::{InspectReport, InspectReportStatus, InspectResult};
use stellar_scenario::plan::{ExecutionPlan, SideEffect};
use stellar_scenario::reporter::ValidationReport;
use stellar_scenario::runner::find_stellar_cli;
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
        Command::Doctor { output } => run_doctor(output),
    }
}

fn run_validate(file: &Path, output: OutputFormat) -> ExitCode {
    let (report, code) = match Scenario::from_file(file) {
        Ok(scenario) => (ValidationReport::valid(file, &scenario), 0),
        Err(e) => (ValidationReport::invalid(file, &e), e.exit_code()),
    };

    match output {
        OutputFormat::Json => println!("{}", report.to_json()),
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
                OutputFormat::Json => println!("{}", report.to_json()),
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
        OutputFormat::Json => println!("{}", plan.to_json()),
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
            });
            continue;
        }
        if AssertionMode::for_action(&step.action).is_none() {
            skipped.push(SkippedStep {
                step_index: idx,
                action: step.action.clone(),
                reason: SkipReason::NotExecutable,
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
                OutputFormat::Json => println!("{}", report.to_json()),
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
        OutputFormat::Json => println!("{}", report.to_json()),
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

fn run_doctor(output: OutputFormat) -> ExitCode {
    let diagnostics = stellar::diagnose();
    let all_ok = diagnostics.iter().all(|d| d.ok);

    match output {
        OutputFormat::Json => {
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
