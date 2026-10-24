# Stellar Scenario Kit

Local-first toolkit for defining, validating, and planning reproducible
Stellar/Soroban development scenarios.

A scenario is a versioned YAML file that describes a repeatable development
flow — fund a test identity, send a payment, assert a balance. The kit
currently validates those files, plans them against the Stellar CLI, and
reports structured results. Live execution is a later milestone (see
[Status](#status) and [Execution contract](#execution-contract)).

No website, dashboard, hosted server, AI API, or paid service is involved.
Everything runs locally.

## Status

Implemented:

- `stellar-scenario validate` — parse and validate a scenario file
- `stellar-scenario plan` — print the exact Stellar CLI commands a run would
  execute, without executing anything
- `stellar-scenario doctor` — report environment readiness
- Versioned schema (`schema_version: "1.0"`)
- Semantic validation of steps, actions, and fields
- Actionable errors naming the file, field, and line
- Stable JSON and text output
- Deterministic exit codes
- Process execution adapter with timeouts and structured errors
- Unit and integration tests with checked-in fixtures

Not yet implemented (deferred deliberately):

- Live execution of scenarios against a network
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

The `plan` and `doctor` commands locate the Stellar CLI via `stellar` on
`PATH`, or the `STELLAR_CLI` environment variable when set.

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

### Plan a scenario

```bash
stellar-scenario plan examples/payment.yml
```

```json
{
  "scenario": "payment_alice_to_bob",
  "network": "testnet",
  "commands": [
    {
      "step_index": 0,
      "action": "ensure_funded",
      "executable": "/home/ubuntu/.local/bin/stellar",
      "args": ["keys", "fund", "alice", "--network", "testnet"],
      "side_effect": "mutates_remote_state"
    },
    {
      "step_index": 1,
      "action": "payment",
      "executable": "/home/ubuntu/.local/bin/stellar",
      "args": ["tx", "new", "payment", "--source-account", "alice", "--destination", "bob", "--amount", "100", "--asset", "native", "--network", "testnet"],
      "side_effect": "mutates_remote_state"
    },
    {
      "step_index": 2,
      "action": "assert_balance",
      "executable": "/home/ubuntu/.local/bin/stellar",
      "args": ["token", "balance", "--account", "bob", "--network", "testnet"],
      "side_effect": "read_only"
    }
  ],
  "read_only": false
}
```

`plan` spawns no subprocess and contacts no network. It is a pure function of
the scenario file plus the resolved CLI path, so it is safe to run anywhere,
including CI.

### Check the environment

```bash
stellar-scenario doctor
```

```json
[
  { "name": "stellar_cli", "ok": true, "detail": "found at /home/ubuntu/.local/bin/stellar" },
  { "name": "stellar_cli_version", "ok": true, "detail": "stellar 28.1.0 (c0f4d0da891bbf214c08b8c5035ae6db80e9a3bd)" },
  { "name": "networks_configured", "ok": true, "detail": "4 network(s): local, futurenet, mainnet, testnet" },
  { "name": "identities_configured", "ok": true, "detail": "4 identity name(s) present" }
]
```

`doctor` runs only read-only Stellar CLI commands (`--version`, `keys ls`,
`network ls`). It reports identity *counts*, never names or keys, and exits
1 if any check fails.

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success — scenario is valid, or the plan/doctor succeeded |
| 1 | Semantic validation failed, or a `doctor` check failed |
| 2 | I/O error, YAML parse error, or unexpected internal error |

The split between 1 and 2 matters for CI: a schema or semantic problem is a
bug in the scenario file, while a parse or I/O failure usually means the file
is not YAML at all or is unreadable.

## Execution contract

These rules are enforced by the code and by tests, not just documented.

**`validate` is pure.** It reads a file and reports on it. It never runs a
subprocess, never funds an account, and never contacts a network. Funding is
never a side effect of parsing or validating a scenario.

**`plan` is pure.** It resolves the Stellar CLI path and prints the argv that
a live run would use. No subprocess is spawned. The `plan_does_not_mutate_
remote_state` integration test asserts the local identity list is unchanged
after planning a scenario that would otherwise fund and pay.

**Simulation is not confirmation.** `stellar tx simulate` exists in Stellar
CLI 28.1.0 and is read-only on the network, but a simulation result is not a
confirmed transaction. Simulation support is deferred; when it lands, its
output must never be reported as a settled transaction.

**Live submission requires explicit intent.** Any future live execution will
require an explicit `run` invocation with a flag that names the action. It
will never happen implicitly during validation or planning.

**Secrets stay out.** No command in this toolkit accepts a secret key as
input, and no output path prints one. `plan` and `doctor` output are checked
by tests that scan for 56-character `S`-prefixed tokens, the shape of a
Stellar secret seed.

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

| Action | Required fields | Optional fields | Side effect when run |
|--------|-----------------|-----------------|----------------------|
| `ensure_funded` | `identity` | `min_balance` | Mutates remote state |
| `payment` | `from`, `to`, `amount` | `asset` | Mutates remote state |
| `assert_balance` | `identity` | `min_balance` | Read only |

### JSON output fields

Validation report:

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

Execution plan:

| Field | Type | Notes |
|-------|------|-------|
| `scenario` | string | Scenario name |
| `network` | string | Network profile the plan targets |
| `commands` | array | Planned commands in order |
| `read_only` | boolean | True when every command is read-only |

Each entry in `commands` has `step_index`, `action`, `executable`, `args`, and
`side_effect` (`read_only` or `mutates_remote_state`).

## Examples

- `examples/payment.yml` — fund, pay, assert balance
- `examples/auth_setup.yml` — fund an identity for authorization testing

```bash
stellar-scenario validate examples/payment.yml
stellar-scenario plan examples/payment.yml
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
  lib.rs          crate root, module list, exit code contract
  main.rs         binary entry point, command dispatch
  cli.rs          clap argument definitions
  scenario.rs     scenario model, parsing, validation
  error.rs        error types and exit code mapping
  reporter.rs     JSON and text output for validation
  plan.rs         execution planning and side-effect classification
  runner.rs       subprocess adapter: argv, timeouts, structured errors
  stellar.rs      Stellar CLI discovery and read-only diagnostics
tests/
  unit_tests.rs            parsing and validation unit tests
  integration_test.rs      validate command tests
  execution_test.rs        plan and doctor command tests
  scenarios/               checked-in fixtures
examples/                  documented example scenarios
```

### Adding a fixture

Any file under `tests/scenarios/` whose name starts with `invalid_` must fail
validation; every other `.yml` file under `tests/scenarios/` and `examples/`
must pass. The `fixture_integrity` test enforces this, so a broken fixture
cannot enter the repository unnoticed.

### Testing without the Stellar CLI

`runner.rs` is tested against real system executables (`true`, `false`,
`echo`, `cat`, `sleep`, `bash`) rather than mocks, so the timeout, exit-code,
stdin, and argument-safety paths are exercised for real. The `stellar.rs`
tests require the Stellar CLI to be installed; they assert on local config
only and never touch a network.

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
