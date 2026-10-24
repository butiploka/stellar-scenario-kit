# Stellar Scenario Kit

Local-first toolkit for defining, validating, and eventually executing
reproducible Stellar/Soroban development scenarios.

A scenario is a versioned YAML file that describes a repeatable development
flow — fund a test identity, send a payment, assert a balance. The kit
currently validates those files and reports structured results; execution
against a live network is a later milestone (see [Status](#status)).

No website, dashboard, hosted server, AI API, or paid service is involved.
Everything runs locally.

## Status

Implemented:

- `stellar-scenario validate` — parse and validate a scenario file
- Versioned schema (`schema_version: "1.0"`)
- Semantic validation of steps, actions, and fields
- Actionable errors naming the file, field, and line
- Stable JSON and text output
- Deterministic exit codes
- Unit and integration tests with checked-in fixtures

Not yet implemented (deferred deliberately):

- Execution of scenarios against a network
- Any dependency on the `soroban` binary — the `stellar` CLI is sufficient
  for the current scope, and no execution path has been verified yet

## Installation

Requires Rust 1.80.0 or newer.

```bash
cargo install --path .
```

Or build from source:

```bash
git clone https://github.com/butiploka/stellar-scenario-kit.git
cd stellar-scenario-kit
cargo build --release
# binary at target/release/stellar-scenario
```

## Usage

### Validate a scenario

```bash
stellar-scenario validate examples/payment.yml
```

JSON output (default):

```json
{
  "schema_version": "1.0",
  "result": "valid",
  "scenario": "payment_alice_to_bob",
  "steps": 3,
  "errors": [],
  "file": "examples/payment.yml"
}
```

Human-readable output on stderr:

```bash
stellar-scenario validate examples/payment.yml --output text
```

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | Scenario is valid |
| 1 | Semantic validation failed — the file parsed but broke a rule |
| 2 | I/O error, YAML parse error, or unexpected internal error |

The split between 1 and 2 matters for CI: a schema or semantic problem is a
bug in the scenario file, while a parse or I/O failure usually means the file
is not YAML at all or is unreadable.

## Scenario format

A scenario is a YAML document. Unknown top-level fields are rejected rather
than silently ignored, so a typo in a field name is an error instead of a
silently dropped setting.

```yaml
schema_version: "1.0"          # required; must be "1.0"
name: payment_alice_to_bob     # required; non-empty
description: Send 100 XLM      # optional
network: testnet               # required; network profile name
source: alice                  # optional; default identity
steps:                         # required; at least one step
  - action: ensure_funded
    identity: alice
    min_balance: 200

  - action: payment
    from: alice
    to: bob
    amount: "100"              # string, to preserve decimal precision
    asset: native              # optional; defaults to native XLM

  - action: assert_balance
    identity: bob
    min_balance: 100
```

### Actions

| Action | Required fields | Optional fields |
|--------|-----------------|-----------------|
| `ensure_funded` | `identity` | `min_balance` |
| `payment` | `from`, `to`, `amount` | `asset` |
| `assert_balance` | `identity` | `min_balance` |

### JSON output fields

| Field | Type | Notes |
|-------|------|-------|
| `schema_version` | string | Version of the report format |
| `result` | string | Exactly `"valid"` or `"invalid"` |
| `scenario` | string | Scenario name; omitted when the file could not be parsed |
| `steps` | integer | Step count; omitted when the file could not be parsed |
| `error` | string | One-line failure summary; omitted when valid |
| `errors` | array | Structured issues; empty when valid |
| `file` | string | Path that was validated |

Each entry in `errors` has `field`, `message`, and an optional `line`.

## Examples

- `examples/payment.yml` — fund, pay, assert balance
- `examples/auth_setup.yml` — fund an identity for authorization testing

```bash
stellar-scenario validate examples/payment.yml
stellar-scenario validate examples/auth_setup.yml
```

## Development

```bash
cargo build
cargo test
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

### Layout

```
src/
  lib.rs          crate root, exit code contract
  main.rs         binary entry point
  cli.rs          clap argument definitions
  scenario.rs     scenario model, parsing, validation
  error.rs        error types and exit code mapping
  reporter.rs     JSON and text output
tests/
  unit_tests.rs            parsing and validation unit tests
  integration_test.rs      CLI tests against the compiled binary
  scenarios/               checked-in fixtures
examples/                  documented example scenarios
```

### Adding a fixture

Any file under `tests/scenarios/` whose name starts with `invalid_` must fail
validation; every other `.yml` file under `tests/scenarios/` and `examples/`
must pass. The `fixture_integrity` test enforces this, so a broken fixture
cannot enter the repository unnoticed.

## CI

`.github/workflows/ci.yml` runs on every push and pull request:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`

## Minimum Supported Rust Version

1.80.0. The direct dependencies (`clap` 4.5, `serde` 1.0, `serde_yaml` 0.9,
`serde_json` 1.0, `thiserror` 1.0) all support older versions; 1.80.0 is the
floor chosen for this project and is enforced via `rust-version` in
`Cargo.toml`.

## License

MIT. See [LICENSE](LICENSE).
