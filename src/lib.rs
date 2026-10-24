//! Stellar Scenario Kit — a local-first toolkit for defining, validating,
//! and (in later milestones) executing reproducible Stellar/Soroban
//! development scenarios.
//!
//! # Exit code contract
//!
//! The `stellar-scenario` binary uses these codes:
//!
//! | Code | Meaning |
//! |------|---------|
//! | 0 | Success — scenario is valid |
//! | 1 | Validation failed — the file parsed but broke a schema or semantic rule |
//! | 2 | I/O error, YAML parse error, or unexpected internal error |

pub mod cli;
pub mod error;
pub mod plan;
pub mod reporter;
pub mod runner;
pub mod scenario;
pub mod stellar;

/// Crate version, taken from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
