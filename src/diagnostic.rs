//! Diagnostic classification for `simulate_invoke` failures.
//!
//! This module turns a raw Stellar CLI error into a category plus a short
//! remediation. It exists so an author can act on a failure without reading
//! the CLI's full output.
//!
//! # What is proven, not guessed
//!
//! Every category below was reproduced against Stellar CLI 28.1.0 before
//! being added. The matchers are anchored to the CLI's own error prefixes
//! (`error: <text>`) rather than to loose substrings, so a message that
//! merely contains the word "contract" does not get misclassified.
//!
//! Anything unrecognized is returned as [`DiagnosticCategory::Unknown`] and
//! keeps the original message. A wrong category is worse than no category,
//! because it sends the author to the wrong fix.
//!
//! # Safety
//!
//! Classification runs on text the CLI already emitted, which is then
//! redacted by the caller. No secret is introduced here.

use serde::Serialize;

/// A proven failure category, with the remediation that applies to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCategory {
    /// The contract id does not exist on the network.
    ContractNotFound,
    /// The function name is not in the contract interface.
    FunctionNotFound,
    /// A required argument was omitted.
    MissingArgument,
    /// An argument was present but the wrong type.
    InvalidArgument,
    /// The network profile is not in the Stellar CLI config.
    NetworkNotConfigured,
    /// The source identity is not in the Stellar CLI config.
    IdentityNotConfigured,
    /// The RPC endpoint could not be reached.
    RpcUnreachable,
    /// The envelope is a classic operation, which `tx simulate` rejects.
    UnsupportedOperation,
    /// The simulation ran and the contract returned a host error.
    HostError,
    /// The simulated envelope could not be decoded.
    DecodeFailed,
    /// The CLI produced no output at all.
    EmptyOutput,
    /// Not recognized. The original message is preserved.
    Unknown,
}

impl DiagnosticCategory {
    /// The remediation shown for this category.
    pub fn remediation(&self) -> &'static str {
        match self {
            Self::ContractNotFound => {
                "check the contract_id; confirm it exists on the target network with \
                 `stellar contract info interface --id <id> --network <n>`"
            }
            Self::FunctionNotFound => {
                "check the function name; run `stellar-scenario inspect <file> --output text` \
                 to list what the contract exposes"
            }
            Self::MissingArgument => "add the missing argument; the CLI names it in its error",
            Self::InvalidArgument => {
                "check the argument's type; the CLI states the expected type in its error"
            }
            Self::NetworkNotConfigured => {
                "add the network with `stellar network add <name> --rpc-url <url> \
                 --network-passphrase <passphrase>`, or use one from `stellar network ls`"
            }
            Self::IdentityNotConfigured => {
                "add the identity with `stellar keys generate <name>`, or use one from \
                 `stellar keys ls`"
            }
            Self::RpcUnreachable => {
                "check network connectivity and the RPC URL; confirm with \
                 `stellar network info`"
            }
            Self::UnsupportedOperation => {
                "only Soroban InvokeHostFunction envelopes can be simulated; payments and \
                 other classic operations cannot"
            }
            Self::HostError => {
                "the contract rejected the call at runtime; the event log in the CLI output \
                 names the failing function and reason"
            }
            Self::DecodeFailed => {
                "the simulated envelope could not be decoded; this usually means the CLI \
                 version and the envelope disagree"
            }
            Self::EmptyOutput => {
                "the CLI produced no output; check that STELLAR_CLI points at a working \
                 `stellar` binary"
            }
            Self::Unknown => "the CLI message is not recognized; read it directly for the cause",
        }
    }

    /// The stable string used in JSON output.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ContractNotFound => "contract_not_found",
            Self::FunctionNotFound => "function_not_found",
            Self::MissingArgument => "missing_argument",
            Self::InvalidArgument => "invalid_argument",
            Self::NetworkNotConfigured => "network_not_configured",
            Self::IdentityNotConfigured => "identity_not_configured",
            Self::RpcUnreachable => "rpc_unreachable",
            Self::UnsupportedOperation => "unsupported_operation",
            Self::HostError => "host_error",
            Self::DecodeFailed => "decode_failed",
            Self::EmptyOutput => "empty_output",
            Self::Unknown => "unknown",
        }
    }
}

