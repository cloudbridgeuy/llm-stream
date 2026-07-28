# Manual QA Testing Plan: Xtask Toolbox Convergence

Source plan: .claude/plans/2026-07-28-xtask-toolbox-convergence.md
Generated: 2026-07-28

## Overview

Validate the cached Cargo wrapper, the six xtask command families, the five-check lint pipeline, and safe generated-hook ownership.

## Prerequisites

- Run from the repository root.
- Rust and Cargo must be installed.
- Use a temporary repository for hook lifecycle checks.

## How to Run

- Solo: execute each step manually and compare output to the expected output block.
- With an agent: ask the agent to walk through the QA plan one step at a time, confirming each result before advancing.

## Scenarios

### Scenario 1: Wrapper help and rebuild

Purpose: confirm dispatch forwarding and forced rebuilding.

Steps:

1. Run:
   ~~~sh
   cargo run --manifest-path xtask/cargo-xtask/Cargo.toml -- xtask --help
   ~~~
   Expected output: usage text includes build, install, publish, github, changelog, and lint.

2. Run:
   ~~~sh
   cargo run --manifest-path xtask/cargo-xtask/Cargo.toml -- xtask --rebuild --help
   ~~~
   Expected output: the same help appears after a quiet successful build.

Pass criteria: both commands exit 0 and show all six command families.

### Scenario 2: Lint selection and logging

Purpose: confirm skip flags and the stable log path.

Steps:

1. Run:
   ~~~sh
   cargo xtask lint --no-fmt --no-check --no-clippy --no-test --no-rail
   ~~~
   Expected output: the command exits 0 without running a check.

2. Run:
   ~~~sh
   test -f target/xtask-lint.log && rg '^\[(fmt|check|clippy|test|rail)\] skipped$' target/xtask-lint.log
   ~~~
   Expected output: five skipped entries, one for each check.

Pass criteria: the log exists and records all five checks as skipped.

### Scenario 3: Hook lifecycle and foreign-hook preservation

Purpose: confirm generated ownership without touching the real hook.

Steps:

1. Run:
   ~~~sh
   qa_dir=$(mktemp -d); git -C "$qa_dir" init -q; mkdir -p "$qa_dir/.git/hooks"; printf '%s\n' '#!/bin/sh' 'echo foreign' > "$qa_dir/.git/hooks/pre-commit"; (cd "$qa_dir" && cargo xtask lint --install-hooks)
   ~~~
   Expected output: installation reports a generated pre-commit hook.

2. Run:
   ~~~sh
   find "$qa_dir/.git/hooks" -maxdepth 1 -type f -print | sort
   ~~~
   Expected output: pre-commit and a timestamped pre-commit.backup.* file are present.

3. Run:
   ~~~sh
   (cd "$qa_dir" && cargo xtask lint --hooks-status)
   ~~~
   Expected output: pre-commit hook: generated.

4. Run:
   ~~~sh
   (cd "$qa_dir" && cargo xtask lint --uninstall-hooks)
   ~~~
   Expected output: the generated hook is removed and the backup remains.

Pass criteria: foreign content is preserved throughout.

## Edge Cases

- Run CARGO_WORKSPACE_DIR=/tmp cargo run --manifest-path xtask/cargo-xtask/Cargo.toml -- xtask --help; expected result is normal help through upward workspace fallback.
- Run cargo xtask lint --help; expected output lists --fix, --verbose, all five --no-* flags, and the three hook flags.
- If Cargo Rail is unavailable, a lint run should log Rail as skipped; an ordinary Rail failure should still fail the run.

## Rollback / Cleanup

Remove the temporary QA repository with rm -rf "$qa_dir" after confirming the path is the directory printed by mktemp. Do not remove hooks outside it.
