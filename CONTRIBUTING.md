# Contributing

Thanks for considering a contribution. This document covers what you need to
build, test, and submit a change.

## Prerequisites

- Rust 1.85.0 or newer. The CI runs an MSRV job at exactly 1.85.0, so a
  dependency that raises the real minimum will fail there.
- The Stellar CLI (`stellar`) is optional for development. Most tests skip
  cleanly without it, and one test file drives a fake CLI instead.

## Build and test

```bash
cargo build
cargo test
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

All four must pass before a pull request is ready. CI runs the same commands.

### Testing without the Stellar CLI

Some tests need the real `stellar` binary and skip when it is absent. To
check that path explicitly:

```bash
STELLAR_CLI=/nonexistent/stellar cargo test --workspace
```

`tests/robustness_test.rs` needs no CLI at all: it writes a fake `stellar`
script to a temp directory and drives it through `STELLAR_CLI`. Use that
pattern for new subprocess failure modes rather than adding another
network-dependent test.

## What to change, and what not to

This project is deliberately scoped. Please do not add:

- Transaction submission (`stellar tx send`) or any ledger mutation.
- An `--execute` flag or a generic `run` command.
- A new dependency without a concrete need the standard library or an
  existing dependency cannot meet.
- Changes to a public JSON field name, type, or status vocabulary. If a
  change is genuinely needed, say so in the pull request and update
  `tests/json_contract_test.rs` in the same commit.

## Repository layout

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
  json_contract_test.rs    JSON shape, status vocabulary, exit-code contract
  robustness_test.rs       subprocess failure modes via a fake CLI
  scenarios/               checked-in fixtures
examples/                  documented example scenarios
```

## Adding a scenario fixture

Drop a `.yml` file in `tests/scenarios/`. Naming decides the expectation:

- `invalid_*.yml` must **fail** validation.
- anything else must **pass** validation.

`fixture_integrity` in `tests/integration_test.rs` enforces this by walking
both `tests/scenarios/` and `examples/`, so a broken fixture cannot land
unnoticed. Update the minimum-count assertions at the bottom of that test
when you add a batch.

## Adding a CLI command

1. Add the variant to `Command` in `src/cli.rs` with a doc comment; clap
   turns that comment into `--help` text, so write it for a user.
2. Add a `run_<name>` function in `src/main.rs` and dispatch to it.
3. Return an `ExitCode` consistent with the contract in `src/lib.rs`:
   `0` success, `1` a semantic or assertion failure, `2` an I/O, parse, or
   subprocess error.
4. Print JSON to stdout and human-readable text to stderr. Never mix them.
5. Extend `tests/json_contract_test.rs` so the new shape is pinned.

## Exit code contract

| Code | Meaning |
|------|---------|
| 0 | Success — scenario is valid, plan produced, all assertions passed, or all simulations simulated |
| 1 | Semantic validation failed, an assertion failed, a `doctor` check failed, or a simulation was refused as unsupported |
| 2 | I/O error, YAML parse error, assertion query error, simulation failure, or unexpected internal error |

## Safety rules

These are enforced by code and by tests, not just by convention:

- `validate` and `plan` must stay pure: no subprocess, no network.
- `assert` may run only read-only RPC reads.
- `simulate` may build and simulate, never submit.
- No command may accept a secret key as input, and no output path may print
  one. Diagnostics pass through `redact()` before reaching a report.

## Pull requests

- One concern per pull request.
- Include a test that fails without your change.
- Describe what you ran and what it printed. A green CI run is expected, not
  sufficient evidence on its own.
- Keep the diff small. Refactors unrelated to the change make review harder.

## Reporting a bug

Open an issue with:

- the exact command you ran,
- the scenario file (redact anything sensitive),
- the JSON output or error text,
- your `stellar --version` and `cargo --version`.

## License

Contributions are accepted under the MIT license in `LICENSE`.