/// A classified diagnostic.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Diagnostic {
    /// The proven category.
    pub category: DiagnosticCategory,
    /// The remediation for that category.
    pub remediation: String,
    /// The original CLI message, redacted by the caller.
    pub detail: String,
}

impl Diagnostic {
    /// Classify a CLI error message.
    ///
    /// `detail` is the text the CLI emitted. It is returned unchanged apart
    /// from trimming; the caller redacts it before it reaches a report.
    pub fn classify(detail: &str) -> Self {
        let trimmed = detail.trim();
        let category = categorize(trimmed);
        Self {
            category,
            remediation: category.remediation().to_string(),
            detail: trimmed.to_string(),
        }
    }

    /// Render a one-line summary: category, remediation, then the raw detail.
    pub fn to_text(&self) -> String {
        format!(
            "{}: {}\n  detail: {}",
            self.category.as_str(),
            self.remediation,
            self.detail
        )
    }
}

/// Match a CLI message against the proven categories.
///
/// Order matters: the more specific prefixes come first, so a message that
/// could match two patterns lands on the accurate one.
fn categorize(msg: &str) -> DiagnosticCategory {
    // Strip the CLI's own `❌ error: ` prefix so the matchers see the text.
    let body = msg
        .strip_prefix("❌ error: ")
        .or_else(|| msg.strip_prefix("error: "))
        .unwrap_or(msg);

    if body.is_empty() {
        return DiagnosticCategory::EmptyOutput;
    }

    // Envelope build failures, in the order the CLI reports them.
    if body.starts_with("contract not found:") {
        return DiagnosticCategory::ContractNotFound;
    }
    if body.starts_with("Failed to find config network for") {
        return DiagnosticCategory::NetworkNotConfigured;
    }
    if body.starts_with("Failed to find config identity for") {
        return DiagnosticCategory::IdentityNotConfigured;
    }
    if body.starts_with("Missing required argument") {
        return DiagnosticCategory::MissingArgument;
    }
    if body.starts_with("Failed to parse argument") {
        return DiagnosticCategory::InvalidArgument;
    }
    // An unknown function surfaces as a clap usage error, which names the
    // subcommand it could not resolve.
    if body.starts_with("unrecognized subcommand") {
        return DiagnosticCategory::FunctionNotFound;
    }

    // Simulation failures.
    if body.contains("unsupported operation type") {
        return DiagnosticCategory::UnsupportedOperation;
    }
    if body.starts_with("transaction simulation failed: HostError:") {
        return DiagnosticCategory::HostError;
    }
    if body.starts_with("client error (Connect)") || body.starts_with("client error (Request)") {
        return DiagnosticCategory::RpcUnreachable;
    }

    // Decode failures.
    if body.starts_with("error decoding XDR:") || body.starts_with("failed to decode XDR") {
        return DiagnosticCategory::DecodeFailed;
    }

    DiagnosticCategory::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every string below was copied from real Stellar CLI 28.1.0 output.
    #[test]
    fn classifies_contract_not_found() {
        let d = Diagnostic::classify(
            "❌ error: contract not found: CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC",
        );
        assert_eq!(d.category, DiagnosticCategory::ContractNotFound);
        assert!(d.remediation.contains("contract info interface"));
    }

    #[test]
    fn classifies_function_not_found() {
        let d = Diagnostic::classify("error: unrecognized subcommand 'nosuchfn'");
        assert_eq!(d.category, DiagnosticCategory::FunctionNotFound);
        assert!(d.remediation.contains("inspect"));
    }

    #[test]
    fn classifies_missing_argument() {
        let d = Diagnostic::classify(
            "❌ error: Missing required argument 'id' of type address (G... for account, C... for contract, or identity name)",
        );
        assert_eq!(d.category, DiagnosticCategory::MissingArgument);
    }

    #[test]
    fn classifies_invalid_argument() {
        let d = Diagnostic::classify(
            "❌ error: Failed to parse argument 'id': invalid type: number, expected string or map",
        );
        assert_eq!(d.category, DiagnosticCategory::InvalidArgument);
    }

    #[test]
    fn classifies_network_not_configured() {
        let d = Diagnostic::classify("❌ error: Failed to find config network for nosuchnet");
        assert_eq!(d.category, DiagnosticCategory::NetworkNotConfigured);
        assert!(d.remediation.contains("network add"));
    }

    #[test]
    fn classifies_identity_not_configured() {
        let d = Diagnostic::classify("❌ error: Failed to find config identity for nosuchidentity");
        assert_eq!(d.category, DiagnosticCategory::IdentityNotConfigured);
        assert!(d.remediation.contains("keys generate"));
    }

    #[test]
    fn classifies_unsupported_operation() {
        let d = Diagnostic::classify(
            "❌ error: transaction simulation failed: transaction contains unsupported operation type: OperationTypePayment",
        );
        assert_eq!(d.category, DiagnosticCategory::UnsupportedOperation);
    }

    #[test]
    fn classifies_host_error() {
        let d = Diagnostic::classify(
            "❌ error: transaction simulation failed: HostError: Error(Storage, MissingValue)",
        );
        assert_eq!(d.category, DiagnosticCategory::HostError);
    }

    #[test]
    fn classifies_rpc_unreachable() {
        let d = Diagnostic::classify("❌ error: client error (Connect)");
        assert_eq!(d.category, DiagnosticCategory::RpcUnreachable);
    }

    #[test]
    fn classifies_decode_failed() {
        let d = Diagnostic::classify("❌ error: error decoding XDR: Invalid symbol 45, offset 3.");
        assert_eq!(d.category, DiagnosticCategory::DecodeFailed);
    }

    #[test]
    fn empty_message_is_empty_output() {
        assert_eq!(
            Diagnostic::classify("").category,
            DiagnosticCategory::EmptyOutput
        );
        assert_eq!(
            Diagnostic::classify("   ").category,
            DiagnosticCategory::EmptyOutput
        );
    }

    #[test]
    fn unknown_message_is_preserved() {
        let weird = "❌ error: something entirely new happened";
        let d = Diagnostic::classify(weird);
        assert_eq!(d.category, DiagnosticCategory::Unknown);
        assert_eq!(d.detail, weird.trim());
        assert!(d.remediation.contains("not recognized"));
    }

    #[test]
    fn a_message_mentioning_contract_is_not_misclassified() {
        // Loose substring matching would call this ContractNotFound. The
        // anchored matcher must not.
        let d =
            Diagnostic::classify("❌ error: contract invocation succeeded but returned nothing");
        assert_eq!(d.category, DiagnosticCategory::Unknown);
    }

    #[test]
    fn every_category_has_a_remediation() {
        for cat in [
            DiagnosticCategory::ContractNotFound,
            DiagnosticCategory::FunctionNotFound,
            DiagnosticCategory::MissingArgument,
            DiagnosticCategory::InvalidArgument,
            DiagnosticCategory::NetworkNotConfigured,
            DiagnosticCategory::IdentityNotConfigured,
            DiagnosticCategory::RpcUnreachable,
            DiagnosticCategory::UnsupportedOperation,
            DiagnosticCategory::HostError,
            DiagnosticCategory::DecodeFailed,
            DiagnosticCategory::EmptyOutput,
            DiagnosticCategory::Unknown,
        ] {
            assert!(!cat.remediation().is_empty(), "{cat:?} has no remediation");
            assert!(!cat.as_str().is_empty(), "{cat:?} has no string");
        }
    }

    #[test]
    fn category_strings_are_snake_case() {
        for cat in [
            DiagnosticCategory::ContractNotFound,
            DiagnosticCategory::FunctionNotFound,
            DiagnosticCategory::RpcUnreachable,
            DiagnosticCategory::Unknown,
        ] {
            let s = cat.as_str();
            assert!(!s.contains(char::is_uppercase), "{s} is not snake_case");
            assert!(!s.contains(' '), "{s} contains a space");
        }
    }

    #[test]
    fn to_text_includes_category_remediation_and_detail() {
        let d = Diagnostic::classify("❌ error: contract not found: CAAA");
        let text = d.to_text();
        assert!(text.contains("contract_not_found"));
        assert!(text.contains("contract info interface"));
        assert!(text.contains("contract not found: CAAA"));
    }

    #[test]
    fn detail_is_trimmed() {
        let d = Diagnostic::classify("  ❌ error: contract not found: CAAA\n");
        assert_eq!(d.detail, "❌ error: contract not found: CAAA");
    }
}
