//! CLI argument definitions and command dispatch.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "stellar-scenario",
    about = "Local-first toolkit for Stellar/Soroban test scenarios",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Validate a scenario file and emit a JSON report.
    Validate {
        /// Path to a YAML scenario file.
        file: PathBuf,
        /// Output format.
        #[arg(long, value_enum, default_value = "json")]
        output: OutputFormat,
    },

    /// Plan a scenario without running anything.
    ///
    /// Prints the exact Stellar CLI commands that would be executed. No
    /// subprocess is spawned and no network is contacted.
    Plan {
        /// Path to a YAML scenario file.
        file: PathBuf,
        /// Output format.
        #[arg(long, value_enum, default_value = "json")]
        output: OutputFormat,
    },

    /// Execute read-only `assert_balance` steps.
    ///
    /// Only read-only steps are executed. Any step that would mutate remote
    /// state (funding, payments) is reported as skipped, never run.
    Assert {
        /// Path to a YAML scenario file.
        file: PathBuf,
        /// Output format.
        #[arg(long, value_enum, default_value = "json")]
        output: OutputFormat,
    },

    /// Simulate `simulate_invoke` steps without submitting anything.
    ///
    /// Builds a Soroban invoke envelope, simulates it against the network,
    /// and decodes the simulated result. No transaction is ever submitted.
    /// Classic operations (payments, account creation) are rejected by
    /// `stellar tx simulate` and reported as unsupported.
    Simulate {
        /// Path to a YAML scenario file.
        file: PathBuf,
        /// Output format.
        #[arg(long, value_enum, default_value = "json")]
        output: OutputFormat,
    },

    /// Inspect one or more Soroban contract interfaces.
    ///
    /// Runs `stellar contract info interface --output json` for each
    /// `inspect_contract` step and emits a structured report of functions
    /// and events. Read-only; no transaction is built or submitted.
    Inspect {
        /// Path to a YAML scenario file.
        file: PathBuf,
        /// Output format.
        #[arg(long, value_enum, default_value = "json")]
        output: OutputFormat,
    },

    /// Generate a `simulate_invoke` step from a contract interface.
    ///
    /// Reads the contract inspected by the scenario, picks one function,
    /// and prints a YAML step with placeholder arguments. Read-only; no
    /// transaction is built or submitted.
    Scaffold {
        /// Path to a YAML scenario file that contains an `inspect_contract`
        /// step.
        file: PathBuf,
        /// Contract id to scaffold for. Required when the scenario inspects
        /// more than one contract.
        #[arg(long)]
        contract_id: Option<String>,
        /// Function name to scaffold.
        function: String,
        /// Output format.
        #[arg(long, value_enum, default_value = "text")]
        output: OutputFormat,
    },

    /// Report environment readiness.
    ///
    /// Checks whether the Stellar CLI is available and what it reports.
    /// Never prints secret material.
    Doctor {
        /// Output format.
        #[arg(long, value_enum, default_value = "json")]
        output: OutputFormat,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    Json,
    Jsonl,
    Text,
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json => write!(f, "json"),
            Self::Jsonl => write!(f, "jsonl"),
            Self::Text => write!(f, "text"),
        }
    }
}
