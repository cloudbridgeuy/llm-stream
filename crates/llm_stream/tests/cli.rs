//! Offline coverage of the CLI's surface.
//!
//! Nothing here touches the network or needs credentials — every path asserted
//! below fails (or answers) before a socket is opened — so these run on any
//! machine, in CI, for free. Contrast `live_chatgpt.rs`, which is opt-in.

use std::fs;
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

/// Runs an offline metadata command against a throwaway cache fixture and
/// returns the process output plus the resulting cache text.
fn run_metadata_command(id: &str, body: &str, args: &[&str]) -> (bool, String, String, String) {
    let dir = tempfile::tempdir().expect("could not create a temporary config directory");
    let cache_dir = dir.path().join("cache");
    fs::create_dir_all(&cache_dir).expect("could not create the cache directory");
    fs::write(cache_dir.join(format!("{id}.toml")), body).expect("could not write cache fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_llm-stream"))
        .arg("--config-dir")
        .arg(dir.path())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("could not run the llm-stream binary");
    let updated = fs::read_to_string(cache_dir.join(format!("{id}.toml")))
        .expect("could not read cache fixture");

    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        updated,
    )
}

const CACHE_FIXTURE: &str = r#"# keep this comment
api = "openai"
custom_key = "keep me"

[[conversation]]
role = "user"
content = "hello"
"#;

#[test]
fn set_title_adds_missing_metadata_without_network() {
    let (ok, stdout, stderr, updated) = run_metadata_command(
        "missing-title",
        CACHE_FIXTURE,
        &[
            "--from",
            "missing-title",
            "--set-title",
            "Picked conversation",
            "--api",
            "open-ai",
            "--api-base-url",
            "http://127.0.0.1:1",
        ],
    );

    assert!(ok, "expected success\nstdout: {stdout}\nstderr: {stderr}");
    assert!(
        stdout.is_empty(),
        "metadata commands keep stdout quiet: {stdout}"
    );
    assert!(
        stderr.contains("updated conversation metadata"),
        "stderr: {stderr}"
    );
    assert!(updated.contains("title = \"Picked conversation\""));
    assert!(updated.contains("custom_key = \"keep me\""));
    assert!(updated.contains("content = \"hello\""));
}

#[test]
fn set_title_replaces_existing_metadata_without_network() {
    let body = CACHE_FIXTURE.replace(
        "custom_key = \"keep me\"",
        "title = \"Old title\"\ncustom_key = \"keep me\"",
    );
    let (ok, stdout, stderr, updated) = run_metadata_command(
        "replace-title",
        &body,
        &["--from", "replace-title", "--set-title", "New title"],
    );

    assert!(ok, "expected success\nstdout: {stdout}\nstderr: {stderr}");
    assert!(stdout.is_empty());
    assert!(updated.contains("title = \"New title\""));
    assert!(!updated.contains("title = \"Old title\""));
}

#[test]
fn set_title_and_description_updates_both_without_network() {
    let (ok, stdout, stderr, updated) = run_metadata_command(
        "both",
        CACHE_FIXTURE,
        &[
            "--from",
            "both",
            "--set-title",
            "A title",
            "--set-description",
            "A description",
        ],
    );

    assert!(ok, "expected success\nstdout: {stdout}\nstderr: {stderr}");
    assert!(stdout.is_empty());
    assert!(updated.contains("title = \"A title\""));
    assert!(updated.contains("description = \"A description\""));
}

#[test]
fn set_metadata_can_select_the_latest_conversation_without_network() {
    let (ok, stdout, stderr, updated) = run_metadata_command(
        "latest",
        CACHE_FIXTURE,
        &["--from-last", "--set-title", "Latest conversation"],
    );

    assert!(ok, "expected success\nstdout: {stdout}\nstderr: {stderr}");
    assert!(stdout.is_empty());
    assert!(updated.contains("title = \"Latest conversation\""));
}

#[test]
fn set_metadata_requires_a_conversation_name() {
    let (ok, stdout, stderr) = run_without_credentials(&["--set-title", "Needs a name"]);

    assert!(!ok);
    assert!(stdout.is_empty());
    assert!(stderr.contains("--from or --from-last"), "stderr: {stderr}");
}

#[test]
fn set_metadata_rejects_a_nonexistent_conversation() {
    let (ok, stdout, stderr, _) = run_metadata_command(
        "other",
        CACHE_FIXTURE,
        &[
            "--from",
            "missing",
            "--set-description",
            "No such conversation",
        ],
    );

    assert!(!ok);
    assert!(stdout.is_empty());
    assert!(
        stderr.contains("conversation `missing` does not exist"),
        "stderr: {stderr}"
    );
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

#[test]
fn last_on_its_own_is_not_a_silent_no_op() {
    // `--last` used to only be read inside `show`, which nothing reached
    // unless `--show` was also passed: the flag its help text says prints the
    // last message printed nothing and exited zero. It must now either print
    // or explain itself.
    let (ok, stdout, stderr) = run_without_credentials(&["--last"]);
    assert!(
        !ok,
        "with no conversation named this must fail\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stderr.contains("--from"), "stderr: {stderr}");
    assert!(stdout.is_empty(), "nothing should reach stdout: {stdout}");
}

#[test]
fn version_reports_the_binary_name_and_the_package_version() {
    let output = match Command::new(env!("CARGO_BIN_EXE_llm-stream"))
        .arg("--version")
        .stdin(Stdio::null())
        .output()
    {
        Ok(output) => output,
        Err(e) => panic!("could not run the llm-stream binary: {e}"),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let expected = format!("llm-stream {}", env!("CARGO_PKG_VERSION"));

    // `CARGO_PKG_VERSION` here is the test's own package — the same one the
    // binary is built from — so this stays true across version bumps without
    // anyone remembering to edit it.
    assert_eq!(stdout.trim(), expected, "stdout: {stdout}");
}
