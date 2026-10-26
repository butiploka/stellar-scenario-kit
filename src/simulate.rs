//! Transaction simulation.
//!
//! This module implements an explicitly-simulated capability: build a Soroban
//! invoke envelope, simulate it against the network, and decode the simulated
//! result. It never submits a transaction.
//!
//! # Verified interface (Stellar CLI 28.1.0)
//!
//! Envelope construction, without signing or submitting:
//!
//! ```text
//! stellar contract invoke --contract-id <C...> --source-account <acct> \
//!   --network <n> --build-only -- <function> [--arg value ...]
//! ```
//!
//! Simulation, read-only:
//!
//! ```text
//! stellar tx simulate --source-account <acct> --network <n> <base64-xdr>
//! ```
//!
//! Decoding the simulated envelope, read-only:
//!
//! ```text
//! stellar tx decode <base64-xdr>
//! ```
//!
//! # What simulation is not
//!
//! A simulated result is **not** a confirmed transaction. `stellar tx
//! simulate` asks the RPC to execute the transaction against a ledger
//! snapshot and return the resources and fees it would need. Nothing is
//! written. The result must never be reported as a settled transaction.
//!
//! # Known interface limit
//!
//! `stellar tx simulate` rejects classic operations. Simulating a payment
//! envelope fails with:
//!
//! ```text
//! ❌ error: transaction simulation failed: transaction contains unsupported
//! operation type: OperationTypePayment
//! ```
//!
//! Only Soroban `InvokeHostFunction` envelopes are supported. This module
//! reports that case as [`SimulationStatus::UnsupportedOperation`] rather
//! than pretending the simulation ran.

use crate::assert::jsonl_line;
use crate::runner;
use crate::scenario::{Scenario, Step};

/// Outcome of a simulation attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SimulationStatus {
    /// The envelope was built, simulated, and decoded.
    Simulated,
    /// The CLI refused the operation type (e.g. a classic payment).
    UnsupportedOperation,
    /// Building the envelope failed.
    BuildFailed,
    /// The simulate call failed.
    SimulateFailed,
    /// The simulated envelope could not be decoded.
    DecodeFailed,
    /// A subprocess-level error (missing CLI, timeout, spawn failure).
    Error,
}

/// Resources reported by decoding a simulated envelope.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SimulatedResources {
    /// Inclusion fee in stroops, from the decoded envelope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee: Option<String>,
    /// Resource fee in stroops, from the decoded envelope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_fee: Option<String>,
    /// CPU instructions budgeted, from the decoded envelope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<u64>,
}

/// Result of a single simulation step.
///
/// `simulated` and `submitted` are always present and are the machine-readable
/// answer to "did this touch the ledger?". `submitted` is always `false`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SimulationResult {
    /// Scenario name.
    pub scenario: String,
    /// Zero-based step index.
    pub step_index: usize,
    /// Action that produced this result.
    pub action: String,
    /// Network profile used.
    pub network: String,
    /// Source account the envelope was built for.
    pub source: String,
    /// Contract that was invoked.
    pub contract_id: String,
    /// Function that was invoked.
    pub function: String,
    /// Always `true`: this result came from simulation.
    pub simulated: bool,
    /// Always `false`: no transaction was submitted.
    pub submitted: bool,
    /// Outcome.
    pub status: SimulationStatus,
    /// SHA-256 of the base64 string returned by `stellar tx simulate`.
    ///
    /// This is a content fingerprint of the *encoded* envelope, not a hash of
    /// the decoded XDR bytes. It exists so a report can be tied to the exact
    /// simulated envelope; it is not a ledger identifier and is not comparable
    /// to a transaction hash from the network.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub envelope_sha256: Option<String>,
    /// Resources decoded from the simulated envelope.
    pub resources: SimulatedResources,
    /// Human-readable diagnostic. Never contains secret material.
    pub diagnostic: String,
    /// Classified failure, when the diagnostic came from a CLI error.
    ///
    /// Present only when the CLI produced a recognizable error. Absent on
    /// success, so a consumer that does not read it is unaffected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic_category: Option<crate::diagnostic::DiagnosticCategory>,
    /// The remediation for [`Self::diagnostic_category`], when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

