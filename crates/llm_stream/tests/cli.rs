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
fn nvidia_without_credentials_names_the_missing_variable() {
    // `run_without_credentials` is not enough on its own: the harness's own
    // environment may carry `NVIDIA_API_KEY`, and the binary inherits it.
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(e) => panic!("could not create a temporary config directory: {e}"),
    };
    let mut command = Command::new(env!("CARGO_BIN_EXE_llm-stream"));
    command
        .arg("--config-dir")
        .arg(dir.path())
        .args(["--api", "nvidia", "hi"])
        .env_remove("NVIDIA_API_KEY")
        .stdin(Stdio::null());
    let output = match command.output() {
        Ok(output) => output,
        Err(e) => panic!("could not run the llm-stream binary: {e}"),
    };

    let ok = output.status.success();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    assert!(
        !ok,
        "expected a failure exit\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("NVIDIA_API_KEY is not set; export it or pass --api-key"),
        "stderr: {stderr}"
    );
    assert!(stdout.is_empty(), "nothing should reach stdout: {stdout}");
}

#[test]
fn reasoning_effort_is_not_filtered_by_clap() {
    // `max` was never in the clap value list, so reaching the provider proves
    // the flag is passed through unparsed rather than validated up front.
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(e) => panic!("could not create a temporary config directory: {e}"),
    };
    let mut command = Command::new(env!("CARGO_BIN_EXE_llm-stream"));
    command
        .arg("--config-dir")
        .arg(dir.path())
        .args(["--api", "nvidia", "--reasoning-effort", "max", "hi"])
        .env_remove("NVIDIA_API_KEY")
        .stdin(Stdio::null());
    let output = match command.output() {
        Ok(output) => output,
        Err(e) => panic!("could not run the llm-stream binary: {e}"),
    };

    let ok = output.status.success();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    assert!(
        !ok,
        "expected a failure exit\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("NVIDIA_API_KEY is not set; export it or pass --api-key"),
        "stderr: {stderr}"
    );
    assert!(
        !stderr.contains("invalid value"),
        "clap must not filter the flag anymore: {stderr}"
    );
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

