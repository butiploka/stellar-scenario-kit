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
| `inspect_contract` | `contract_id` | — | Read only |

### `inspect_contract` semantics

- `contract_id` — the Soroban contract to inspect, as a `C...` contract id
  or alias.
- The command runs `stellar contract info interface --contract-id <id>
  --network <n> --output json` and parses the `SCSpecEntry` array.
- Only `function_v0` and `event_v0` entries are recognized. Any other spec
  version is skipped rather than guessed at.
- The report lists functions and events with their parameter names, types,
  return types, and documentation strings. No transaction is built or
  submitted.

### Scaffolding a step from an interface

```bash
stellar-scenario scaffold <file> <function> [--contract-id <id>]
```

Reads the contract inspected by `<file>`, picks `<function>`, and prints a
YAML `simulate_invoke` step on stdout. The diagnostic goes to stderr, so
`> step.yml` captures only the step.

`--contract-id` is required when the scenario has more than one
`inspect_contract` step; the error lists the candidates.

Generated output for `balance` on the native SAC:

```yaml
# Generated by `stellar-scenario scaffold`.
# Replace every <placeholder> before running.
- action: simulate_invoke
  contract_id: CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC
  function: balance
  args:
    - "--id"
    - "<address>"
```

**Arguments are placeholders, never invented values.** The generator knows
parameter names and types, not what you want to pass. Emitting a plausible
address or amount would read as ready-to-run and fail, or succeed against
the wrong account. Replace every `<placeholder>` before running.

Placeholders by type:

| Soroban type | Placeholder |
|--------------|-------------|
| `address` | `<address>` |
| `muxed_address` | `<muxed-address>` |
| `i32`, `i64`, `i128`, `u32`, `u64`, `u128`, `timepoint`, `duration` | `<integer>` |
| `bool` | `<bool>` |
| `string` | `<string>` |
| `symbol` | `<symbol>` |
| `bytes`, `bytes_n` | `<hex-bytes>` |
| `option<T>` | `<value-or-omit>` |

Container types (`vec<...>`, `map<...>`) and unknown composite types are
refused with an error naming the type and the reason, rather than emitted
with a placeholder that cannot be filled in correctly. Write those steps
by hand.

`option<T>` is supported because the native SAC's own events use it
(`option<string>`, `option<u64>`, `option<bytes_n<32>>`), so the rendered
form is verified rather than guessed. `vec<...>` and `map<...>` are not
supported because no contract available on this host's testnet identities
exposes them, so there is no verified CLI form to generate against.

### Argument format

`stellar contract invoke` takes each argument as `--<name> <value>`, where
`<value>` is JSON for every type except `Bytes` and `BytesN`, which are raw
bytes. The CLI states this in its own help:

```
Usage Notes:
Each arg has a corresponding --<arg_name>-file-path which is a path to a
file containing the corresponding JSON argument.
Note: The only types which aren't JSON are Bytes and BytesN, which are raw bytes
```

The generator emits the inline `--<name> <value>` form. For large or
awkward values, the `--<name>-file-path` variant is the alternative; it is
not generated because the placeholder would be a file path, which is a
different thing to fill in.

Exit codes: `0` generated, `1` function not found or unsupported argument
type or the scenario has no usable `inspect_contract` step, `2` the
interface could not be fetched.

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

### Simulate diagnostics and remediation

Every `simulate_invoke` failure now carries a `diagnostic_category` and a
`remediation` field in JSON, and the text output prints the remediation on
its own line. The categories below were reproduced against Stellar CLI 28.1.0
before being added; anything unrecognized keeps the original message and
`category: "unknown"`.

| Category | When it appears | Remediation |
|----------|-----------------|-------------|
| `contract_not_found` | The contract id does not exist on the network | `check the contract_id; confirm it exists on the target network with stellar contract info interface --id <id> --network <n>` |
| `function_not_found` | The function name is not in the contract interface | `check the function name; run stellar-scenario inspect <file> --output text to list what the contract exposes` |
| `missing_argument` | A required argument was omitted | `add the missing argument; the CLI names it in its error` |
| `invalid_argument` | An argument was present but the wrong type | `check the argument's type; the CLI states the expected type in its error` |
| `network_not_configured` | The network profile is not in the Stellar CLI config | `add the network with stellar network add <name> --rpc-url <url> --network-passphrase <passphrase>, or use one from stellar network ls` |
| `identity_not_configured` | The source identity is not in the Stellar CLI config | `add the identity with stellar keys generate <name>, or use one from stellar keys ls` |
| `rpc_unreachable` | The RPC endpoint could not be reached | `check network connectivity and the RPC URL; confirm with stellar network info` |
| `unsupported_operation` | The envelope is a classic operation | `only Soroban InvokeHostFunction envelopes can be simulated; payments and other classic operations cannot` |
| `host_error` | The contract returned a runtime error | `the contract rejected the call at runtime; the event log in the CLI output names the failing function and reason` |
| `decode_failed` | The simulated envelope could not be decoded | `the simulated envelope could not be decoded; this usually means the CLI version and the envelope disagree` |
| `malformed_decoded_output` | Decoding succeeded but the document is not the expected shape | `the simulated envelope decoded but its structure was not recognized; fee and resource figures are unavailable, though the simulation itself succeeded` |
| `timeout` | The CLI did not finish within `timeout_seconds` | `the Stellar CLI did not finish within the scenario's timeout_seconds; raise it, or check whether the RPC endpoint is responding` |
| `empty_output` | The CLI produced no output at all | `the CLI produced no output; check that STELLAR_CLI points at a working stellar binary` |
| `unknown` | Not recognized | `the CLI message is not recognized; read it directly for the cause` |