impl SimulationResult {
    /// Execute a `simulate_invoke` step.
    ///
    /// Runs up to three read-only subprocesses: build the envelope, simulate
    /// it, decode the simulated result. No signing key is ever passed and no
    /// submission command is ever invoked.
    pub fn execute(
        scenario: &Scenario,
        step_index: usize,
        step: &Step,
        executable: &std::path::Path,
    ) -> Self {
        let timeout = scenario.timeout_duration();
        let source = step
            .params
            .get("source")
            .and_then(|v| v.as_str())
            .or(scenario.source.as_deref())
            .unwrap_or("<missing>")
            .to_string();
        let contract_id = step
            .params
            .get("contract_id")
            .and_then(|v| v.as_str())
            .unwrap_or("<missing>")
            .to_string();
        let function = step
            .params
            .get("function")
            .and_then(|v| v.as_str())
            .unwrap_or("<missing>")
            .to_string();
        let args: Vec<String> = step
            .params
            .get("args")
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let base = Self {
            scenario: scenario.name.clone(),
            step_index,
            action: step.action.clone(),
            network: scenario.network.clone(),
            source: source.clone(),
            contract_id: contract_id.clone(),
            function: function.clone(),
            simulated: true,
            submitted: false,
            status: SimulationStatus::Error,
            envelope_sha256: None,
            resources: SimulatedResources::default(),
            diagnostic: String::new(),
            diagnostic_category: None,
            remediation: None,
        };

        // Step 1 — build the envelope. No signing, no submission.
        let mut build_args: Vec<std::ffi::OsString> = vec![
            "contract".into(),
            "invoke".into(),
            "--contract-id".into(),
            contract_id.clone().into(),
            "--source-account".into(),
            source.clone().into(),
            "--network".into(),
            scenario.network.clone().into(),
            "--build-only".into(),
            "--".into(),
            function.clone().into(),
        ];
        for a in &args {
            build_args.push(a.clone().into());
        }
        let build_refs: Vec<&std::ffi::OsStr> = build_args.iter().map(|a| a.as_os_str()).collect();

        let envelope = match runner::run(executable.as_os_str(), &build_refs, timeout) {
            Ok(out) => out.stdout.trim().to_string(),
            Err(e) => {
                // Classify on the RunnerError variant, so a local timeout is
                // reported as a timeout rather than as an RPC failure.
                let diag = crate::diagnostic::Diagnostic::classify_runner(&e);
                return base.with(
                    SimulationStatus::BuildFailed,
                    format!("failed to build envelope: {}", diag.detail),
                    Some(diag),
                );
            }
        };
        if envelope.is_empty() {
            let diag = crate::diagnostic::Diagnostic::classify("");
            return base.with(
                SimulationStatus::BuildFailed,
                "envelope build produced no output".to_string(),
                Some(diag),
            );
        }

        // Step 2 — simulate. Read-only.
        let sim_args: Vec<&std::ffi::OsStr> = vec![
            std::ffi::OsStr::new("tx"),
            std::ffi::OsStr::new("simulate"),
            std::ffi::OsStr::new("--source-account"),
            std::ffi::OsStr::new(&source),
            std::ffi::OsStr::new("--network"),
            std::ffi::OsStr::new(&scenario.network),
            std::ffi::OsStr::new(&envelope),
        ];
        let simulated = match runner::run_capture(executable.as_os_str(), &sim_args, timeout) {
            Ok(out) => out,
            Err(e) => {
                // Classify on the RunnerError variant: a local timeout is a
                // local fact, not evidence the RPC is down.
                let diag = crate::diagnostic::Diagnostic::classify_runner(&e);
                return base.with(
                    SimulationStatus::Error,
                    format!("simulate subprocess error: {e}"),
                    Some(diag),
                );
            }
        };
        if !simulated.is_success() {
            let detail = if simulated.stderr.trim().is_empty() {
                simulated.stdout.trim().to_string()
            } else {
                simulated.stderr.trim().to_string()
            };
            let diag = crate::diagnostic::Diagnostic::classify(&detail);
            // A refused operation type keeps its own status, so existing
            // consumers of `unsupported_operation` are unaffected.
            let status =
                if diag.category == crate::diagnostic::DiagnosticCategory::UnsupportedOperation {
                    SimulationStatus::UnsupportedOperation
                } else {
                    SimulationStatus::SimulateFailed
                };
            return base.with(status, diag.detail.clone(), Some(diag));
        }

        let simulated_xdr = simulated.stdout.trim().to_string();
        if simulated_xdr.is_empty() {
            let diag = crate::diagnostic::Diagnostic::classify("");
            return base.with(
                SimulationStatus::SimulateFailed,
                "simulate produced no output".to_string(),
                Some(diag),
            );
        }

        let mut result = base.with(
            SimulationStatus::Simulated,
            "envelope simulated; not submitted".to_string(),
            None,
        );
        result.envelope_sha256 = Some(sha256_hex(&simulated_xdr));

        // Step 3 — decode, best effort. A decode failure does not undo a
        // successful simulation; it only leaves `resources` empty and
        // records why.
        let decode_args: Vec<&std::ffi::OsStr> = vec![
            std::ffi::OsStr::new("tx"),
            std::ffi::OsStr::new("decode"),
            std::ffi::OsStr::new(&simulated_xdr),
        ];
        match runner::run_capture(executable.as_os_str(), &decode_args, timeout) {
            Ok(out) if out.is_success() => match parse_decoded_resources(&out.stdout) {
                Some(resources) => result.resources = resources,
                None => {
                    // Decoding exited 0, so the envelope decoded, but the
                    // document is not the shape this build reads. The
                    // simulation still succeeded; only the fee and resource
                    // figures are unavailable.
                    let diag = crate::diagnostic::Diagnostic::classify_structured(
                        crate::diagnostic::DiagnosticCategory::MalformedDecodedOutput,
                        "decode succeeded but the output was not the expected document shape",
                    );
                    result.diagnostic = format!(
                        "{} (fee and resource figures unavailable: {})",
                        result.diagnostic, diag.detail
                    );
                    result.diagnostic_category = Some(diag.category);
                    result.remediation = Some(diag.remediation);
                }
            },
            Ok(out) => {
                let detail = crate::assert::first_non_empty(&[&out.stderr, &out.stdout])
                    .unwrap_or_else(|| "<no output>".to_string());
                let diag = crate::diagnostic::Diagnostic::classify(&detail);
                result.diagnostic =
                    format!("{} (decode failed: {})", result.diagnostic, diag.detail);
                result.diagnostic_category = Some(diag.category);
                result.remediation = Some(diag.remediation);
            }
            Err(e) => {
                let diag = crate::diagnostic::Diagnostic::classify_runner(&e);
                result.diagnostic =
                    format!("{} (decode error: {})", result.diagnostic, diag.detail);
                result.diagnostic_category = Some(diag.category);
                result.remediation = Some(diag.remediation);
            }
        }

        result
    }

