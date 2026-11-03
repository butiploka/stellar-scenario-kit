# Usage

Full command reference for `stellar-scenario`. Back to [README](../README.md).

## Validate a scenario

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

Validation is static: it never invokes the Stellar CLI and never contacts an
RPC endpoint. `plan`, `assert`, and `simulate` share the same validation pass,
so a scenario that fails `validate` fails all three with the same exit code
before any execution begins.

Validation accumulates independent problems. A single run reports every
configuration error it can prove, each with the offending field path, rather
than stopping at the first:

```json
{
  "schema_version": "1.0",
  "result": "invalid",
  "error": "9 validation issue(s)",
  "errors": [
    { "field": "network", "message": "network must not be empty" },
    { "field": "source", "message": "source must not be empty when provided; remove it to use the CLI default" },
    { "field": "steps[2].contract_id", "message": "field 'contract_id' must not be empty" },
    { "field": "steps[2].function", "message": "field 'function' must not be empty" }
  ],
  "file": "examples/broken.yml"
}
```

Structural problems that prevent parsing (invalid YAML, unknown top-level
fields) still fail fast with a single `file`-level diagnostic, because no
further validation would be reliable.

### `contract_id` references

`inspect_contract` and `simulate_invoke` steps name a contract in
`contract_id`. It must be either:

- a `C...` StrKey contract id: 56 characters, starting with `C`, with the
  remaining 55 characters drawn from the base32 alphabet (`A`–`Z`, `2`–`7`),
  or
- an alias: starts with a letter, followed by letters, digits, `_`, or `-`.

Aliases are resolved by the Stellar CLI, not by this toolkit, so any value
matching the alias shape is accepted at validation time. Values that match
neither shape are rejected before execution:

```json
{
  "field": "steps[0].contract_id",
  "message": "contract_id 'not a valid id' is neither a C... contract StrKey (56 chars, base32) nor a valid alias (start with a letter; letters, digits, '_' or '-')"
}
```

## Plan a scenario

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
      "side_effect": "mutates_remote_state",
      "execution_status": "would_mutate",
      "block_reason": "would mutate ledger state; blocked in plan mode"
    },
    {
      "step_index": 1,
      "action": "payment",
      "executable": "/home/ubuntu/.local/bin/stellar",
      "args": ["tx", "new", "payment", "--source-account", "alice", "--destination", "bob", "--amount", "100", "--asset", "native", "--network", "testnet"],
      "side_effect": "mutates_remote_state",
      "execution_status": "would_mutate",
      "block_reason": "would mutate ledger state; blocked in plan mode"
    },
    {
      "step_index": 2,
      "action": "assert_balance",
      "executable": "/home/ubuntu/.local/bin/stellar",
      "args": ["token", "balance", "--id", "native", "--account", "bob", "--network", "testnet", "--output", "json"],
      "side_effect": "read_only",
      "execution_status": "executable",
      "annotation": "balance >= 100"
    }
  ],
  "read_only": false,
  "step_counts": {
    "executable": 1,
    "would_mutate": 2,
    "total": 3
  }
}
```

For `assert_balance` steps, the JSON includes an `annotation` field showing the
threshold condition being checked (`balance >= <value>`). For `assert_asset`
steps, the annotation states the existence check (`assert balance entry exists`)
— it never carries a `min_balance` threshold. For `inspect_contract`
steps, the annotation states what the step discovers (`discover interface
(functions, events)`). For `simulate_invoke` steps, the annotation shows the
function being simulated and its args (`simulate <function> <arg1> <arg2> ...`).
Annotations appear in both JSON and text output, helping reviewers understand a
step without opening the YAML file. Values that resemble secret keys (56
characters, `S`-prefix) are redacted in the annotation while the planned command
retains the original values.

Every planned command carries an `execution_status` of either `executable`
(read-only, safe to run as-is) or `would_mutate` (changes ledger state, so it
is shown for planning only). When a step is `would_mutate`, `block_reason`
explains why it is blocked — currently `would mutate ledger state; blocked in
plan mode`. `plan` never executes a mutating step; it only reports what would
happen. The plan also emits a `step_counts` summary so a reviewer can see the
split at a glance without scanning every command:

```json
"step_counts": { "executable": 1, "would_mutate": 2, "total": 3 }
```

Text output tags each step (`[executable]` / `[would_mutate]`), appends the
block reason, and prints the same summary on its final line:

```text
  [0] [would_mutate] stellar keys fund alice --network testnet  [blocked: would mutate ledger state; blocked in plan mode]
  [1] [would_mutate] stellar tx new payment ...  [blocked: would mutate ledger state; blocked in plan mode]
  [2] [executable] stellar token balance --id native --account bob --network testnet --output json  (balance >= 100)
  summary: 1 executable, 2 would-mutate (3 total)
