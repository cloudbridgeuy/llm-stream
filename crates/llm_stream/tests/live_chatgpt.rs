//! One live round trip against a real ChatGPT subscription.
//!
//! Opt-in only. Every run spends a small amount of the operator's subscription
//! quota and requires a completed `llm-stream --login`, so it must never fire
//! from a bare `cargo test` or from CI.
//!
//! It drives the real binary rather than the CLI's internals because the CLI
//! package has no library target — and driving the binary is the better test
//! anyway: it covers argument parsing and the stdout/stderr split, which a
//! direct call to the provider would skip.

use std::process::{Command, Stdio};

/// The environment variable that opts in. Its absence is a skip, never a
/// failure: a machine with no ChatGPT subscription is a normal machine.
const OPT_IN: &str = "LLM_STREAM_LIVE_TEST";

/// A word no plausible refusal or error message contains, so finding it in
/// stdout means a model really answered.
const SENTINEL: &str = "PONG";

#[test]
fn live_chatgpt_round_trip() {
    if std::env::var(OPT_IN).is_err() {
        eprintln!(
            "skipping live_chatgpt_round_trip: set {OPT_IN}=1 to run it (spends subscription quota)"
        );
        return;
    }

    // The default config dir, deliberately: copying `auth.json` into a temp
    // directory risks a refresh-token rotation landing there and orphaning the
    // real credential file. `--no-cache` keeps the only other side effect — a
    // conversation file — from happening.
    let output = match Command::new(env!("CARGO_BIN_EXE_llm-stream"))
        .args([
            "--api",
            "chatgpt",
            "--quiet",
            "true",
            "--no-cache",
            "Reply with exactly one word: PONG",
        ])
        .stdin(Stdio::null())
        .output()
    {
        Ok(output) => output,
        Err(e) => panic!("could not run the llm-stream binary: {e}"),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Both streams go into every failure message. A live failure is something
    // you get one shot at reading before deciding whether to spend more quota.
    assert!(
        output.status.success(),
        "llm-stream exited with {}\nstdout: {stdout}\nstderr: {stderr}",
        output.status
    );
    assert!(
        stdout.contains(SENTINEL),
        "the answer never reached stdout\nstdout: {stdout}\nstderr: {stderr}"
    );
}