    fn with(
        mut self,
        status: SimulationStatus,
        diagnostic: String,
        diag: Option<crate::diagnostic::Diagnostic>,
    ) -> Self {
        self.status = status;
        self.diagnostic = redact(&diagnostic);
        if let Some(d) = diag {
            self.diagnostic_category = Some(d.category);
            self.remediation = Some(d.remediation);
        }
        self
    }
}

/// Extract fee and resource fields from `stellar tx decode` JSON.
///
/// Returns `None` when the document is not the expected shape, so the caller
/// can tell "decoded but unrecognized" apart from "decoded with some fields
/// absent". The decoder is not part of the safety story, only of the
/// reporting.
fn parse_decoded_resources(json: &str) -> Option<SimulatedResources> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    // A real decode always nests the transaction under `tx.tx`. Without that
    // anchor the document is not the one this build reads.
    let tx = v.get("tx")?.get("tx")?;
    let fee = tx
        .get("fee")
        .and_then(|f| f.as_u64())
        .map(|f| f.to_string());
    let ext_v1 = tx.get("ext").and_then(|e| e.get("v1"));
    let resources = ext_v1.and_then(|e| e.get("resources"));
    let resource_fee = ext_v1
        .and_then(|e| e.get("resource_fee"))
        .and_then(|f| f.as_str())
        .map(String::from);
    let instructions = resources
        .and_then(|r| r.get("instructions"))
        .and_then(|i| i.as_u64());

    Some(SimulatedResources {
        fee,
        resource_fee,
        instructions,
    })
}