```

`plan` spawns no subprocess and contacts no network. It is a pure function of
the scenario file plus the resolved CLI path, so it is safe to run anywhere,
including CI. For `simulate_invoke` steps it prints only the first subcommand
(`contract invoke --build-only`); the follow-up `tx simulate` and `tx decode`
calls are not listed. See [simulate_invoke semantics](scenario-format.md#simulate_invoke-semantics)
for the full set of read-only subprocesses a simulation actually performs.

## Assert balances or asset existence

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
  "executed": 1,
  "skipped_mutating": 0,
  "skipped_not_executable": 0,
  "status": "passed",
  "step_counts": {
    "executed": 1,
    "passed": 1,
    "failed": 0,
    "errors": 0,
    "skipped": 0,
    "total": 1
  },
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
      "diagnostic": "balance 99282512062 >= expected 1",
      "execution_status": "executable"
    }
  ],
  "skipped": []
}
```

Every result carries an `execution_status` of `executable` (the step ran) and
omits `block_reason` (it was not blocked). `executed` counts results that ran,
so a blocked step is never counted as a failure.

The top-level report also carries a nested `step_counts` object with the same
information in a single structured value for CI consumers:

```json
{
  "step_counts": {
    "executed": 1,
    "passed": 1,
    "failed": 0,
    "errors": 0,
    "skipped": 2,
    "total": 3
  }
}
```

The counting equations are guaranteed: `executed = passed + failed + errors`,
`skipped = skipped_mutating + skipped_not_executable`, and `total = executed +
skipped`. All flat count fields are retained for backward compatibility; the
nested object supplements them.

For CI pipelines that consume individual assertion results incrementally, use
`--output jsonl`:

```bash
stellar-scenario assert examples/payment.yml --output jsonl
```

Stdout becomes a JSON Lines event stream — one compact JSON object per line,
in step order, with no surrounding prose:

```
{"type":"result","step_index":2,"action":"assert_balance","mode":"min_balance","account":"bob","network":"testnet","asset":"native","expected_min_balance":"0","actual_balance":null,"status":"error","diagnostic":"stellar error invalid_address: Account alias \"bob\" not Found","diagnostic_category":"identity_not_configured","remediation":"add the identity with `stellar keys generate <name>`, or use one from `stellar keys ls`","execution_status":"executable"}
{"type":"skipped","step_index":0,"action":"ensure_funded","reason":"would_mutate","execution_status":"would_mutate","block_reason":"would mutate ledger state; blocked in plan mode"}
{"type":"skipped","step_index":1,"action":"payment","reason":"would_mutate","execution_status":"would_mutate","block_reason":"would mutate ledger state; blocked in plan mode"}
```

Each event carries a `"type"` discriminator: `"result"` for an executed
assertion, `"skipped"` for a blocked or non-executable step. Result events
preserve `status` (`pass`/`fail`/`error`) and `execution_status`. Skipped
events preserve `reason`, `execution_status`, and `block_reason` where
applicable. Each line parses independently as JSON.

After the event stream, the aggregate report is emitted as **one compact JSON
object on stderr** (never on stdout). It includes the existing report fields
and the nested `step_counts`, so consumers get the final tally:

```bash
stellar-scenario assert examples/payment.yml --output jsonl 2>&1 >/dev/null \
  | python3 -c 'import sys,json; print(json.load(sys.stdin)["step_counts"])'
```

Exit codes are unchanged: `0` for passed, `1` for failed, `2` for errored.
`--output json` and `--output text` keep their existing behaviour.

`assert` executes only read-only steps. Any step that would mutate remote
state (funding, payments) is reported under `skipped` with reason
`would_mutate`, `execution_status` of `would_mutate`, a `block_reason` of
`would mutate ledger state; blocked in plan mode`, and never run. Read-only
steps that have no executor yet are reported with reason `not_executable`,
`execution_status` of `executable`, and no `block_reason`.

A scenario with both a blocked step and a passing assertion reports both
distinctly:

```json
{
  "scenario": "mutating_then_assert",
  "total_steps": 2,
  "passed": 1,
  "failed": 0,
  "errors": 0,
  "executed": 1,
  "skipped_mutating": 1,
  "skipped_not_executable": 0,
  "status": "passed",
  "results": [
    {
      "step_index": 1,
      "action": "assert_balance",
      "status": "pass",
      "execution_status": "executable"
    }
  ],
  "skipped": [
    {
      "step_index": 0,
      "action": "ensure_funded",
      "reason": "would_mutate",
      "execution_status": "would_mutate",
      "block_reason": "would mutate ledger state; blocked in plan mode"
    }
  ]
}
```

Human-readable output on stderr:

```bash
stellar-scenario assert examples/assert_balance.yml --output text
```

```
assert_native_balance on testnet: passed (1 step(s), 1 passed, 0 failed, 0 error(s), 0 skipped)
  PASS	[0]	deployer on testnet	balance 99282512062 >= expected 1
```

