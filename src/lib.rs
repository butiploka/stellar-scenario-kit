//! Stellar Scenario Kit — a local-first toolkit for defining, validating,
//! and executing reproducible Stellar/Soroban development scenarios.
//!
//! # Exit code contract
//!
//! The `stellar-scenario` binary uses these codes:
//!
//! | Code | Meaning |
//! |------|---------|
//! | 0 | Success — scenario is valid, or every assertion passed |
//! | 1 | Validation failed, or an assertion failed |
//! | 2 | I/O error, YAML parse error, assertion query error, or internal error |

pub mod assert;
pub mod cli;
pub mod error;
pub mod plan;
pub mod reporter;
pub mod runner;
pub mod scenario;
pub mod simulate;
pub mod stellar;

/// Crate version, taken from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
