//! Process execution adapter for invoking the Stellar CLI.
//!
//! Design rules:
//! - No shell interpolation. Arguments are passed as a vector; the executable
//!   is never resolved through a shell.
//! - No secret material is echoed into error messages by this module.
//! - Deterministic, structured errors for callers.
//! - Timeouts are enforced by a poll-and-kill watchdog, not by signals.

use std::ffi::OsStr;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use thiserror::Error;

/// Default subprocess timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// Environment variable that overrides the Stellar CLI executable.
pub const CLI_ENV_VAR: &str = "STELLAR_CLI";

/// Result of a successful subprocess invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// Exit code reported by the process.
    pub exit_code: i32,
    /// UTF-8 stdout, with one trailing newline removed.
    pub stdout: String,
    /// UTF-8 stderr, with one trailing newline removed.
    pub stderr: String,
}

impl CommandOutput {
    /// True when the process exited with code 0.
    pub fn is_success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Errors produced while running an external command.
#[derive(Debug, Error)]
pub enum RunnerError {
    /// The executable could not be found.
    #[error("stellar CLI not found: {detail}")]
    NotFound {
        /// Where the search was performed.
        detail: String,
    },

    /// The process could not be spawned.
    #[error("failed to start stellar CLI: {source}")]
    SpawnFailed {
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },

    /// The process exceeded its timeout and was killed.
    #[error("stellar CLI timed out after {secs}s")]
    Timeout {
        /// Timeout that was exceeded, in seconds.
        secs: u64,
    },

    /// The process exited with a non-zero status.
    #[error("stellar CLI exited with code {exit_code}: {stderr}")]
    NonZeroExit {
        /// Exit code reported by the process.
        exit_code: i32,
        /// Captured stderr, redacted.
        stderr: String,
    },

    /// Output was not valid UTF-8.
    #[error("stellar CLI produced non-UTF-8 output on {stream}")]
    InvalidOutput {
        /// Which stream failed to decode.
        stream: &'static str,
    },
}

/// Resolve the Stellar CLI executable.
///
/// Search order:
/// 1. Explicit path argument
/// 2. The `STELLAR_CLI` environment variable
/// 3. `stellar` on `PATH`
pub fn find_stellar_cli(explicit: Option<&str>) -> Result<PathBuf, RunnerError> {
    if let Some(p) = explicit {
        if !p.trim().is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    if let Ok(env_path) = std::env::var(CLI_ENV_VAR) {
        if !env_path.trim().is_empty() {
            return Ok(PathBuf::from(env_path));
        }
    }
    if let Some(found) = search_path("stellar") {
        return Ok(found);
    }
    Err(RunnerError::NotFound {
        detail: format!("no explicit path, ${CLI_ENV_VAR} unset, and 'stellar' not on PATH"),
    })
}

/// Search `PATH` for an executable file with the given name.
fn search_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Run a subprocess with the given arguments and timeout.
///
/// The command is executed directly, never through a shell, so arguments
/// containing spaces or metacharacters cannot be reinterpreted.
pub fn run(
    executable: &OsStr,
    args: &[&OsStr],
    timeout: Duration,
) -> Result<CommandOutput, RunnerError> {
    run_with_stdin(executable, args, timeout, None)
}

/// Run a subprocess and return its output regardless of exit status.
///
/// Unlike [`run`], a non-zero exit is not an error: the caller gets the exit
/// code plus both streams so it can interpret them. Use this when the CLI
/// reports structured errors on stdout with a non-zero exit code.
pub fn run_capture(
    executable: &OsStr,
    args: &[&OsStr],
    timeout: Duration,
) -> Result<CommandOutput, RunnerError> {
    run_with_stdin_capture(executable, args, timeout, None)
}

/// Run a subprocess, optionally writing `stdin_data` to its standard input.
pub fn run_with_stdin(
    executable: &OsStr,
    args: &[&OsStr],
    timeout: Duration,
    stdin_data: Option<String>,
) -> Result<CommandOutput, RunnerError> {
    let output = run_with_stdin_capture(executable, args, timeout, stdin_data)?;
    if output.exit_code == 0 {
        Ok(output)
    } else {
        Err(RunnerError::NonZeroExit {
            exit_code: output.exit_code,
            stderr: output.stderr,
        })
    }
}

/// Shared implementation: spawn, drain, enforce the timeout, and return the
/// raw result without judging the exit code.
pub fn run_with_stdin_capture(
    executable: &OsStr,
    args: &[&OsStr],
    timeout: Duration,
    stdin_data: Option<String>,
) -> Result<CommandOutput, RunnerError> {
    let mut cmd = Command::new(executable);
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if stdin_data.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });

    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => RunnerError::NotFound {
            detail: format!("cannot execute {executable:?}"),
        },
        _ => RunnerError::SpawnFailed { source: e },
    })?;

    if let Some(data) = stdin_data {
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write as _;
            let _ = stdin.write_all(data.as_bytes());
            // Dropping stdin closes the pipe, signalling EOF.
        }
    }

    // Drain both pipes on separate threads so a full pipe buffer cannot
    // deadlock the child while we poll for exit.
    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let out_handle = std::thread::spawn(move || read_to_end(&mut out_pipe));
    let err_handle = std::thread::spawn(move || read_to_end(&mut err_pipe));

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = out_handle.join();
                    let _ = err_handle.join();
                    return Err(RunnerError::Timeout {
                        secs: timeout.as_secs(),
                    });
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                let _ = out_handle.join();
                let _ = err_handle.join();
                return Err(RunnerError::SpawnFailed { source: e });
            }
        }
    };

    let out_bytes = out_handle.join().unwrap_or_default();
    let err_bytes = err_handle.join().unwrap_or_default();

    let stdout = String::from_utf8(out_bytes)
        .map_err(|_| RunnerError::InvalidOutput { stream: "stdout" })?;
    let stderr = String::from_utf8(err_bytes)
        .map_err(|_| RunnerError::InvalidOutput { stream: "stderr" })?;

    let exit_code = status.code().unwrap_or(-1);

    Ok(CommandOutput {
        exit_code,
        stdout: trim_trailing_newline(&stdout),
        stderr: trim_trailing_newline(&stderr),
    })
}