/// Coverage of the `claude` provider, which spawns a child process rather than
/// opening a socket.
///
/// That is what makes it testable offline where the other providers are not: a
/// stub script on `LLM_STREAM_CLAUDE_BIN` replays a fixture of the JSON Lines
/// the real binary emits, and the whole spawn/parse/print path runs for free.
#[cfg(unix)]
mod claude {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Runs the CLI with `--api claude` pointed at a stub of the given script.
    fn run_with_stub(script: &str, args: &[&str]) -> (bool, String, String) {
        let dir = tempfile::tempdir().expect("could not create a temporary directory");
        let stub = dir.path().join("claude-stub.sh");
        fs::write(&stub, script).expect("could not write the stub");
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755))
            .expect("could not make the stub executable");

        let output = Command::new(env!("CARGO_BIN_EXE_llm-stream"))
            .arg("--config-dir")
            .arg(dir.path())
            .arg("--api")
            .arg("claude")
            .arg("--no-cache")
            .args(args)
            .env("LLM_STREAM_CLAUDE_BIN", &stub)
            .stdin(Stdio::null())
            .output()
            .expect("could not run the llm-stream binary");

        (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }

    /// A stub that drains the prompt and replays `lines` as its own stdout.
    fn replaying(lines: &str) -> String {
        format!("#!/bin/sh\ncat > /dev/null\ncat <<'JSONL'\n{lines}\nJSONL\n")
    }

    fn text_delta(text: &str) -> String {
        format!(
            r#"{{"type":"stream_event","event":{{"type":"content_block_delta","index":1,"delta":{{"type":"text_delta","text":"{text}"}}}}}}"#
        )
    }

    const SUCCESS: &str = r#"{"type":"result","subtype":"success","is_error":false,"result":"x"}"#;

    #[test]
    fn text_deltas_are_joined_into_the_answer() {
        let fixture = format!(
            "{}\n{}\n{SUCCESS}",
            text_delta("Hello, "),
            text_delta("world.")
        );
        let (ok, stdout, stderr) = run_with_stub(&replaying(&fixture), &["hi"]);

        assert!(ok, "stdout: {stdout}\nstderr: {stderr}");
        assert_eq!(stdout, "Hello, world.", "stderr: {stderr}");
    }

    #[test]
    fn thinking_deltas_never_reach_the_answer() {
        // This provider streams text only. A thinking delta shares the
        // `content_block_delta` path, so nothing but the delta's own type tells
        // the two apart.
        let thinking = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"SECRET"}}}"#;
        let fixture = format!("{thinking}\n{}\n{SUCCESS}", text_delta("visible"));
        let (ok, stdout, stderr) = run_with_stub(&replaying(&fixture), &["hi"]);

        assert!(ok, "stdout: {stdout}\nstderr: {stderr}");
        assert_eq!(stdout, "visible", "stderr: {stderr}");
        assert!(!stdout.contains("SECRET"), "stdout: {stdout}");
    }

    #[test]
    fn housekeeping_events_are_ignored() {
        let noise = [
            r#"{"type":"system","subtype":"init","session_id":"x"}"#,
            r#"{"type":"assistant","message":{"content":[]}}"#,
            r#"{"type":"stream_event","event":{"type":"message_start"}}"#,
            r#"{"type":"rate_limit_event","status":"allowed"}"#,
        ]
        .join("\n");
        let fixture = format!("{noise}\n{}\n{SUCCESS}", text_delta("only this"));
        let (ok, stdout, stderr) = run_with_stub(&replaying(&fixture), &["hi"]);

        assert!(ok, "stdout: {stdout}\nstderr: {stderr}");
        assert_eq!(stdout, "only this", "stderr: {stderr}");
    }

    #[test]
    fn the_prompt_reaches_the_child_on_stdin() {
        // The stub answers with whatever it was asked, which is the only way to
        // prove the prompt was piped in rather than dropped.
        let script = "#!/bin/sh\nprompt=$(cat)\nprintf '{\"type\":\"stream_event\",\"event\":{\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"%s\"}}}\\n' \"$prompt\"\n";
        let (ok, stdout, stderr) = run_with_stub(script, &["marco polo"]);

        assert!(ok, "stdout: {stdout}\nstderr: {stderr}");
        assert_eq!(stdout, "marco polo", "stderr: {stderr}");
    }

    #[test]
    fn a_failed_result_line_fails_the_run() {
        // The real binary exits 0 and reports the failure here, so exit status
        // alone would let this look like an empty answer.
        let failure = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"api_error_status":"rate limit exceeded"}"#;
        let (ok, stdout, stderr) = run_with_stub(&replaying(failure), &["hi"]);

        assert!(!ok, "a failed result must fail the run\nstdout: {stdout}");
        assert!(stderr.contains("rate limit exceeded"), "stderr: {stderr}");
    }

    #[test]
    fn a_nonzero_exit_surfaces_the_childs_stderr() {
        let script = "#!/bin/sh\ncat > /dev/null\necho 'the stub refused' >&2\nexit 1\n";
        let (ok, stdout, stderr) = run_with_stub(script, &["hi"]);

        assert!(!ok, "a nonzero exit must fail the run\nstdout: {stdout}");
        assert!(stderr.contains("the stub refused"), "stderr: {stderr}");
    }

    #[test]
    fn a_missing_binary_says_how_to_fix_it() {
        let dir = tempfile::tempdir().expect("could not create a temporary directory");
        let output = Command::new(env!("CARGO_BIN_EXE_llm-stream"))
            .arg("--config-dir")
            .arg(dir.path())
            .args(["--api", "claude", "--no-cache", "hi"])
            .env("LLM_STREAM_CLAUDE_BIN", "llm-stream-no-such-binary")
            .stdin(Stdio::null())
            .output()
            .expect("could not run the llm-stream binary");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "stderr: {stderr}");
        assert!(stderr.contains("LLM_STREAM_CLAUDE_BIN"), "stderr: {stderr}");
    }

    #[test]
    fn ignored_sampling_knobs_are_announced_on_stderr() {
        // Silence here would let an operator believe a preset's temperature was
        // applied. Stderr, not stdout, so a piped answer stays clean.
        let fixture = format!("{}\n{SUCCESS}", text_delta("ok"));
        let (ok, stdout, stderr) = run_with_stub(
            &replaying(&fixture),
            &["--temperature", "0.5", "--top-k", "40", "hi"],
        );

        assert!(ok, "stdout: {stdout}\nstderr: {stderr}");
        assert_eq!(stdout, "ok", "the warning must not reach stdout");
        assert!(stderr.contains("--temperature"), "stderr: {stderr}");
        assert!(stderr.contains("--top-k"), "stderr: {stderr}");
    }
}
