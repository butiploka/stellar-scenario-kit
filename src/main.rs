//! Binary entry point for `stellar-scenario`.

use clap::Parser;
use std::path::Path;
use std::process::ExitCode;

use stellar_scenario::cli::{Cli, Command, OutputFormat};
use stellar_scenario::reporter::ValidationReport;
use stellar_scenario::scenario::Scenario;

fn main() -> ExitCode {
    let args = Cli::parse();

    match args.command {
        Command::Validate { file, output } => run_validate(&file, output),
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
