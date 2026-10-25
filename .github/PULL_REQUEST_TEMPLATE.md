name: Pull request

on:
  pull_request_target:
    types: [opened, reopened, synchronize]

body:
  - type: checkboxes
    id: scope
    attributes:
      label: Scope
      description: This project does not implement transaction submission or ledger mutation.
      options:
        - label: This PR does not add transaction submission, account funding, or any ledger mutation
          required: true
        - label: This PR does not change a public JSON field name, type, or status vocabulary
          required: true
        - label: This PR does not add a new dependency without a stated need
          required: true
  - type: textarea
    id: what
    attributes:
      label: What changed
      description: One concern per pull request.
    validations:
      required: true
  - type: textarea
    id: test
    attributes:
      label: Test evidence
      description: Which test fails without this change, and what did you run?
    validations:
      required: true
  - type: textarea
    id: commands
    attributes:
      label: Commands run
      description: fmt, clippy, and test output.
    validations:
      required: true