**A `timeout` is a local fact, not a diagnosis of the network.** It means the
subprocess was killed after the deadline. It is deliberately *not* reported as
`rpc_unreachable`, because a slow CLI is not proof that the RPC endpoint is
down — the two need different fixes, and conflating them sends the user to
check the wrong thing. `timeout` is derived from the runner's own
`RunnerError::Timeout` variant, never from matching the word "timeout" in CLI
text.

**Secrets are never emitted.** A diagnostic passes through `redact()` before
it reaches JSON or text output. The redactor catches a 56-character
`S`-prefixed seed anywhere in the text, including inside punctuation, so
`(key SAAA...)` becomes `(key <redacted-secret>...)`. The remediation field
is a fixed string per category and cannot carry any CLI text.

### Assertion diagnostics

The `assert` command reports the same `diagnostic_category` and `remediation`
fields on `assert_balance` and `assert_asset` error results. The categories
are the same set, plus two that only a balance query can produce:

| Category | When it appears | Remediation |
|----------|-----------------|-------------|
| `asset_not_deployed` | The classic asset has no Stellar Asset Contract on the network | `the classic asset has no deployed Stellar Asset Contract on this network; use a different asset, or deploy its SAC with stellar contract asset deploy --asset <ASSET>` |
| `trustline_missing` | The account has no trustline for the asset | `the account has no trustline for this asset; a trustline must exist before its balance can be read, and creating one is a ledger change this toolkit does not perform` |

Assertion errors are classified from a different signal than simulation
errors, because the CLI reports them differently:

- A **structured error** (`{"error":{"type":...,"message":...}}` on stdout
  with exit code 0) is classified from the `type` field. The `type` values
  `config`, `invalid_address`, `sac_not_deployed`, and `invoke` map to the
  categories above; any other type stays `unknown`.
- A **non-zero exit** is classified from the CLI's raw stderr, before the
  exit-code prefix is added. Wrapping the message first would push the key
  text past the classifier's prefix checks and turn a recognizable error into
  `unknown`.
- A **subprocess failure** (timeout, spawn failure, missing executable) is
  classified from the runner error variant, so a timeout is a timeout.

A `pass` result never carries a category or remediation, so a consumer that
treats their presence as a failure signal is not misled.

**What classification does not promise.** The categories cover only error
shapes that were reproduced against Stellar CLI 28.1.0. A future CLI version,
a new RPC failure mode, or a differently-worded error will land in `unknown`
with its original message intact — that is the intended behavior, not a gap to
be closed by loosening the matchers. Adding a category requires reproducing
the error first; a plausible-looking string match is not evidence.

### Inspect diagnostics

The `inspect` command reports the same `diagnostic_category` and `remediation`
fields on failed inspection results. The category list is the same set as
[simulation diagnostics](#simulate-diagnostics-and-remediation).

Inspect classifies failures differently than simulation or assertion:

- A **non-zero exit** is classified from the CLI's raw stderr/stdout, before
  the exit-code prefix is added. Wrapping the message first would push the key
  text past the classifier's prefix checks.
- A **subprocess failure** (missing CLI, timeout, spawn failure) is classified
  from the runner error variant, so a timeout is a timeout.
- **Parse failures** (`parse_failed`, `unexpected_shape`) carry no category:
  these describe "output was not the expected shape", which is a distinct
  status, not a failure cause. No existing category describes that semantic.

An `inspected` success result never carries a category or remediation, so a
consumer that treats their presence as a failure signal is not misled.

### `assert_balance` semantics

- `identity` — the account to query. Accepts a Stellar CLI identity name or
  a `G...` public key. Secret keys are not accepted.
- `asset` — the token to query. Accepts `native`, a classic asset as
  `CODE:ISSUER`, or a contract id or alias. Defaults to `native`.
- `min_balance` — the minimum acceptable balance, in the asset's smallest
  unit (stroops for native XLM). **Required.** Accepts a YAML integer or a
  quoted string of digits; the string form is preferred for large values.
  Values above `u128::MAX` (`340282366920938463463374607431768211455`) are
  rejected at validation time rather than at comparison time.
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
| Non-zero exit, stderr present | Error result: `stellar CLI exited with code N: <stderr>` |
| Non-zero exit, both streams empty | Error result: `stellar CLI exited with code N: <no output>` |

A non-zero exit is checked **before** the output is parsed. A failed command
may emit nothing or a partial document; reading that as "malformed JSON"
would hide the real cause, so the exit code and the CLI's stderr are reported
instead. When both streams are non-empty, stderr wins, because that is the
CLI's error channel.

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
`status` (`pass`, `fail`, or `error`), and `diagnostic`. On a classified
error it also carries `diagnostic_category` and `remediation`; both are
omitted on `pass` and `fail`, and on an error whose cause was not recognized.
See [Assertion diagnostics](#assertion-diagnostics).

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
`diagnostic`, and — on a classified failure — `diagnostic_category` and
`remediation`.

`diagnostic_category` and `remediation` are omitted on success, so a consumer
that treats their presence as a failure signal is not misled. The one
exception is `malformed_decoded_output`: the simulation succeeded, so
`status` is `simulated`, but the category is still present because the fee and
resource figures could not be read. See
[Simulate diagnostics and remediation](#simulate-diagnostics-and-remediation)
for the category list.

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
