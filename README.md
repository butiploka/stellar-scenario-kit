# Stellar Scenario Kit

Local-first toolkit for defining, validating, and executing reproducible
Stellar/Soroban development scenarios.

A scenario is a versioned YAML file that describes a repeatable development
flow — assert a balance, simulate a contract call, and (in a later milestone)
send a payment. The kit validates those files, plans them against the Stellar
CLI, and runs the read-only and simulated steps. Transaction submission is
not implemented (see [Status](#status) and [Execution contract](#execution-contract)).

No website, dashboard, hosted server, AI API, or paid service is involved.
Everything runs locally.

## Status

Implemented:

- `stellar-scenario validate` — parse and validate a scenario file
- `stellar-scenario plan` — print the exact Stellar CLI commands a run would
  execute, without executing anything
- `stellar-scenario assert` — execute read-only `assert_balance` and
  `assert_asset` steps, with a structured aggregate report
- `stellar-scenario simulate` — build and simulate Soroban invoke envelopes
  without submitting anything
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

### Simulate a transaction

```bash
stellar-scenario simulate examples/simulate_invoke.yml
```

```json
{
  "scenario": "simulate_native_balance",
  "schema_version": "1.0",
  "network": "testnet",
  "total_steps": 1,
  "simulated": 1,
  "unsupported": 0,
  "failed": 0,
  "skipped": 0,
  "status": "simulated",
  "submitted": false,
  "results": [
    {
      "scenario": "simulate_native_balance",
      "step_index": 0,
      "action": "simulate_invoke",
      "network": "testnet",
      "source": "GDIPVO6M4SADGLNFTIMYHCTMMOXNONMV4ORZOMP6OAP2DPPYRQBMAW26",
      "contract_id": "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC",
      "function": "balance",
      "simulated": true,
      "submitted": false,
      "status": "simulated",
      "envelope_sha256": "b7775547cb49791cf3d64c1823f9f0999086e8823d7c2a20871885ba044112b6",
      "resources": {
        "fee": "7266",
        "resource_fee": "7166",
        "instructions": 141406
      },
      "diagnostic": "envelope simulated; not submitted"
    }
  ]
}
```

Human-readable output on stderr:

```bash
stellar-scenario simulate examples/simulate_invoke.yml --output text
```

```
simulate_native_balance on testnet: simulated (1 step(s), 1 simulated, 0 unsupported, 0 failed, 0 skipped) [not submitted]
  SIMULATED	[0]	CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC::balance on testnet	envelope simulated; not submitted
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
| 0 | Success — scenario is valid, plan produced, all assertions passed, or all simulations simulated |
| 1 | Semantic validation failed, an assertion failed, a `doctor` check failed, or a simulation was refused as unsupported |
| 2 | I/O error, YAML parse error, assertion query error, simulation failure, or unexpected internal error |

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
| `simulate_invoke` | `contract_id`, `function` | `source`, `args` | Read only |

### `simulate_invoke` semantics

- `contract_id` — the Soroban contract to invoke, as a `C...` contract id.
- `function` — the function name to invoke.
- `source` — the account the envelope is built for. Accepts an identity name
  or a `G...` public key. Falls back to the scenario-level `source`. Secret
  keys are not accepted.
- `args` — arguments passed to the contract function, as a list of strings in
  the CLI's own `--flag value` form, e.g. `["--to", "world"]`.

The command runs three read-only subprocesses:

1. `stellar contract invoke ... --build-only` — builds the envelope without
   signing or submitting.
2. `stellar tx simulate ...` — simulates it against the RPC.
3. `stellar tx decode` — decodes the simulated envelope to read fee and
   resource figures.

**A simulated result is not a confirmed transaction.** `stellar tx simulate`
asks the RPC to execute the transaction against a ledger snapshot and return
the resources and fees it would need. Nothing is written. The report carries
`"submitted": false` on every result and on the report itself, and the text
summary ends with `[not submitted]`.

**Classic operations are not supported.** `stellar tx simulate` rejects
non-Soroban operations:

```
❌ error: transaction simulation failed: transaction contains unsupported
operation type: OperationTypePayment
```

That case is reported as `status: "unsupported_operation"` with exit code 1,
not as a simulation that ran. Payments, account creation, and other classic
operations are not simulatable through this path.

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

Simulation report (`simulate` command):

| Field | Type | Notes |
|-------|------|-------|
| `scenario` | string | Scenario name |
| `schema_version` | string | Version declared by the scenario file |
| `network` | string | Network profile the run targeted |
| `total_steps` | integer | Total steps declared in the scenario |
| `simulated` | integer | Steps that produced a simulated envelope |
| `unsupported` | integer | Steps refused as unsupported operation types |
| `failed` | integer | Steps that failed for another reason |
| `skipped` | integer | Steps not executed (mutating or non-simulation) |
| `status` | string | `"simulated"`, `"unsupported_operation"`, `"failed"`, or `"no_simulations"` |
| `submitted` | boolean | Always `false` |
| `results` | array | Per-step results, in step order |

Each entry in `results` has `scenario`, `step_index`, `action`, `network`,
`source`, `contract_id`, `function`, `simulated` (always `true`), `submitted`
(always `false`), `status`, `envelope_sha256` (string or null), `resources`,
and `diagnostic`.

`envelope_sha256` is the SHA-256 of the base64 string returned by
`stellar tx simulate` — a content fingerprint of the *encoded* envelope, not a
hash of the decoded XDR bytes. It is not a ledger identifier and is not
comparable to a transaction hash from the network.

`resources` holds `fee`, `resource_fee`, and `instructions` decoded from the
simulated envelope. `resource_fee` is read from `ext.v1.resource_fee`, a
sibling of `ext.v1.resources` in the decoded envelope — not from inside
`resources`.

Aggregation rules:
- `failed` wins over everything (at least one failure → `failed`)
- then `unsupported_operation` (at least one unsupported, no failures)
- then `simulated` (at least one simulated, no failures or unsupported)
- otherwise `no_simulations`

### JSON shape inconsistencies

Two shape differences exist today. Both are documented rather than changed,
because changing either would break consumers of the current output.

**`doctor` returns a top-level array; every other command returns an object.**
`doctor` also has no `schema_version` field, because a bare array has nowhere
to put one. A consumer that parses `doctor` output must handle a JSON array,
not an object.

**`schema_version` means two different things depending on the command.**
- In `validate` output it is the *report format version* this build emits —
  currently always `"1.0"`.
- In `assert` and `simulate` output it is the *scenario's declared*
  `schema_version`, echoed from the file.

They are equal today only because `"1.0"` is the sole accepted scenario
version. If the scenario schema ever moves to `"1.1"`, these two fields would
diverge, and `validate`'s `schema_version` would still read `"1.0"`.

`tests/json_contract_test.rs` pins the current behavior for all of the above,
so any future change is deliberate rather than accidental.

## Examples

- `examples/assert_balance.yml` — read-only balance assertion
- `examples/assert_asset.yml` — existence-only asset assertion
- `examples/payment.yml` — fund, pay, assert balance (mutating steps are skipped by `assert`/`simulate`)
- `examples/auth_setup.yml` — fund an identity for authorization testing (mutating steps are skipped by `assert`/`simulate`)
- `examples/simulate_invoke.yml` — simulate `balance(id)` on the native SAC
- `examples/simulate_decimals.yml` — simulate `decimals()` (zero args, u32)
- `examples/simulate_allowance.yml` — simulate `allowance(from, spender)` (two Address args)

```bash
stellar-scenario validate examples/assert_balance.yml
stellar-scenario plan examples/payment.yml
stellar-scenario assert examples/assert_balance.yml
stellar-scenario assert examples/assert_asset.yml
stellar-scenario simulate examples/simulate_invoke.yml
stellar-scenario simulate examples/simulate_decimals.yml
stellar-scenario simulate examples/simulate_allowance.yml
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

1.80.0. The direct dependencies (`clap` 4.5, `serde` 1.0, `serde_yaml` 0.9,
`serde_json` 1.0, `thiserror` 1.0) all support older versions; 1.80.0 is the
floor chosen for this project and is enforced via `rust-version` in
`Cargo.toml`.

## License

MIT. See [LICENSE](LICENSE).
