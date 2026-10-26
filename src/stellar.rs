//! Stellar CLI adapter: discovery, version probing, and read-only diagnostics.
//!
//! Every function in this module is read-only. Nothing here submits
//! transactions, funds accounts, or mutates remote state. The commands used
//! are limited to `stellar --version`, `stellar keys ls`, `stellar network ls`,
//! and `stellar network settings`.

use std::path::PathBuf;
use std::time::Duration;

use crate::runner::{self, CommandOutput, RunnerError};

/// The oldest `stellar` CLI release this toolkit was verified against.
///
/// Every diagnostic matcher, envelope shape, and JSON error `type` in this
/// crate was reproduced against this release. A newer CLI is expected to keep
/// working; an older one is not, and `doctor` reports it.
pub const MINIMUM_CLI_VERSION: &str = "28.1.0";

/// Errors produced by the Stellar CLI adapter.
#[derive(Debug, thiserror::Error)]
pub enum StellarError {
    /// The Stellar CLI executable could not be located.
    #[error("stellar CLI not available: {detail}")]
    CliNotFound {
        /// Explanation of where the search failed.
        detail: String,
    },

    /// The Stellar CLI ran but failed.
    #[error("stellar CLI error: {0}")]
    Runner(#[from] RunnerError),
}

/// A resolved Stellar CLI installation.
#[derive(Debug, Clone)]
pub struct StellarCli {
    /// Path to the executable.
    pub executable: PathBuf,
    /// Version string reported by `stellar --version`.
    pub version: String,
}

impl StellarCli {
    /// Locate the Stellar CLI and probe its version.
    ///
    /// Uses `STELLAR_CLI` if set, otherwise `stellar` on `PATH`.
    pub fn discover() -> Result<Self, StellarError> {
        let executable = runner::find_stellar_cli(None).map_err(|e| StellarError::CliNotFound {
            detail: e.to_string(),
        })?;

        let out = runner::run(
            executable.as_os_str(),
            &[std::ffi::OsStr::new("--version")],
            Duration::from_secs(30),
        )?;

        Ok(Self {
            executable,
            version: out.stdout,
        })
    }

    /// List configured identity names.
    ///
    /// Returns only names. Secret keys are never requested or returned.
    pub fn list_identities(&self) -> Result<Vec<String>, StellarError> {
        let out = self.run_readonly(&["keys", "ls"])?;
        Ok(out
            .stdout
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect())
    }

    /// List configured network names.
    pub fn list_networks(&self) -> Result<Vec<String>, StellarError> {
        let out = self.run_readonly(&["network", "ls"])?;
        Ok(out
            .stdout
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect())
    }

    /// Fetch network configuration settings as raw JSON.
    pub fn network_settings(&self, network: &str) -> Result<String, StellarError> {
        let args = vec![
            std::ffi::OsStr::new("network"),
            std::ffi::OsStr::new("settings"),
            std::ffi::OsStr::new("--network"),
            std::ffi::OsStr::new(network),
        ];
        Ok(self.run_readonly_os(&args)?.stdout)
    }

    /// Run a read-only subcommand and return its output.
    fn run_readonly(&self, args: &[&str]) -> Result<CommandOutput, StellarError> {
        let os_args: Vec<&std::ffi::OsStr> = args.iter().map(|a| a.as_ref()).collect();
        self.run_readonly_os(&os_args)
    }

    fn run_readonly_os(&self, args: &[&std::ffi::OsStr]) -> Result<CommandOutput, StellarError> {
        Ok(runner::run(
            self.executable.as_os_str(),
            args,
            Duration::from_secs(60),
        )?)
    }
}

/// A single environment diagnostic check.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Diagnostic {
    /// Human-readable check name.
    pub name: String,
    /// Whether the check passed.
    pub ok: bool,
    /// Detail message. Must never contain secrets.
    pub detail: String,
}

