# Project toolbox

The project toolbox is exposed through cargo xtask. Install the cached
wrapper once:

~~~sh
cargo install --path xtask/cargo-xtask --locked
~~~

The wrapper locates the workspace root, checks the modification time of
xtask/src/**/*.rs and xtask/Cargo.toml, and builds target/debug/xtask
when the binary is missing or any input is newer. --rebuild forces that
build and may appear anywhere in the forwarded arguments. If
CARGO_WORKSPACE_DIR is set to an invalid directory, the wrapper falls back
to searching upward for a workspace manifest.

## Commands

The existing project commands remain available:

~~~sh
cargo xtask build
cargo xtask install --name llm-stream --path /usr/local/bin/llm-stream
cargo xtask publish --next-version 0.2.0 --dry-run
cargo xtask github --version 0.2.0
cargo xtask changelog --prev-version 0.1.0 --next-version 0.2.0
~~~

Lint runs these checks in order and stops at the first ordinary failure:

1. cargo fmt -- --check
2. cargo check --workspace --all-targets
3. cargo clippy --workspace --all-targets -- -D warnings
4. cargo test --workspace --all-targets
5. cargo rail unify --check

Cargo Rail is optional. Its check is marked skipped when Cargo reports that
the subcommand or cargo-rail executable is unavailable; other Rail failures
remain failures. Install Rail separately if you want that check enforced.

### CI quality gate

CI uses the repository's pinned Rust toolchain and runs five independent
required checks: formatting, Clippy with warnings denied, all workspace tests
and targets, rustdoc with warnings denied, and cargo-deny. `cargo xtask lint`
remains the local convenience path; optional cargo-rail is not a CI
requirement.

Useful lint options:

~~~sh
cargo xtask lint --fix
cargo xtask lint --verbose
cargo xtask lint --no-fmt --no-rail
cargo xtask lint --install-hooks
cargo xtask lint --uninstall-hooks
cargo xtask lint --hooks-status
~~~

Lint output, including captured command output, is written to
target/xtask-lint.log. Hook mode uses --staged-only, implies --fix, and
re-stages only the Rust paths collected before checks begin.

## Hooks

--install-hooks creates a generated .git/hooks/pre-commit hook that runs
the staged lint pipeline. A foreign existing hook is preserved by renaming it
to a timestamped pre-commit.backup.* file. --uninstall-hooks removes only
the generated hook and refuses to remove foreign content. --hooks-status
reports absent, generated, or foreign.
