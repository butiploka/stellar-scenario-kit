# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.2.x   | :white_check_mark: |
| < 0.2   | :x:                |

Only the latest release on `main` receives security fixes. Older
releases are not backported unless a fix is explicitly released for them.

## Reporting a Vulnerability

**Do not open a public issue for a security vulnerability.**

Use GitHub's private vulnerability reporting for this repository
(Security → Report a vulnerability), if it is enabled. If private
reporting is not available, contact the maintainer through GitHub
directly and do not publish exploit details until a fix is available.

Please include, when you can:

- The exact command you ran and the scenario file (redact anything
  sensitive — this toolkit never accepts a secret key as input, and
  no output path should print one).
- The JSON output or error text.
- Your `stellar --version` and `cargo --version`.
- The version of `stellar-scenario` you used (`--version`, if available,
  otherwise the release tag or commit).

## Scope notes

This toolkit is deliberately local-first and read-only in its
current scope:

- `validate` and `plan` are pure: they spawn no subprocess and
  contact no network.
- `assert` runs only read-only RPC reads.
- `simulate` builds and simulates envelopes; it never submits
  a transaction.
- No command accepts a Stellar secret seed (`S...`) as input,
  and diagnostics pass through redaction before reaching output.

A report that a mutating step (funding, payment, submission)
ran implicitly during validation, planning, assertion, or
simulation is a security-relevant bug for this project, because
it breaks the execution contract documented in `README.md`.

## Disclosure

The maintainer will confirm receipt, work on a fix, and
coordinate disclosure. Please allow reasonable time for a fix
and a release before publishing details publicly.
