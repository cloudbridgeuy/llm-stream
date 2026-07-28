# cargo-xtask

This dependency-free Cargo subcommand wrapper launches the repository's
xtask binary without requiring a Cargo alias. Install it once from the
workspace root:

~~~sh
cargo install --path xtask/cargo-xtask --locked
~~~

The installed executable accepts the Cargo plugin protocol:

~~~text
cargo-xtask xtask [--rebuild] <xtask arguments>
~~~

It forwards all arguments except every --rebuild flag. Without
--rebuild, it compares target/debug/xtask with every Rust source under
xtask/src/ and xtask/Cargo.toml; a missing or stale binary is rebuilt with
cargo build --manifest-path xtask/Cargo.toml --quiet. Equal timestamps are
fresh. The wrapper then replaces itself with xtask on Unix, preserving
signals and the exit status.

The workspace root comes from CARGO_WORKSPACE_DIR when that value points to
a manifest. Otherwise, the wrapper searches upward from the current
directory. An invalid nonempty environment value therefore does not prevent
use from a nested directory.
