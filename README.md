# Stellar Scenario Kit

Local-first toolkit for defining, validating, and executing reproducible
Stellar/Soroban development scenarios.

A scenario is a versioned YAML file that describes a repeatable development
flow — fund a test identity, send a payment, assert a balance. The kit
validates those files, plans them against the Stellar CLI, and executes the
read-only steps. Live transaction submission is a later milestone (see
[Status](#status) and [Execution contract](#execution-contract)).

No website, dashboard, hosted server, AI API, or paid service is involved.
Everything runs locally.

## Status

Implemented:

- `stellar-scenario validate` — parse and validate a scenario file
- `stellar-scenario plan` — print the exact Stellar CLI commands a run would
  execute, without executing anything
- `stellar-scenario assert` — execute read-only `assert_balance` steps
- `stellar-scenario doctor` — report environment readiness
- Versioned schema (`schema_version: "1.0"`)
- Semantic validation of steps, actions, and fields
- Actionable errors naming the file, field, and line
- Stable JSON and text output
- Deterministic exit codes
- Process execution adapter with timeouts and structured errors
- Unit and integration tests with checked-in fixtures

Not yet implemented (deferred deliberately):

- Transaction submission (`stellar tx send`)
- Automatic account funding (`stellar keys fund`)
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

The `plan`, `assert`, and `doctor` commands locate the Stellar CLI via
`stellar` on `PATH`, or the `STELLAR_CLI` environment variable when set.

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
      "args": ["token", "balance", "--id", "native", "--account", "bob", "--network", "testnet", "--output", "json"],
      "side_effect": "read_only"
    }
  ],
  "read_only": false
}
```

`plan` spawns no subprocess and contacts no network. It is a pure function of
the scenario file plus the resolved CLI path, so it is safe to run anywhere,
including CI.

### Assert a balance

```bash
stellar-scenario assert examples/assert_balance.yml
```

```json
{
  "scenario": "assert_native_balance",
  "network": "testnet",
  "results": [
    {
      "scenario": "assert_native_balance",
      "step_index": 0,
      "account": "deployer",
      "network": "testnet",
      "asset": "native",
      "expected_min_balance": "1",
      "actual_balance": "99282512062",
      "status": "pass",
      "diagnostic": "balance 99282512062 >= expected 1"
    }
  ],
  "skipped_steps": []
}
```

`assert` executes only read-only steps. Any step that would mutate remote
state (funding, payments) is reported under `skipped_steps` and never run.

Human-readable output on stderr:

```bash
stellar-scenario assert examples/assert_balance.yml --output text
```

```
PASS	[0]	deployer on testnet	balance 99282512062 >= expected 1
```

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
| 0 | Success — scenario is valid, plan produced, or all assertions passed |
| 1 | Semantic validation failed, an assertion failed, or a `doctor` check failed |
| 2 | I/O error, YAML parse error, assertion query error, or unexpected internal error |

The split between 1 and 2 matters for CI: a schema or semantic problem is a
bug in the scenario file, while a parse or I/O failure usually means the file
is not YAML at all or is unreadable.

## Execution contract

These rules are enforced by the code and by tests, not just documented.

**`validate` and `plan` are pure.** Neither spawns a subprocess nor contacts
a network. `plan_does_not_mutate_remote_state` asserts the local identity
list is unchanged after planning a scenario that would otherwise fund and pay.

**`assert` runs exactly one read-only command per executable step.** The only
subprocess it spawns is `stellar token balance --id <asset> --account
<identity> --network <n> --output json`. That command performs an RPC read
and changes no ledger state.

**`assert` never executes a mutating step.** `assert_never_invokes_funding_
or_payment` asserts that a scenario containing only `ensure_funded` and
`payment` steps produces zero executed results and two skipped steps.

**A balance query is not a transaction.** `stellar token balance` reads state
through the RPC. It is not a simulation and not a confirmation. Its result
must never be reported as either.

**Live submission requires explicit intent.** Any future live execution will
require an explicit `run` invocation with a flag that names the action. It
will never happen implicitly during validation, planning, or assertion.

**Secrets stay out.** No command in this toolkit accepts a secret key as
input, and no output path prints one. `plan`, `doctor`, and `assert` output
are checked by tests that scan for 56-character `S`-prefixed tokens, the
shape of a Stellar secret seed.

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
    asset: native              # optional; defaults to native
    min_balance: 100           # optional; integer or string of digits
```