/// Redact anything shaped like a Stellar secret seed from a diagnostic.
///
/// A seed is a 56-character base32 run starting with `S`. The scan looks for
/// that shape anywhere in the text, including inside punctuation, so a seed
/// wrapped as `(key SAAA...)` is caught.
///
/// This is a defensive filter, not a guarantee. The real protection is that
/// no code path accepts a secret key as input.
pub fn redact(text: &str) -> String {
    // Scan for the seed shape anywhere in the text, not only in
    // whitespace-delimited tokens. A seed wrapped in punctuation, e.g.
    // `(key SAAA...)`, would otherwise survive, because the surrounding
    // characters make the token longer than 56.
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'S' && is_seed_at(bytes, i) {
            out.push_str("<redacted-secret>");
            i += SEED_LEN;
        } else {
            // Copy one UTF-8 character, so multi-byte input is not split.
            let ch_len = utf8_len(bytes[i]);
            let end = (i + ch_len).min(bytes.len());
            out.push_str(&text[i..end]);
            i = end;
        }
    }
    out
}

/// Length of a Stellar secret seed: 56 base32 characters.
const SEED_LEN: usize = 56;

/// True when `bytes[start..start + SEED_LEN]` is a seed-shaped run.
fn is_seed_at(bytes: &[u8], start: usize) -> bool {
    if start + SEED_LEN > bytes.len() {
        return false;
    }
    let window = &bytes[start..start + SEED_LEN];
    // The character after the run must not be alphanumeric, otherwise this
    // is a longer token that merely starts with S.
    let after = bytes.get(start + SEED_LEN);
    if after.is_some_and(|b| b.is_ascii_alphanumeric()) {
        return false;
    }
    window.iter().all(|b| b.is_ascii_alphanumeric())
}

/// UTF-8 sequence length from a leading byte.
fn utf8_len(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first >> 5 == 0b110 {
        2
    } else if first >> 4 == 0b1110 {
        3
    } else if first >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

/// Minimal SHA-256, used only to give the simulated envelope a stable id.
///
/// This is a compact, self-contained implementation so the crate needs no
/// hashing dependency. It is not used for any security decision. The input is
/// the base64 envelope string as returned by the CLI, not decoded XDR bytes.
fn sha256_hex(input: &str) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let mut msg = input.as_bytes().to_vec();
    let bit_len = (msg.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    h.iter().map(|x| format!("{x:08x}")).collect()
}

// --------------------------------------------------------------------------
// Scenario-level simulation report
// --------------------------------------------------------------------------

/// Overall status of a simulation run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SimulationReportStatus {
    /// Every simulation step produced a simulated envelope.
    Simulated,
    /// At least one step was refused as an unsupported operation.
    UnsupportedOperation,
    /// At least one step failed for another reason.
    Failed,
    /// No simulation step ran.
    NoSimulations,
}

/// Nested step-count summary for [`SimulationReport`].
/// Mirrors the flat counts (`simulated`, `unsupported`, `failed`) while
/// providing granular breakdown and explicit exclusion of skipped steps.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SimulationStepCounts {
    /// Steps that produced a simulated envelope.
    pub simulated: usize,
    /// Steps refused as unsupported operation types.
    pub unsupported: usize,
    /// Steps that failed (BuildFailed, SimulateFailed, DecodeFailed, or Error).
    pub failed: usize,
    /// Steps skipped because they are not simulation steps.
    pub skipped: usize,
    /// Total steps (simulated + unsupported + failed + skipped).
    pub total: usize,
}

/// Aggregate report for a simulation run.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SimulationReport {
    /// Scenario name.
    pub scenario: String,
    /// Schema version declared by the scenario file.
    pub schema_version: String,
    /// Network profile the run targeted.
    pub network: String,
    /// Total steps declared in the scenario.
    pub total_steps: usize,
    /// Steps that produced a simulated envelope.
    pub simulated: usize,
    /// Steps refused as unsupported operation types.
    pub unsupported: usize,
    /// Steps that failed for another reason.
    pub failed: usize,
    /// Steps skipped because they are not simulation steps.
    pub skipped: usize,
    /// Overall status.
    pub status: SimulationReportStatus,
    /// Always `false`. Present so a consumer never has to infer it.
    pub submitted: bool,
    /// Per-step results, in step order.
    pub results: Vec<SimulationResult>,
    /// Nested step-count summary, following [`crate::assert::AssertStepCounts`]
    /// conventions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_counts: Option<SimulationStepCounts>,
}

