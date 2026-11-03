# Stellar Scenario Kit

Local-first toolkit for defining, validating, and executing reproducible
Stellar/Soroban development scenarios.

A scenario is a versioned YAML file that describes a repeatable development
flow — assert a balance, simulate a contract call, and (in a later milestone)
send a payment. The kit validates those files, plans them against the Stellar
CLI, and runs the read-only and simulated steps. Transaction submission is
not implemented (see [Status](#status) and [Execution contract](docs/usage.md#execution-contract)).

No website, dashboard, hosted server, AI API, or paid service is involved.
Everything runs locally.

## Status

Implemented:

- `stellar-scenario validate` — parse and validate a scenario file
- `stellar-scenario plan` — print the first Stellar CLI command each step would
  execute, without executing anything
- `stellar-scenario assert` — execute read-only `assert_balance` and
  `assert_asset` steps, with a structured aggregate report
- `stellar-scenario simulate` — build and simulate Soroban invoke envelopes
  without submitting anything
- `stellar-scenario inspect` — read a contract's functions and events
- `stellar-scenario scaffold` — generate a `simulate_invoke` step from a
  contract interface, with placeholder arguments
- `stellar-scenario doctor` — report environment readiness
- Versioned schema (`schema_version: "1.0"`)
- Semantic validation of steps, actions, and fields
- Actionable errors naming the file, field, and line
- Stable JSON and text output
- Deterministic exit codes
- Process execution adapter with timeouts and structured errors
- JSON output contract tests pinning every command's public shape
- Unit and integration tests with checked-in fixtures

Not yet implemented (deferred deliberately):

- Transaction submission (`stellar tx send`)
- Automatic account funding (`stellar keys fund`)
- Any dependency on the `soroban` binary — the `stellar` CLI is sufficient
  for the current scope, and no execution path has been verified yet

## Installation

Requires Rust 1.85.0 or newer.

From crates.io (once published):

```bash
cargo install stellar-scenario
```

From source:

```bash
git clone https://github.com/butiploka/stellar-scenario-kit.git
cd stellar-scenario-kit
cargo install --path .
```

Or build without installing:

```bash
cargo build --release
# binary at target/release/stellar-scenario
```

The MSRV is enforced by a CI job at exactly 1.85.0, so a dependency bump that
raises the real minimum fails there rather than in your build.

### Prerequisites

- The Stellar CLI (`stellar`) must be on `PATH`, or `STELLAR_CLI` must point
  at it. `validate` and `plan` work without it; `assert`, `simulate`, and
  `doctor` need it.
- `assert` and `simulate` contact a Stellar network over RPC. The network
  named in the scenario must exist in `stellar network ls`. `testnet` is
  configured by default.
- `assert` and `simulate` read existing on-chain state. They never create
  accounts, fund accounts, or submit transactions.

Check your setup:

```bash
stellar-scenario doctor
```

## Quick start

```bash
# 1. Validate a scenario — pure local, no network
stellar-scenario validate examples/assert_balance.yml

# 2. See exactly what a run would execute — pure local, no network
stellar-scenario plan examples/assert_balance.yml

# 3. Assert a balance against the ledger — read-only RPC
stellar-scenario assert examples/assert_balance.yml

# 4. Simulate a Soroban call — read-only RPC, no submission
stellar-scenario simulate examples/simulate_decimals.yml
```

Every command prints JSON on stdout by default and accepts `--output text`
for a human-readable summary on stderr.

### Discover a contract, then scaffold a step

```bash
# What functions does this contract expose?
stellar-scenario inspect examples/inspect_contract.yml --output text

# Generate a simulate_invoke step for one of them
stellar-scenario scaffold examples/inspect_contract.yml balance > step.yml

# Paste step.yml under `steps:` in your scenario, then replace the
# placeholders and validate.
```

`scaffold` prints YAML on stdout and its diagnostic on stderr, so
`> step.yml` captures only the step. The generated step always validates;
what it does not do is carry real values, because the generator knows
parameter names and types but not what you want to pass.

## Commands

| Command | What it does |
| ------- | ------------ |
| `validate` | Parse and validate a scenario file. See [Usage](docs/usage.md#validate-a-scenario). |
| `plan` | Print the first Stellar CLI command each step would execute, without executing anything. See [Usage](docs/usage.md#plan-a-scenario). |
| `assert` | Execute read-only `assert_balance` and `assert_asset` steps. See [Usage](docs/usage.md#assert-balances-or-asset-existence). |
| `simulate` | Build and simulate Soroban invoke envelopes without submitting anything. See [Usage](docs/usage.md#simulate-a-transaction). |
| `inspect` | Read a contract's functions and events. See [Scenario format](docs/scenario-format.md#inspect_contract-semantics). |
| `scaffold` | Generate a `simulate_invoke` step from a contract interface, with placeholder arguments. See [Scenario format](docs/scenario-format.md#scaffolding-a-step-from-an-interface). |
| `doctor` | Report environment readiness. See [Usage](docs/usage.md#check-the-environment). |

Exit codes are documented in [Usage](docs/usage.md#exit-codes), and the
no-submission / no-mutation guarantees in [Execution contract](docs/usage.md#execution-contract).

## Documentation

- [Usage](docs/usage.md) — full command reference and execution contract
- [Scenario format](docs/scenario-format.md) — scenario file format, step semantics, JSON output fields, and diagnostics
- [SECURITY.md](SECURITY.md) — supported versions and vulnerability reporting
- [CHANGELOG.md](CHANGELOG.md) — release history
- [CONTRIBUTING.md](CONTRIBUTING.md) — build steps and contribution rules

## Examples

- `examples/assert_balance.yml` — read-only balance assertion
- `examples/assert_asset.yml` — existence-only asset assertion
- `examples/payment.yml` — fund, pay, assert balance (mutating steps are skipped by `assert`/`simulate`)
- `examples/auth_setup.yml` — fund an identity for authorization testing (mutating steps are skipped by `assert`/`simulate`)
- `examples/simulate_invoke.yml` — simulate `balance(id)` on the native SAC
- `examples/simulate_decimals.yml` — simulate `decimals()` (zero args, u32)
- `examples/simulate_allowance.yml` — simulate `allowance(from, spender)` (two Address args)
- `examples/inspect_contract.yml` — inspect the native SAC interface

```bash
stellar-scenario validate examples/assert_balance.yml
stellar-scenario plan examples/payment.yml
stellar-scenario assert examples/assert_balance.yml
stellar-scenario assert examples/assert_asset.yml
stellar-scenario simulate examples/simulate_invoke.yml
stellar-scenario simulate examples/simulate_decimals.yml
stellar-scenario simulate examples/simulate_allowance.yml
stellar-scenario inspect examples/inspect_contract.yml --output text
stellar-scenario scaffold examples/inspect_contract.yml balance
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
  simulate.rs     transaction simulation model, execution, and reporting
tests/
  unit_tests.rs            parsing and validation unit tests
  integration_test.rs      validate command tests
  execution_test.rs        plan and doctor command tests
  assert_test.rs           assert command tests
  simulate_test.rs         simulate command tests
  json_contract_test.rs    JSON shape, status vocabulary, and exit-code contract
  robustness_test.rs       subprocess failure modes via a fake CLI
  scenarios/               checked-in fixtures
examples/                  documented example scenarios
```

### Adding a fixture

Any file under `tests/scenarios/` whose name starts with `invalid_` must fail
validation; every other `.yml` file under `tests/scenarios/` and `examples/`
must pass. The `fixture_integrity` test enforces this, so a broken fixture
cannot enter the repository unnoticed.

### JSON output contract

`tests/json_contract_test.rs` pins the public shape of every command's JSON:
field names, types, closed status vocabularies, and exit-code mapping. It
also asserts that stdout is parseable JSON in json mode and empty in text
mode, so a command can never mix the two. When changing a public field,
update that test in the same commit.

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
4. `cargo test --workspace` again with `STELLAR_CLI` pointing at a
   nonexistent path, which is the state a contributor without the Stellar
   CLI will hit
5. `cargo check --workspace --all-targets` on Rust 1.85.0, pinning the MSRV

The Stellar CLI is not installed on the runner, so tests that need it skip.
That is why step 4 exists: it proves the suite is green in that state rather
than assuming it.

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

### Verified simulation signatures

Live-verified against the native Stellar Asset Contract
(`CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC`) on testnet,
using a pre-existing funded account. All three were built with
`--build-only`, simulated with `stellar tx simulate`, and decoded with
`stellar tx decode`. Nothing was submitted.

| Function | Arguments | Return | fee | resource_fee | instructions |
|----------|-----------|--------|-----|--------------|--------------|
| `balance` | one `Address` | `i128` | 7266 | 7166 | 141406 |
| `decimals` | none | `u32` | 6226 | 6126 | 109125 |
| `allowance` | two `Address` | `i128` | 6923 | 6823 | 167318 |

These three cover the argument shapes the `args` field can express today:
zero arguments, and one or more `--flag value` string pairs. The contract
interface was read with `stellar contract info interface`, which lists the
full SEP-41 surface including `approve`, `burn`, `mint`, `transfer`, and
`clawback` — all of which mutate state and are therefore out of scope.

Argument type errors are surfaced, not coerced. Passing a number where an
`Address` is expected fails at envelope build:

```
❌ error: Failed to parse argument 'id': invalid type: number, expected string or map
Context: Expected type address (G... for account, C... for contract, or identity name), but received: '12345'
```

That case is reported as `status: "build_failed"` with exit code 2. An
unknown function name fails at build with `unrecognized subcommand`, also
`build_failed`.

## Minimum Supported Rust Version

**1.85.0**, verified by building and running the full test suite on that
toolchain, and enforced by a CI job pinned to it.

The floor is set by the resolved dependency graph, not by the direct
dependencies' own minimums. `clap_lex` 1.1.1 (a transitive dependency of
`clap` 4.6) requires the `edition2024` Cargo feature, which is not
stabilized before 1.85.0. An earlier claim of 1.80.0 was wrong: that
toolchain cannot even parse the lockfile.

`rust-version` in `Cargo.toml` is set to the tested value. If a future
dependency bump raises the real minimum, the MSRV CI job fails rather than
users discovering it in their own build.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for build steps, the exit-code
contract, what is deliberately out of scope, and how to add a fixture or a
command.

## License

MIT. See [LICENSE](LICENSE).
