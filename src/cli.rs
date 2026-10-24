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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    Json,
    Text,
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json => write!(f, "json"),
            Self::Text => write!(f, "text"),
        }
    }
}
