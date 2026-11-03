# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-10-26

### Added

- Scenario preflight validation, finalised for v0.2.0.
- `inspect` command: read a Soroban contract's functions and events
  via `stellar contract info interface`.
- `scaffold` command: generate a `simulate_invoke` step from a
  contract interface, with placeholder arguments (never invented values).
- `simulate` command: build and simulate Soroban invoke envelopes
  without submitting anything, with fee/resource figures from
  the decoded envelope.
- Diagnostic classification with category and remediation for
  `assert`, `simulate`, and `inspect` failures.
- `doctor` command: report Stellar CLI presence, version floor
  (>= 28.1.0), configured networks, and identity counts
  (never names or keys).
- Structured aggregate reports and JSON output contracts for
  every command, pinned by `tests/json_contract_test.rs`.
- Unit, integration, execution, assertion, simulation, inspect,
  scaffold, robustness, and JSON contract tests, with checked-in
  YAML fixtures under `tests/scenarios/` and `examples/`.
- `CONTRIBUTING.md`, GitHub issue templates, and a pull request
  template enforcing the project's scope rules.

### Changed

- `assert_balance` now requires `min_balance` (previously optional,
  defaulting to `0`); use `assert_asset` for existence-only checks.
- Validation accumulates independent problems, each naming the
  field path, instead of stopping at the first error (structural
  parse problems still fail fast).

### Fixed

- CLI-dependent tests skip cleanly when the Stellar CLI is absent
  or non-functional, including JSON contract tests and the
  exit-1 JSON routing case.
- Input robustness and subprocess error handling regressions
  found during the v0.2.0 work.

### Security

- No command accepts a secret key as input, and no output path
  prints one. Diagnostics pass through redaction for
  56-character `S`-prefixed seeds before reaching a report.
- `validate` and `plan` remain pure; `assert` runs only
  read-only RPC reads; `simulate` never submits.

## Initial MVP - 2026-10-24 (git history, no separate release tag)

Initial stellar-scenario MVP, as recorded in git history:
`validate` command, execution foundation with `plan`, `doctor`,
and a runner, plus the first `assert_balance` execution path.

[Unreleased]: https://github.com/butiploka/stellar-scenario-kit/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/butiploka/stellar-scenario-kit/releases/tag/v0.2.0