/// Extract a `MAJOR.MINOR.PATCH` version from a `stellar --version` line.
///
/// The CLI prints e.g. `stellar 28.1.0 (c0f4d0da891bbf214c08b8c5035ae6db80e9a3bd)`.
/// Returns the first token that looks like a dotted triplet, optionally with a
/// hyphenated pre-release suffix (`28.1.0-rc1`). Returns `None` when no token
/// matches.
pub fn extract_version(text: &str) -> Option<String> {
    for token in text.split_whitespace() {
        // Skip leading non-digits so "(v28.1.0)" also resolves.
        let candidate = token.trim_start_matches(|c: char| !c.is_ascii_digit());
        if candidate.is_empty() {
            continue;
        }
        // The numeric core must be exactly N.N.N.
        let core = candidate.split('-').next().unwrap_or(candidate);
        if core.chars().filter(|&c| c == '.').count() != 2 {
            continue;
        }
        if !core.chars().all(|c| c.is_ascii_digit() || c == '.') {
            continue;
        }
        // Any pre-release suffix may be alphanumeric (e.g. -rc1).
        if !candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        {
            continue;
        }
        return Some(candidate.to_string());
    }
    None
}

/// Compare two `MAJOR.MINOR.PATCH` versions (ignoring any pre-release suffix).
///
/// Returns `true` when `installed >= required`. Unparseable input returns
/// `false` — callers treat that as "not compatible".
pub fn version_at_least(installed: &str, required: &str) -> bool {
    fn parts(v: &str) -> Option<(u64, u64, u64)> {
        let core = v.split('-').next().unwrap_or(v);
        let mut it = core.split('.');
        let major = it.next()?.parse().ok()?;
        let minor = it.next()?.parse().ok()?;
        let patch = it.next()?.parse().ok()?;
        if it.next().is_some() {
            return None;
        }
        Some((major, minor, patch))
    }
    match (parts(installed), parts(required)) {
        (Some(a), Some(b)) => a >= b,
        _ => false,
    }
}