## Simulate a transaction

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

### `simulate --output jsonl` — event stream

The same contract as `assert --output jsonl`: stdout carries the machine-readable
per-step event stream, stderr carries the compact aggregate report.

```bash
stellar-scenario simulate examples/simulate_invoke.yml --output jsonl
```

Stdout — one compact JSON object per simulation result, in step order, with no
surrounding prose:

```
{"type":"result","scenario":"simulate_native_balance","step_index":0,"action":"simulate_invoke","network":"testnet","source":"GDIPVO6M4SADGLNFTIMYHCTMMOXNONMV4ORZOMP6OAP2DPPYRQBMAW26","contract_id":"CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC","function":"balance","simulated":true,"submitted":false,"status":"simulated","envelope_sha256":"b7775547cb49791cf3d64c1823f9f0999086e8823d7c2a20871885ba044112b6","resources":{"fee":"7266","resource_fee":"7166","instructions":141406},"diagnostic":"envelope simulated; not submitted"}
```

Every event carries `"type": "result"` and the full `SimulationResult` fields,
including `step_index`, `action`, and the original `status` — which keeps the
four statuses distinct and never collapses them into one generic success:

- `"simulated"` — the envelope was built, simulated, and decoded.
- `"unsupported_operation"` — the CLI refused the operation type (e.g. a
  classic payment). Not counted as simulated.
- `"build_failed"` / `"simulate_failed"` / `"decode_failed"` — a step failed for
  another reason.
- `"error"` — a subprocess-level error (missing CLI, timeout, spawn failure).

`simulated` is always `true` and `submitted` is always `false` on a result: the
envelope was *simulated*, never submitted. Simulation never mutates the ledger.

Stderr — the compact aggregate report (never on stdout):

```json
{"scenario":"simulate_native_balance","schema_version":"1.0","network":"testnet","total_steps":1,"simulated":1,"unsupported":0,"failed":0,"skipped":0,"status":"simulated","submitted":false,"results":[{"scenario":"simulate_native_balance","step_index":0,"action":"simulate_invoke","network":"testnet","source":"GDIPVO6M4SADGLNFTIMYHCTMMOXNONMV4ORZOMP6OAP2DPPYRQBMAW26","contract_id":"CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC","function":"balance","simulated":true,"submitted":false,"status":"simulated","envelope_sha256":"b7775547cb49791cf3d64c1823f9f0999086e8823d7c2a20871885ba044112b6","resources":{"fee":"7266","resource_fee":"7166","instructions":141406},"diagnostic":"envelope simulated; not submitted"}],"step_counts":{"simulated":1,"unsupported":0,"failed":0,"skipped":0,"total":1}}
```

`step_counts` mirrors the flat counts (`simulated`, `unsupported`, `failed`,
`skipped`) and adds `total`. The counting equations are:

- `total = simulated + unsupported + failed + skipped`
- `unsupported` steps are counted separately and never folded into `simulated`
- `failed` covers `build_failed`, `simulate_failed`, `decode_failed`, and
  `error` — steps that ran but did not produce a simulated envelope
- `skipped` covers non-`simulate_invoke` steps

On an invalid scenario, stdout stays empty (no step events exist) and the error
report is emitted as one compact JSON object on stderr. Exit codes are unchanged:
`0` for simulated or no simulations, `1` for unsupported operation, `2` for
failed. `--output json` and `--output text` keep their existing behaviour.

## Check the environment

```bash
stellar-scenario doctor
```

```json
[
  { "name": "stellar_cli", "ok": true, "detail": "found at /home/ubuntu/.local/bin/stellar" },
  { "name": "stellar_cli_version", "ok": true, "detail": "stellar 28.1.0 (c0f4d0da891bbf214c08b8c5035ae6db80e9a3bd)" },
  { "name": "stellar_cli_version_ok", "ok": true, "detail": "requires >= 28.1.0; found 28.1.0" },
  { "name": "networks_configured", "ok": true, "detail": "4 network(s): local, futurenet, mainnet, testnet" },
  { "name": "identities_configured", "ok": true, "detail": "4 identity name(s) present" }
]
```

`doctor` runs only read-only Stellar CLI commands (`--version`, `keys ls`,
`network ls`). It reports identity *counts*, never names or keys, and exits
1 if any check fails.

`stellar_cli_version_ok` compares the installed CLI against the version this
toolkit was verified against (`28.1.0`). It reports `ok: false` when the CLI is
older than that floor or its version string cannot be parsed — an older CLI can
emit envelope shapes or error `type` values this crate does not recognize.
Newer CLIs are accepted; the check is a floor, not a pin.

## Exit codes

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


See also: [Scenario format](scenario-format.md) for step semantics, JSON fields, and diagnostics.
