//! Binary entry point for `stellar-scenario`.

use clap::Parser;
use std::path::Path;
use std::process::ExitCode;

use stellar_scenario::cli::{Cli, Command, OutputFormat};
use stellar_scenario::plan::ExecutionPlan;
use stellar_scenario::reporter::ValidationReport;
use stellar_scenario::runner::find_stellar_cli;
use stellar_scenario::scenario::Scenario;
use stellar_scenario::stellar;

fn main() -> ExitCode {
    let args = Cli::parse();

    match args.command {
        Command::Validate { file, output } => run_validate(&file, output),
        Command::Plan { file, output } => run_plan(&file, output),
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
