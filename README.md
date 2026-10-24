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
- `stellar-scenario assert` — execute read-only `assert_balance` and
  `assert_asset` steps, with a structured aggregate report
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

### Assert balances or asset existence

```bash
stellar-scenario assert examples/assert_balance.yml
```

```json
{
  "scenario": "assert_native_balance",
  "schema_version": "1.0",
  "network": "testnet",
  "total_steps": 1,
  "passed": 1,
  "failed": 0,
  "errors": 0,
  "skipped_mutating": 0,
  "skipped_not_executable": 0,
  "status": "passed",
  "results": [
    {
      "scenario": "assert_native_balance",
      "step_index": 0,
      "action": "assert_balance",
      "mode": "min_balance",
      "account": "deployer",
      "network": "testnet",
      "asset": "native",
      "expected_min_balance": "1",
      "actual_balance": "99282512062",
      "status": "pass",
      "diagnostic": "balance 99282512062 >= expected 1"
    }
  ],
  "skipped": []
}
```

`assert` executes only read-only steps. Any step that would mutate remote
state (funding, payments) is reported under `skipped` with reason
`would_mutate` and never run. Read-only steps that have no executor yet are
reported with reason `not_executable`.

Human-readable output on stderr:

```bash
stellar-scenario assert examples/assert_balance.yml --output text
```

```
assert_native_balance on testnet: passed (1 step(s), 1 passed, 0 failed, 0 error(s), 0 skipped)
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
timeout_seconds: 120           # optional; 1..=3600, defaults to 120
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
    min_balance: 100           # required; integer or string of digits

  - action: assert_asset
    identity: carol
    asset: native              # existence-only; no min_balance
```

### Timeout

`timeout_seconds` is a top-level field that applies to every subprocess the
scenario spawns. It is deliberately scenario-level rather than per-step: the
only executable action today is a single RPC read, and a per-step timeout
would add schema surface with no concrete use case.

- Default when omitted: 120 seconds.
- Allowed range: `1` to `3600` inclusive. Values outside the range are
  rejected at validation time with an actionable error.
- The bounds are policy, not technical limits. The floor of 1 second exists
  because a shorter timeout cannot let a real RPC read complete. The ceiling
  of 3600 seconds exists because a longer value is more likely a hung process
  than a slow query, and would pin a CI job for that long.
- The timeout is enforced by the runner's poll-and-kill loop. On expiry the
  child is killed and reaped before the error returns, so no orphan process
  is left behind.
- Omitting the field preserves the previous 120-second behavior exactly. No
  existing scenario changes behavior.

### Actions

| Action | Required fields | Optional fields | Side effect when run |
|--------|-----------------|-----------------|----------------------|
| `ensure_funded` | `identity` | `min_balance` | Mutates remote state |
| `payment` | `from`, `to`, `amount` | `asset` | Mutates remote state |
| `assert_balance` | `identity`, `min_balance` | `asset` | Read only |
| `assert_asset` | `identity` | `asset` | Read only |

### `assert_balance` semantics

- `identity` — the account to query. Accepts a Stellar CLI identity name or
  a `G...` public key. Secret keys are not accepted.
- `asset` — the token to query. Accepts `native`, a classic asset as
  `CODE:ISSUER`, or a contract id or alias. Defaults to `native`.
- `min_balance` — the minimum acceptable balance, in the asset's smallest
  unit (stroops for native XLM). **Required.** Accepts a YAML integer or a
  quoted string of digits; the string form is preferred for large values.
- Comparison is `actual >= min_balance`, using unsigned 128-bit integer
  arithmetic. No floating-point arithmetic is used anywhere in the comparison.
- A `pass` result proves that, at the moment the RPC read returned, the named
  account held at least `min_balance` of the named asset on the named
  network. It does not prove the balance is still that value later, and it
  is not a transaction confirmation.

### `assert_asset` semantics

- `identity` — the account to query.
- `asset` — the token to query. Defaults to `native`.
- A `pass` result proves that the RPC returned a balance entry for the named
  account and asset. **It does not prove the balance is non-zero.** The RPC
  returns a balance entry with value `0` for a trustline that exists but
  holds no funds, and that is a pass. Use `assert_balance` with an explicit
  `min_balance` when the value matters.
- A `fail` result is not possible for this mode: the query either returns an
  entry (pass) or errors.

### Balance value handling

The parser reads the raw smallest-unit integer string the CLI reports and
rejects anything that is not a non-negative decimal integer. A value
containing a decimal point, sign, exponent, separator, or non-ASCII digit is
an error result, never a coerced zero — a parser failure must not become a
passing assertion.

| Response shape | Outcome |
|----------------|---------|
| `{"balance":"12345"}` | Parsed as the raw balance |
| `{"error":{"type":"...","message":"..."}}` | Error result carrying the CLI's type and message |
| Malformed JSON | Error result naming the parse failure |
| `{"decimals":7}` (no `balance`) | Error result: missing `balance` field |
| `{"balance":""}` | Error result: balance is empty |
| `{"balance":"12.5"}` | Error result: not a non-negative decimal integer |
| `{"balance":"-1"}` | Error result: not a non-negative decimal integer |

