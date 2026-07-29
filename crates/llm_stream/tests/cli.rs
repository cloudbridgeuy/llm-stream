//! Offline coverage of the CLI's surface.
//!
//! Nothing here touches the network or needs credentials — every path asserted
//! below fails (or answers) before a socket is opened — so these run on any
//! machine, in CI, for free. Contrast `live_chatgpt.rs`, which is opt-in.

use std::process::{Command, Stdio};

/// The one sentence a signed-out operator should ever see. Duplicated from
/// `crate::auth::flow::NOT_SIGNED_IN` on purpose: an integration test cannot
/// import from a binary-only crate, and a test that reads the constant it is
/// checking would pass no matter what the constant said.
///
/// The dash is U+2014.
const NOT_SIGNED_IN: &str = "not signed in — run: llm-stream --login";

/// Runs the CLI against an empty, throwaway config directory, so there are no
/// credentials to find. Returns `(exit_ok, stdout, stderr)`.
fn run_without_credentials(args: &[&str]) -> (bool, String, String) {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(e) => panic!("could not create a temporary config directory: {e}"),
    };
    let mut command = Command::new(env!("CARGO_BIN_EXE_llm-stream"));
    command.arg("--config-dir").arg(dir.path());
    command.args(args);
    // Without this the child inherits the harness's stdin, and `parse_args`
    // reads stdin whenever it is not a terminal — the test would hang.
    let output = match command.stdin(Stdio::null()).output() {
        Ok(output) => output,
        Err(e) => panic!("could not run the llm-stream binary: {e}"),
    };
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn a_prompt_without_credentials_names_the_login_command() {
    let (ok, stdout, stderr) = run_without_credentials(&["--api", "chatgpt", "hi"]);
    assert!(
        !ok,
        "expected a failure exit\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stderr.contains(NOT_SIGNED_IN), "stderr: {stderr}");
    // A caller piping stdout must get an empty pipe, not half an error.
    assert!(stdout.is_empty(), "nothing should reach stdout: {stdout}");
}

#[test]
fn models_without_credentials_names_the_login_command() {
    let (ok, stdout, stderr) = run_without_credentials(&["--models"]);
    assert!(
        !ok,
        "expected a failure exit\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stderr.contains(NOT_SIGNED_IN), "stderr: {stderr}");
    // The quota warning must come after the credential check. Warning someone
    // about a cost they were never going to pay is noise that trains them to
    // ignore the warning when it matters.
    assert!(
        !stderr.contains("quota"),
        "no quota warning before the guard: {stderr}"
    );
    assert!(stdout.is_empty(), "nothing should reach stdout: {stdout}");
}