impl SimulationReport {
    /// Build a report. Aggregation precedence:
    /// `failed` > `unsupported_operation` > `simulated` > `no_simulations`.
    pub fn new(scenario: &Scenario, results: Vec<SimulationResult>, skipped: usize) -> Self {
        let simulated = results
            .iter()
            .filter(|r| r.status == SimulationStatus::Simulated)
            .count();
        let unsupported = results
            .iter()
            .filter(|r| r.status == SimulationStatus::UnsupportedOperation)
            .count();
        let failed = results
            .iter()
            .filter(|r| {
                !matches!(
                    r.status,
                    SimulationStatus::Simulated | SimulationStatus::UnsupportedOperation
                )
            })
            .count();

        let status = if failed > 0 {
            SimulationReportStatus::Failed
        } else if unsupported > 0 {
            SimulationReportStatus::UnsupportedOperation
        } else if simulated > 0 {
            SimulationReportStatus::Simulated
        } else {
            SimulationReportStatus::NoSimulations
        };

        Self {
            scenario: scenario.name.clone(),
            schema_version: scenario.schema_version.clone(),
            network: scenario.network.clone(),
            total_steps: scenario.steps.len(),
            simulated,
            unsupported,
            failed,
            skipped,
            status,
            submitted: false,
            results,
            step_counts: Some(SimulationStepCounts {
                simulated,
                unsupported,
                failed,
                skipped,
                total: scenario.steps.len(),
            }),
        }
    }

    /// Serialize to pretty JSON with a trailing newline.
    pub fn to_json(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        s.push('\n');
        s
    }

    /// Render a human-readable summary.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "{} on {}: {} ({} step(s), {} simulated, {} unsupported, {} failed, {} skipped) [not submitted]\n",
            self.scenario,
            self.network,
            status_label(self.status),
            self.total_steps,
            self.simulated,
            self.unsupported,
            self.failed,
            self.skipped
        ));
        for r in &self.results {
            let mark = match r.status {
                SimulationStatus::Simulated => "SIMULATED",
                SimulationStatus::UnsupportedOperation => "UNSUPPORTED",
                _ => "FAILED",
            };
            out.push_str(&format!(
                "  {mark}\t[{}]\t{}::{} on {}\t{}\n",
                r.step_index, r.contract_id, r.function, r.network, r.diagnostic
            ));
            // A classified failure gets its remediation on its own line, so
            // the fix is visible without reading the raw CLI text.
            if let (Some(cat), Some(fix)) = (r.diagnostic_category, &r.remediation) {
                out.push_str(&format!("    {}\t{}\n", cat.as_str(), fix));
            }
        }
        out
    }

    /// Serialize to a single compact JSON object (no whitespace, one line).
    ///
    /// Used for the JSONL aggregate report on stderr, where a consumer reads
    /// the whole report as one value.
    pub fn to_compact_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Serialize the per-step event stream as JSON Lines.
    ///
    /// Each simulation result emits a {"type": "result", ...} object. One
    /// compact JSON object per line, in step order, with no pretty-printing and
    /// no surrounding prose. Simulation never submits a transaction; results
    /// are read-only simulation outputs.
    pub fn to_jsonl_events(&self) -> String {
        let mut out = String::new();
        for r in &self.results {
            out.push_str(&jsonl_line("result", r));
        }
        out
    }
}