### Network and account prerequisites

- The network named in `network` must exist in the Stellar CLI config
  (`stellar network ls`). A missing profile produces an error result.
- The account must exist on that network. A missing account produces an
  error result, not a zero balance.
- The Stellar CLI must be reachable and the RPC endpoint must respond.

### Versioning and backwards compatibility

`schema_version: "1.0"` is the current and only version. `min_balance` was
previously optional for `assert_balance` (defaulting to `0`); it is now
required. Existing scenarios that omit `min_balance` will fail validation
with a clear message suggesting `assert_asset` for existence-only checks.

No other fields were made required, and no fields were removed. Scenarios
that already specify `min_balance` continue to validate and behave exactly
as before.

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

Assertion report (`assert` command):

| Field | Type | Notes |
|-------|------|-------|
| `scenario` | string | Scenario name |
| `schema_version` | string | Version declared by the scenario file |
| `network` | string | Network profile the run targeted |
| `total_steps` | integer | Total steps declared in the scenario |
| `passed` | integer | Assertions that passed |
| `failed` | integer | Assertions that failed |
| `errors` | integer | Assertions that errored |
| `skipped_mutating` | integer | Steps skipped because they would mutate remote state |
| `skipped_not_executable` | integer | Steps skipped because no executor exists yet |
| `status` | string | `"passed"`, `"failed"`, `"errored"`, or `"no_assertions"` |
| `results` | array | Per-step assertion results, in step order |
| `skipped` | array | Per-step skip records, in step order |

Each entry in `results` has `scenario`, `step_index`, `action`, `mode`
(`min_balance` or `exists`), `account`, `network`, `asset`,
`expected_min_balance` (string or null), `actual_balance` (string or null),
`status` (`pass`, `fail`, or `error`), and `diagnostic`.

Each entry in `skipped` has `step_index`, `action`, and `reason`
(`would_mutate` or `not_executable`).

Aggregation rules:
- `errored` wins over everything (at least one error → `errored`)
- then `failed` (at least one failure, no errors → `failed`)
- then `passed` (at least one pass, no failures or errors)
- otherwise `no_assertions`

A skipped step is never counted as a pass. An error is never counted as a
pass.

## Examples

- `examples/payment.yml` — fund, pay, assert balance
- `examples/auth_setup.yml` — fund an identity for authorization testing
- `examples/assert_balance.yml` — read-only balance assertion
- `examples/assert_asset.yml` — existence-only asset assertion

```bash
stellar-scenario validate examples/payment.yml
stellar-scenario plan examples/payment.yml
stellar-scenario assert examples/assert_balance.yml
stellar-scenario assert examples/assert_asset.yml
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
  assert.rs       balance/asset assertion model, execution, and reporting
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

- **Non-native asset support is not live-verified.** No existing
  non-native asset/account pair was available on this host's testnet
  identities, so no successful non-native balance read has been observed.
  The parser accepts the documented `CODE:ISSUER` form and the same
  `{"balance":"<smallest-unit>"}` response shape, but that shape has only
  been confirmed for `native`. See [Non-native asset status](#non-native-asset-status).
- The `--decimal` output mode is not parsed. Only the raw smallest-unit
  integer form is used, because it avoids floating-point rounding entirely.
- Timeout is scenario-level, not per-step.
- `assert_asset` proves only that a balance entry exists; it does not prove
  the balance is non-zero.

### Non-native asset status

Live investigation performed on this host (read-only, no state changed):

- Command syntax confirmed from `stellar token balance --help`: `--id`
  accepts a contract id or alias, `native`, or a classic asset as
  `CODE:ISSUER`.
- Queried `USDC:GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5`
  against all four local testnet identities (`deployer`, `preview`,
  `throwaway-a`, `throwaway-b`).
- Every query returned an error, not a balance:

  ```
  {"error":{"type":"invoke","message":"transaction simulation failed: HostError: Error(Contract, #13) ... \"trustline entry is missing for account\""}}
  ```

  The `#13` host error means the account has no trustline for that asset.
  None of the local identities hold a USDC trustline on testnet, so no
  successful non-native read was possible without creating one — which this
  milestone does not permit.
- The error shape is therefore live-verified for the non-native path; the
  success shape is not.
- The asset identifier is passed through as the literal `CODE:ISSUER`
  string; the CLI resolves it to a Stellar Asset Contract id internally.
- The raw balance is expected to be in that token's smallest unit, which is
  **not** necessarily stroops. A token with `decimals: 6` reports millionths,
  not ten-millionths. The parser does not assume any particular `decimals`
  value and never converts.

## Minimum Supported Rust Version

1.80.0. The direct dependencies (`clap` 4.5, `serde` 1.0, `serde_yaml` 0.9,
`serde_json` 1.0, `thiserror` 1.0) all support older versions; 1.80.0 is the
floor chosen for this project and is enforced via `rust-version` in
`Cargo.toml`.

## License

MIT. See [LICENSE](LICENSE).