/// Run environment diagnostics and return structured results.
///
/// This never mutates state and never prints secret material.
pub fn diagnose() -> Vec<Diagnostic> {
    let mut results = Vec::new();

    match StellarCli::discover() {
        Ok(cli) => {
            results.push(Diagnostic {
                name: "stellar_cli".into(),
                ok: true,
                detail: format!("found at {}", cli.executable.display()),
            });
            results.push(Diagnostic {
                name: "stellar_cli_version".into(),
                ok: true,
                detail: cli.version.lines().next().unwrap_or("unknown").to_string(),
            });
            let version_ok = extract_version(&cli.version)
                .map(|v| version_at_least(&v, MINIMUM_CLI_VERSION))
                .unwrap_or(false);
            results.push(Diagnostic {
                name: "stellar_cli_version_ok".into(),
                ok: version_ok,
                detail: match extract_version(&cli.version) {
                    Some(v) => format!("requires >= {MINIMUM_CLI_VERSION}; found {v}"),
                    None => format!(
                        "could not parse version from {}",
                        cli.version.lines().next().unwrap_or("unknown")
                    ),
                },
            });

            match cli.list_networks() {
                Ok(networks) if !networks.is_empty() => {
                    results.push(Diagnostic {
                        name: "networks_configured".into(),
                        ok: true,
                        detail: format!("{} network(s): {}", networks.len(), networks.join(", ")),
                    });
                }
                Ok(_) => results.push(Diagnostic {
                    name: "networks_configured".into(),
                    ok: false,
                    detail: "no networks configured".into(),
                }),
                Err(e) => results.push(Diagnostic {
                    name: "networks_configured".into(),
                    ok: false,
                    detail: format!("stellar network ls failed: {e}"),
                }),
            }

            match cli.list_identities() {
                Ok(names) => {
                    results.push(Diagnostic {
                        name: "identities_configured".into(),
                        ok: true,
                        detail: format!("{} identity name(s) present", names.len()),
                    });
                }
                Err(e) => results.push(Diagnostic {
                    name: "identities_configured".into(),
                    ok: false,
                    detail: format!("stellar keys ls failed: {e}"),
                }),
            }
        }
        Err(e) => {
            results.push(Diagnostic {
                name: "stellar_cli".into(),
                ok: false,
                detail: e.to_string(),
            });
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Stellar CLI is not installed on every CI runner. Tests that need
    /// it must skip cleanly rather than fail, so `cargo test --workspace`
    /// stays green in a bare environment.
    fn cli_or_skip() -> Option<StellarCli> {
        match StellarCli::discover() {
            Ok(cli) => Some(cli),
            Err(_) => {
                eprintln!("skipping: stellar CLI not available on this host");
                None
            }
        }
    }

    #[test]
    fn discover_finds_installed_cli() {
        let Some(cli) = cli_or_skip() else {
            return;
        };
        assert!(cli.executable.exists());
        assert!(cli.version.contains("stellar"));
    }

    #[test]
    fn list_networks_returns_known_profiles() {
        let Some(cli) = cli_or_skip() else {
            return;
        };
        let networks = cli.list_networks().unwrap();
        assert!(
            networks.contains(&"testnet".to_string()),
            "got {networks:?}"
        );
    }

    #[test]
    fn list_identities_returns_names_only() {
        let Some(cli) = cli_or_skip() else {
            return;
        };
        let names = cli.list_identities().unwrap();
        // Names must never look like secret keys.
        for n in &names {
            assert!(
                !n.starts_with('S'),
                "identity name {n} looks like a secret key"
            );
        }
    }

    #[test]
    fn diagnose_reports_structured_results() {
        let results = diagnose();
        assert!(!results.is_empty());
        assert!(results.iter().any(|d| d.name == "stellar_cli"));
        for d in &results {
            assert!(!d.detail.is_empty());
        }
    }

    #[test]
    fn extract_version_pulls_the_dotted_triplet() {
        assert_eq!(
            extract_version("stellar 28.1.0 (c0f4d0da891bbf214c08b8c5035ae6db80e9a3bd)"),
            Some("28.1.0".to_string())
        );
        assert_eq!(
            extract_version("stellar 28.1.0"),
            Some("28.1.0".to_string())
        );
        assert_eq!(
            extract_version("stellar 28.1.0-rc1 (abcdef)"),
            Some("28.1.0-rc1".to_string())
        );
        // Leading junk before the version must not defeat extraction.
        assert_eq!(
            extract_version("stellar v28.1.0"),
            Some("28.1.0".to_string())
        );
        assert_eq!(extract_version(""), None);
        assert_eq!(extract_version("stellar"), None);
        // A two-part or four-part version must not be mistaken for one.
        assert_eq!(extract_version("stellar 28.1"), None);
        assert_eq!(extract_version("stellar 28.1.0.1"), None);
    }

    #[test]
    fn version_at_least_compares_major_minor_patch() {
        assert!(version_at_least("28.1.0", "28.1.0"));
        assert!(version_at_least("28.1.1", "28.1.0"));
        assert!(version_at_least("28.2.0", "28.1.0"));
        assert!(version_at_least("29.0.0", "28.1.0"));
        assert!(version_at_least("28.1.0-rc1", "28.1.0"));
        assert!(!version_at_least("28.0.9", "28.1.0"));
        assert!(!version_at_least("27.9.9", "28.1.0"));
        assert!(!version_at_least("28.0.0", "28.1.0"));
    }

    #[test]
    fn version_at_least_rejects_unparseable_input() {
        assert!(!version_at_least("", "28.1.0"));
        assert!(!version_at_least("28.1.0", ""));
        assert!(!version_at_least("abc", "28.1.0"));
        assert!(!version_at_least("28.1.0", "not-a-version"));
        assert!(!version_at_least("28.1", "28.1.0"));
    }

    #[test]
    fn diagnose_emits_version_compatibility_check() {
        let results = diagnose();
        if !results.iter().any(|d| d.name == "stellar_cli") {
            // No CLI on this host; the check is emitted only when the CLI exists.
            return;
        }
        let check = results
            .iter()
            .find(|d| d.name == "stellar_cli_version_ok")
            .expect("diagnose must emit stellar_cli_version_ok when the CLI is present");
        assert!(
            check.detail.contains(MINIMUM_CLI_VERSION),
            "detail must state the required floor, got {:?}",
            check.detail
        );
    }
}