fn read_to_end(pipe: &mut Option<impl Read>) -> Vec<u8> {
    let mut buf = Vec::new();
    if let Some(p) = pipe.as_mut() {
        let _ = p.read_to_end(&mut buf);
    }
    buf
}

/// Remove a single trailing newline, if present.
fn trim_trailing_newline(s: &str) -> String {
    s.strip_suffix('\n')
        .map(|s| s.to_string())
        .unwrap_or_else(|| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_newline() {
        assert_eq!(trim_trailing_newline("a\n"), "a");
        assert_eq!(trim_trailing_newline("a"), "a");
        assert_eq!(trim_trailing_newline(""), "");
    }

    #[test]
    fn run_true_succeeds() {
        let out = run(OsStr::new("true"), &[], Duration::from_secs(5)).unwrap();
        assert!(out.is_success());
        assert_eq!(out.stdout, "");
    }

    #[test]
    fn run_nonzero_exit_is_error() {
        let err = run(OsStr::new("false"), &[], Duration::from_secs(5)).unwrap_err();
        match err {
            RunnerError::NonZeroExit { exit_code, .. } => assert_eq!(exit_code, 1),
            other => panic!("expected NonZeroExit, got {other:?}"),
        }
    }

    #[test]
    fn run_captures_stdout() {
        let out = run(
            OsStr::new("echo"),
            &[OsStr::new("hello")],
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(out.is_success());
        assert_eq!(out.stdout, "hello");
    }

    #[test]
    fn missing_executable_is_not_found() {
        let err = run(
            OsStr::new("/tmp/stellar-scenario-missing-binary-12345"),
            &[],
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(matches!(err, RunnerError::NotFound { .. }), "got {err:?}");
    }

    #[test]
    fn stderr_captured_on_failure() {
        let err = run(
            OsStr::new("bash"),
            &[OsStr::new("-c"), OsStr::new("echo oops >&2; exit 3")],
            Duration::from_secs(5),
        )
        .unwrap_err();
        match err {
            RunnerError::NonZeroExit { exit_code, stderr } => {
                assert_eq!(exit_code, 3);
                assert!(stderr.contains("oops"));
            }
            other => panic!("expected NonZeroExit, got {other:?}"),
        }
    }

    #[test]
    fn timeout_kills_slow_process() {
        let start = Instant::now();
        let err = run(
            OsStr::new("sleep"),
            &[OsStr::new("30")],
            Duration::from_millis(300),
        )
        .unwrap_err();
        let elapsed = start.elapsed();
        assert!(matches!(err, RunnerError::Timeout { .. }), "got {err:?}");
        assert!(elapsed < Duration::from_secs(10), "took {elapsed:?}");
    }

    #[test]
    fn arguments_are_not_shell_interpolated() {
        // A metacharacter must reach the child verbatim, not be expanded.
        let out = run(
            OsStr::new("echo"),
            &[OsStr::new("a; rm -rf /tmp/should-not-happen")],
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(out.stdout, "a; rm -rf /tmp/should-not-happen");
    }

    #[test]
    fn stdin_is_forwarded() {
        let out = run_with_stdin(
            OsStr::new("cat"),
            &[],
            Duration::from_secs(5),
            Some("piped-data".to_string()),
        )
        .unwrap();
        assert_eq!(out.stdout, "piped-data");
    }

    #[test]
    fn find_cli_prefers_explicit_path() {
        let found = find_stellar_cli(Some("/custom/stellar")).unwrap();
        assert_eq!(found, PathBuf::from("/custom/stellar"));
    }
}