fn status_label(status: SimulationReportStatus) -> &'static str {
    match status {
        SimulationReportStatus::Simulated => "simulated",
        SimulationReportStatus::UnsupportedOperation => "unsupported operation",
        SimulationReportStatus::Failed => "failed",
        SimulationReportStatus::NoSimulations => "no simulations",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            sha256_hex(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn redact_removes_secret_seeds() {
        let seed = "S".to_string() + &"A".repeat(55);
        let text = format!("failed with key {seed} on testnet");
        let out = redact(&text);
        assert!(!out.contains(&seed), "seed survived redaction: {out}");
        assert!(out.contains("<redacted-secret>"));
        assert!(out.contains("testnet"));
    }

    #[test]
    fn redact_leaves_public_keys_alone() {
        let public = "G".to_string() + &"A".repeat(55);
        let text = format!("account {public} not found");
        assert!(redact(&text).contains(&public));
    }

    #[test]
    fn parse_decoded_resources_reads_fee_and_resources() {
        // Shape taken from a real `stellar tx decode` output: resource_fee is
        // a sibling of `resources` under `ext.v1`, not a child of it.
        let json = r#"{"tx":{"tx":{"fee":7266,"ext":{"v1":{"resources":{"instructions":141406},"resource_fee":"7166"}}}}}"#;
        let r = parse_decoded_resources(json).expect("recognized shape");
        assert_eq!(r.fee.as_deref(), Some("7266"));
        assert_eq!(r.resource_fee.as_deref(), Some("7166"));
        assert_eq!(r.instructions, Some(141406));
    }

    #[test]
    fn parse_decoded_resources_rejects_unexpected_shape() {
        // Decode exited 0, so the envelope decoded, but the document is not
        // the one this build reads. The caller must be able to tell this
        // apart from "decoded with fields absent".
        assert!(parse_decoded_resources("{}").is_none());
        assert!(parse_decoded_resources("not json").is_none());
        assert!(parse_decoded_resources(r#"{"tx":{}}"#).is_none());
    }

    #[test]
    fn parse_decoded_resources_tolerates_missing_optional_fields() {
        // The `tx.tx` anchor is present, so the shape is recognized; the
        // individual figures are legitimately absent.
        let r = parse_decoded_resources(r#"{"tx":{"tx":{}}}"#).expect("recognized shape");
        assert!(r.fee.is_none());
        assert!(r.resource_fee.is_none());
        assert!(r.instructions.is_none());
    }

    fn scenario_with(raw: &str) -> Scenario {
        Scenario::from_str(raw).unwrap()
    }

    fn result(scenario: &Scenario, idx: usize, status: SimulationStatus) -> SimulationResult {
        SimulationResult {
            scenario: scenario.name.clone(),
            step_index: idx,
            action: "simulate_invoke".into(),
            network: scenario.network.clone(),
            source: "alice".into(),
            contract_id: "CABC".into(),
            function: "balance".into(),
            simulated: true,
            submitted: false,
            status,
            envelope_sha256: None,
            resources: SimulatedResources::default(),
            diagnostic: "d".into(),
            diagnostic_category: None,
            remediation: None,
        }
    }

    const TWO_STEPS: &str = r#"schema_version: "1.0"
name: sim
network: testnet
steps:
  - action: simulate_invoke
    contract_id: CABC
    function: balance
  - action: simulate_invoke
    contract_id: CDEF
    function: balance
"#;

    #[test]
    fn report_all_simulated() {
        let s = scenario_with(TWO_STEPS);
        let rep = SimulationReport::new(
            &s,
            vec![
                result(&s, 0, SimulationStatus::Simulated),
                result(&s, 1, SimulationStatus::Simulated),
            ],
            0,
        );
        assert_eq!(rep.status, SimulationReportStatus::Simulated);
        assert_eq!(rep.simulated, 2);
        assert!(!rep.submitted, "submitted must always be false");
    }

    #[test]
    fn report_failed_beats_unsupported() {
        let s = scenario_with(TWO_STEPS);
        let rep = SimulationReport::new(
            &s,
            vec![
                result(&s, 0, SimulationStatus::UnsupportedOperation),
                result(&s, 1, SimulationStatus::BuildFailed),
            ],
            0,
        );
        assert_eq!(rep.status, SimulationReportStatus::Failed);
        assert_eq!(rep.unsupported, 1);
        assert_eq!(rep.failed, 1);
    }

    #[test]
    fn report_unsupported_beats_simulated() {
        let s = scenario_with(TWO_STEPS);
        let rep = SimulationReport::new(
            &s,
            vec![
                result(&s, 0, SimulationStatus::Simulated),
                result(&s, 1, SimulationStatus::UnsupportedOperation),
            ],
            0,
        );
        assert_eq!(rep.status, SimulationReportStatus::UnsupportedOperation);
    }

    #[test]
    fn report_no_simulations_when_nothing_ran() {
        let s = scenario_with(TWO_STEPS);
        let rep = SimulationReport::new(&s, vec![], 2);
        assert_eq!(rep.status, SimulationReportStatus::NoSimulations);
        assert_eq!(rep.skipped, 2);
    }

    #[test]
    fn report_json_marks_not_submitted() {
        let s = scenario_with(TWO_STEPS);
        let rep = SimulationReport::new(&s, vec![result(&s, 0, SimulationStatus::Simulated)], 1);
        let json = rep.to_json();
        assert!(json.contains("\"submitted\": false"));
        assert!(json.contains("\"simulated\": true"));
    }

    #[test]
    fn report_text_says_not_submitted() {
        let s = scenario_with(TWO_STEPS);
        let rep = SimulationReport::new(&s, vec![result(&s, 0, SimulationStatus::Simulated)], 1);
        assert!(rep.to_text().contains("not submitted"));
    }

    #[test]
    fn report_step_counts_nested_and_flat() {
        let s = scenario_with(TWO_STEPS);
        let rep = SimulationReport::new(
            &s,
            vec![
                result(&s, 0, SimulationStatus::Simulated),
                result(&s, 1, SimulationStatus::BuildFailed),
            ],
            0, // both steps were simulate_invoke, so none skipped
        );
        assert!(rep.step_counts.is_some());
        let sc = rep.step_counts.as_ref().unwrap();
        assert_eq!(sc.simulated, 1);
        assert_eq!(sc.unsupported, 0);
        assert_eq!(sc.failed, 1); // BuildFailed falls under "failed"
        assert_eq!(sc.skipped, 0);
        assert_eq!(sc.total, 2); // TWO_STEPS has 2 steps
                                 // Flat counts for backward compat
        assert_eq!(rep.simulated, 1);
        assert_eq!(rep.failed, 1);
        assert_eq!(rep.skipped, 0);
        assert_eq!(rep.total_steps, 2);
    }

    #[test]
    fn compose_jsonl_events_single_line() {
        let s = scenario_with(TWO_STEPS);
        let rep = SimulationReport::new(
            &s,
            vec![
                result(&s, 0, SimulationStatus::Simulated),
                result(&s, 1, SimulationStatus::UnsupportedOperation),
            ],
            0,
        );
        let events = rep.to_jsonl_events();
        let lines: Vec<&str> = events.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in &lines {
            let ev: serde_json::Value = serde_json::from_str(line).expect("valid JSON per line");
            assert_eq!(ev.get("type").and_then(|v| v.as_str()), Some("result"));
        }
    }

    #[test]
    fn compose_compact_json_is_single_line() {
        let s = scenario_with(TWO_STEPS);
        let rep = SimulationReport::new(&s, vec![result(&s, 0, SimulationStatus::Simulated)], 1);
        let compact = rep.to_compact_json();
        assert!(!compact.contains('\n'), "compact JSON must be single line");
        assert!(compact.contains("\"scenario\""));
        assert!(compact.contains("\"step_counts\""));
    }

    #[test]
    fn jsonl_event_has_type_first() {
        // serde_json preserves struct field order, ensuring `type` is first.
        let s = scenario_with(TWO_STEPS);
        let rep = SimulationReport::new(&s, vec![result(&s, 0, SimulationStatus::Simulated)], 1);
        let events = rep.to_jsonl_events();
        let first_line = events.lines().next().unwrap();
        assert!(
            first_line.starts_with("{\"type\":"),
            "type field must be first: {first_line}"
        );
    }

    /// Every JSONL event preserves its original SimulationStatus, and every
    /// aggregate counter classifies that same set of results consistently.
    /// Skipped steps are excluded from the event stream but counted in `skipped`.
    #[test]
    fn status_to_counter_consistency_all_variants() {
        let raw = r#"schema_version: "1.0"
name: status_matrix
network: testnet
steps:
  - action: simulate_invoke
    contract_id: CAAA
    function: f
  - action: simulate_invoke
    contract_id: CBBB
    function: f
  - action: simulate_invoke
    contract_id: CCCC
    function: f
  - action: simulate_invoke
    contract_id: CDDD
    function: f
  - action: simulate_invoke
    contract_id: CEEE
    function: f
  - action: simulate_invoke
    contract_id: CFFF
    function: f
  - action: simulate_invoke
    contract_id: CGGG
    function: f
"#;
        let s = Scenario::from_str(raw).expect("scenario parses");
        // 7 steps, all simulate_invoke → 0 skipped.
        let statuses = [
            SimulationStatus::Simulated,
            SimulationStatus::UnsupportedOperation,
            SimulationStatus::BuildFailed,
            SimulationStatus::SimulateFailed,
            SimulationStatus::DecodeFailed,
            SimulationStatus::Error,
            SimulationStatus::Simulated,
        ];
        let results: Vec<SimulationResult> = statuses
            .iter()
            .enumerate()
            .map(|(i, st)| result(&s, i, *st))
            .collect();

        let rep = SimulationReport::new(&s, results, 0);
        let sc = rep.step_counts.as_ref().expect("step_counts present");

        // Event stream: one event per result, each with the original status.
        let events = rep.to_jsonl_events();
        let event_statuses: Vec<String> = events
            .lines()
            .map(|l| {
                let v: serde_json::Value = serde_json::from_str(l).expect("event is JSON");
                assert_eq!(v.get("type").and_then(|t| t.as_str()), Some("result"));
                v.get("status")
                    .and_then(|s| s.as_str())
                    .expect("event has status")
                    .to_string()
            })
            .collect();
        assert_eq!(
            event_statuses,
            vec![
                "simulated",
                "unsupported_operation",
                "build_failed",
                "simulate_failed",
                "decode_failed",
                "error",
                "simulated",
            ]
        );

        // Counters classify the same results.
        assert_eq!(sc.simulated, 2, "only Simulated counts as simulated");
        assert_eq!(sc.unsupported, 1, "UnsupportedOperation is its own bucket");
        assert_eq!(
            sc.failed, 4,
            "build/simulate/decode failed + error all count as failed"
        );
        assert_eq!(sc.skipped, 0, "all steps were simulate_invoke");
        assert_eq!(sc.total, s.steps.len(), "total equals scenario step count");
        // Reconciliation: buckets sum to the total.
        assert_eq!(
            sc.simulated + sc.unsupported + sc.failed + sc.skipped,
            sc.total
        );

        // Simulation never submits.
        assert!(!rep.submitted);
        for l in events.lines() {
            let v: serde_json::Value = serde_json::from_str(l).expect("event is JSON");
            assert_eq!(
                v.get("submitted").and_then(|b| b.as_bool()),
                Some(false),
                "submitted must be false on every event"
            );
            assert_eq!(
                v.get("simulated").and_then(|b| b.as_bool()),
                Some(true),
                "simulated must be true on every event"
            );
        }
    }

    /// Skipped steps (non-`simulate_invoke`) are counted in `skipped`, emit no
    /// JSONL events, and do not overlap the other counters.
    #[test]
    fn skipped_steps_emit_no_events_and_count_separately() {
        let raw = r#"schema_version: "1.0"
name: skip_matrix
network: testnet
source: alice
steps:
  - action: simulate_invoke
    contract_id: CAAA
    function: f
  - action: ensure_funded
    identity: alice
    min_balance: 200
  - action: payment
    from: alice
    to: bob
    amount: "1"
    asset: native
  - action: simulate_invoke
    contract_id: CBBB
    function: f
"#;
        let s = Scenario::from_str(raw).expect("scenario parses");
        let results = vec![
            result(&s, 0, SimulationStatus::Simulated),
            result(&s, 3, SimulationStatus::UnsupportedOperation),
        ];
        // 2 skipped: ensure_funded + payment.
        let rep = SimulationReport::new(&s, results, 2);
        let sc = rep.step_counts.as_ref().expect("step_counts present");

        assert_eq!(sc.simulated, 1);
        assert_eq!(sc.unsupported, 1);
        assert_eq!(sc.failed, 0);
        assert_eq!(sc.skipped, 2);
        assert_eq!(sc.total, 4);
        assert_eq!(
            sc.simulated + sc.unsupported + sc.failed + sc.skipped,
            sc.total
        );

        // Only the 2 executed steps emit events — never the skipped ones.
        let events = rep.to_jsonl_events();
        assert_eq!(events.lines().count(), 2);
        for l in events.lines() {
            let v: serde_json::Value = serde_json::from_str(l).expect("event is JSON");
            assert_eq!(v.get("type").and_then(|t| t.as_str()), Some("result"));
        }
    }
}