### Actions

| Action | Required fields | Optional fields | Side effect when run |
|--------|-----------------|-----------------|----------------------|
| `ensure_funded` | `identity` | `min_balance` | Mutates remote state |
| `payment` | `from`, `to`, `amount` | `asset` | Mutates remote state |
| `assert_balance` | `identity` | `asset`, `min_balance` | Read only |

### `assert_balance` semantics

- `identity` — the account to query. Accepts a Stellar CLI identity name or
  a `G...` public key. Secret keys are not accepted.
- `asset` — the token to query. Accepts `native`, a classic asset as
  `CODE:ISSUER`, or a contract id or alias. Defaults to `native`.
- `min_balance` — the minimum acceptable balance, in the asset's smallest
  unit (stroops for native XLM). Defaults to `0`. Accepts a YAML integer or
  a quoted string of digits; the string form is preferred for large values.
- Comparison is `actual >= min_balance`, using unsigned 128-bit integer
  arithmetic. No floating-point arithmetic is used anywhere in the comparison.

### Network and account prerequisites

- The network named in `network` must exist in the Stellar CLI config
  (`stellar network ls`). A missing profile produces an error result.
- The account must exist on that network. A missing account produces an
  error result, not a zero balance.
- The Stellar CLI must be reachable and the RPC endpoint must respond.

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

Assertion result:

| Field | Type | Notes |
|-------|------|-------|
| `scenario` | string | Scenario name |
| `step_index` | integer | Zero-based step index |
| `account` | string | Account that was queried |
| `network` | string | Network profile used |
| `asset` | string | Asset identifier |
| `expected_min_balance` | string | Expected minimum, smallest unit |
| `actual_balance` | string or null | Actual balance; null when the query failed |
| `status` | string | `"pass"`, `"fail"`, or `"error"` |
| `diagnostic` | string | Human-readable explanation; never contains secrets |

The `assert` command wraps these in an object with `scenario`, `network`,
`results`, and `skipped_steps`.

## Examples

- `examples/payment.yml` — fund, pay, assert balance
- `examples/auth_setup.yml` — fund an identity for authorization testing
- `examples/assert_balance.yml` — read-only balance assertion

```bash
stellar-scenario validate examples/payment.yml
stellar-scenario plan examples/payment.yml
stellar-scenario assert examples/assert_balance.yml
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
  assert.rs       balance assertion model and execution
tests/
  unit_tests.rs            parsing and validation unit tests
  integration_test.rs      validate command tests
  execution_test.rs        plan and doctor command tests
  assert_test.rs           assert command tests
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
stdin, and argument-safety paths are exercised for real. Tests that need the
Stellar CLI skip cleanly when it is absent, so `cargo test --workspace` stays
green on a bare host.

## CI

`.github/workflows/ci.yml` runs on every push and pull request:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`

## Known limitations

- Live output compatibility for non-native assets is unverified. The parser
  was built against the observed `{"balance":"<stroops>"}` shape for native
  XLM; `CODE:ISSUER` and contract-id assets have not been exercised against
  a live network.
- The `--decimal` output mode is not parsed. Only the raw smallest-unit
  integer form is used, because it avoids floating-point rounding entirely.
- Assertion results are per-step. There is no aggregate scenario verdict
  beyond the process exit code.
- Timeout is a fixed 120 seconds. It is not configurable per scenario.

## Minimum Supported Rust Version

1.80.0. The direct dependencies (`clap` 4.5, `serde` 1.0, `serde_yaml` 0.9,
`serde_json` 1.0, `thiserror` 1.0) all support older versions; 1.80.0 is the
floor chosen for this project and is enforced via `rust-version` in
`Cargo.toml`.

## License

MIT. See [LICENSE](LICENSE).
