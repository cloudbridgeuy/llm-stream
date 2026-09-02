//! The `claude` provider: Claude Code's own CLI, streamed.
//!
//! Every other provider talks to an HTTP endpoint. This one talks to a child
//! process: it spawns the `claude` binary in print mode and reads the JSON
//! Lines it writes to stdout. That is also why it is the one provider with no
//! half in `lib/llm_stream` — that crate is deliberately free of a Tokio
//! dependency, and a child process needs one. Everything here stays in the CLI
//! crate, which already has Tokio.
//!
//! The child is asked for `stream-json` with `--include-partial-messages`, so
//! the answer arrives as `content_block_delta` events carrying `text_delta`
//! payloads. Those deltas, and nothing else, become the stream this module
//! hands to `handle_stream`. `thinking_delta` events are dropped on the floor:
//! this provider streams text only.
//!
//! `claude` is an agent, not a completion endpoint. Left alone it would load
//! the operator's settings, hooks, `CLAUDE.md` files and MCP servers before
//! answering — measured on 2026-08-10, a bare `claude -p` emitted 35 `system`
//! events and a `SessionStart` hook payload ahead of the first token. The flag
//! set in `cli_args` cuts that to 5 events and no hooks, which is as close to a
//! plain model call as the binary gets.

use futures::stream::Stream;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStderr, ChildStdout, Command};

use crate::prelude::*;

/// The binary to spawn, when `LLM_STREAM_CLAUDE_BIN` says nothing else.
const DEFAULT_BIN: &str = "claude";

/// Overrides `DEFAULT_BIN`. Useful when `claude` is not on `PATH`, or when a
/// test wants to point the provider at a stub.
const BIN_ENV: &str = "LLM_STREAM_CLAUDE_BIN";

/// The knobs this provider cannot honour.
///
/// The CLI exposes no sampling controls, so `--temperature` and friends have
/// nowhere to go. Dropping them silently would let an operator believe a preset
/// was applied when it was not, so say so once, on stderr, where it cannot
/// contaminate a piped answer.
fn warn_ignored(args: &Args) {
    let mut ignored = Vec::new();

    if args.temperature.is_some() {
        ignored.push("--temperature");
    }
    if args.top_p.is_some() {
        ignored.push("--top-p");
    }
    if args.top_k.is_some() {
        ignored.push("--top-k");
    }
    if args.max_tokens.is_some() {
        ignored.push("--max-tokens");
    }
    if args.min_tokens.is_some() {
        ignored.push("--min-tokens");
    }

    if !ignored.is_empty() {
        eprintln!(
            "warning: the claude provider ignores {} — the CLI exposes no sampling controls",
            ignored.join(", ")
        );
    }
}

/// The arguments handed to the child, in order.
///
/// Split out from `run` so the flag set can be asserted in a test rather than
/// only in a live call. The first four are what turn the binary into a stream:
/// `stream-json` needs `--verbose` to be accepted alongside `--print`, and
/// without `--include-partial-messages` the answer arrives in one lump at the
/// end rather than as deltas.
///
/// The rest are containment. `--setting-sources ''` is the load-bearing one: it
/// loads no user, project or local settings, which is what keeps the operator's
/// hooks and `CLAUDE.md` out of an answer they asked a model for.
fn cli_args(model: Option<&str>, system: Option<&str>) -> Vec<String> {
    let mut argv = vec![
        "--print".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--include-partial-messages".to_string(),
        "--verbose".to_string(),
        "--no-session-persistence".to_string(),
        "--strict-mcp-config".to_string(),
        "--setting-sources".to_string(),
        String::new(),
    ];

    if let Some(model) = model {
        argv.push("--model".to_string());
        argv.push(model.to_string());
    }

    // `--system-prompt` replaces Claude Code's own agent prompt, where
    // `--append-system-prompt` would leave it in place. Replacing is right
    // here: the operator asked for a model, not for a coding agent.
    if let Some(system) = system {
        argv.push("--system-prompt".to_string());
        argv.push(system.to_string());
    }

    argv
}

/// Flattens a conversation into the single prompt string the CLI accepts.
///
/// `claude -p` takes one prompt and has no way to be handed a prior assistant
/// turn, so multi-turn history has to be rendered into the prompt itself. A
/// lone user message — the common case — passes through untouched; only a real
/// back-and-forth gets the `Human:`/`Assistant:` scaffolding.
///
/// `System` messages are skipped. They reach the child through
/// `--system-prompt` instead, which is how `anthropic::run` treats them too.
fn flatten_prompt(conversation: &[ConversationMessage]) -> String {
    let turns: Vec<&ConversationMessage> = conversation
        .iter()
        .filter(|message| message.role != ConversationRole::System)
        .collect();

    match turns.as_slice() {
        [] => String::new(),
        [only] => only.content.clone(),
        many => many
            .iter()
            .map(|message| {
                let speaker = match message.role {
                    ConversationRole::Assistant => "Assistant",
                    _ => "Human",
                };
                format!("{speaker}: {}", message.content)
            })
            .collect::<Vec<String>>()
            .join("\n\n"),
    }
}

/// The answer text carried by one JSONL line, if it carries any.
///
/// Navigating a `Value` rather than deserializing a struct is deliberate: the
/// child emits a dozen event shapes we have no interest in, and a typed parse
/// would have to model all of them to avoid failing on the ones it does not
/// know. Anything that is not the exact path we want is simply not a delta.
fn text_delta(line: &str) -> Option<String> {
    let value: Value = serde_json::from_str(line).ok()?;

    if value.get("type")?.as_str()? != "stream_event" {
        return None;
    }

    let event = value.get("event")?;

    if event.get("type")?.as_str()? != "content_block_delta" {
        return None;
    }

    let delta = event.get("delta")?;

    // `thinking_delta` shares this path. Text only, so it is filtered here.
    if delta.get("type")?.as_str()? != "text_delta" {
        return None;
    }

    Some(delta.get("text")?.as_str()?.to_string())
}

/// The failure a `result` line reports, if it reports one.
///
/// The child exits 0 on a refusal or an API error and says so in this line
/// instead, so exit status alone would let a failed run look like an empty
/// answer.
fn result_error(line: &str) -> Option<String> {
    let value: Value = serde_json::from_str(line).ok()?;

    if value.get("type")?.as_str()? != "result" {
        return None;
    }

    if !value
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }

    // In order of how much they tell an operator: the server's own status, the
    // result text, then the subtype, which is at least a category.
    let detail = value
        .get("api_error_status")
        .and_then(Value::as_str)
        .or_else(|| value.get("result").and_then(Value::as_str))
        .or_else(|| value.get("subtype").and_then(Value::as_str))
        .unwrap_or("the claude CLI reported an error with no detail");

    Some(detail.to_string())
}

/// What `unfold` carries between polls.
struct StreamState {
    lines: Lines<BufReader<ChildStdout>>,
    stderr: Option<ChildStderr>,
    child: Child,
    finished: bool,
}

/// Everything the child wrote to stderr, for an error message.
async fn drain_stderr(stderr: Option<ChildStderr>) -> String {
    let Some(mut stderr) = stderr else {
        return String::new();
    };

    let mut buffer = String::new();
    let _ = tokio::io::AsyncReadExt::read_to_string(&mut stderr, &mut buffer).await;
    buffer.trim().to_string()
}

/// Turns the child's stdout into the stream `handle_stream` consumes.
fn delta_stream(
    state: StreamState,
) -> impl Stream<Item = std::result::Result<String, llm_stream::error::Error>> + Unpin {
    Box::pin(futures::stream::unfold(state, |mut state| async move {
        if state.finished {
            return None;
        }

        loop {
            match state.lines.next_line().await {
                Ok(Some(line)) => {
                    log::debug!("claude line: {line}");

                    if let Some(text) = text_delta(&line) {
                        return Some((Ok(text), state));
                    }

                    if let Some(detail) = result_error(&line) {
                        state.finished = true;
                        return Some((Err(llm_stream::error::Error::ApiError(detail)), state));
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    state.finished = true;
                    return Some((Err(llm_stream::error::Error::IO(e)), state));
                }
            }
        }

        // stdout is closed: the child is done talking. Its exit status decides
        // whether that was an answer or a failure.
        state.finished = true;

        let status = match state.child.wait().await {
            Ok(status) => status,
            Err(e) => return Some((Err(llm_stream::error::Error::IO(e)), state)),
        };

        if status.success() {
            return None;
        }

        let stderr = drain_stderr(state.stderr.take()).await;
        let detail = if stderr.is_empty() {
            format!("the claude CLI exited with {status}")
        } else {
            stderr
        };

        Some((Err(llm_stream::error::Error::ApiError(detail)), state))
    }))
}

pub async fn run(mut args: Args) -> Result<()> {
    warn_ignored(&args);

    let binary = std::env::var(BIN_ENV).unwrap_or_else(|_| DEFAULT_BIN.to_string());
    let model = args.model.take();
    let system = args.system.take();
    let argv = cli_args(model.as_deref(), system.as_deref());

    log::info!("binary: {binary}");
    log::info!("argv: {argv:#?}");

    let prompt = flatten_prompt(&args.conversation);

    log::info!("prompt: {prompt}");

    let mut child = Command::new(&binary)
        .args(&argv)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::InvalidValue(format!(
                    "could not run `{binary}` — install Claude Code, or point {BIN_ENV} at the binary"
                ))
            } else {
                Error::Io(e)
            }
        })?;

    // The prompt goes in on a task rather than inline: a prompt larger than the
    // pipe buffer would otherwise block this side before anything starts
    // reading the other one.
    if let Some(mut stdin) = child.stdin.take() {
        tokio::spawn(async move {
            let _ = stdin.write_all(prompt.as_bytes()).await;
            let _ = stdin.shutdown().await;
        });
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::InvalidValue("the claude CLI gave us no stdout".to_string()))?;
    let stderr = child.stderr.take();

    let stream = delta_stream(StreamState {
        lines: BufReader::new(stdout).lines(),
        stderr,
        child,
        finished: false,
    });

    handle_stream(stream, args).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(role: ConversationRole, content: &str) -> ConversationMessage {
        ConversationMessage {
            role,
            content: content.to_string(),
        }
    }

    #[test]
    fn cli_args_ask_for_a_partial_message_stream() {
        let argv = cli_args(None, None);

        for flag in [
            "--print",
            "--include-partial-messages",
            "--verbose",
            "--no-session-persistence",
            "--strict-mcp-config",
        ] {
            assert!(
                argv.iter().any(|a| a == flag),
                "{flag} missing from {argv:?}"
            );
        }

        assert!(argv
            .windows(2)
            .any(|w| w == ["--output-format", "stream-json"]));
    }

    #[test]
    fn cli_args_load_no_settings() {
        let argv = cli_args(None, None);
        let at = argv
            .iter()
            .position(|a| a == "--setting-sources")
            .expect("--setting-sources missing");

        // An empty value is the whole point: it is what keeps the operator's
        // hooks and CLAUDE.md out of the answer.
        assert_eq!(argv[at + 1], "");
    }

    #[test]
    fn cli_args_omit_model_and_system_when_unset() {
        let argv = cli_args(None, None);
        assert!(!argv.iter().any(|a| a == "--model"));
        assert!(!argv.iter().any(|a| a == "--system-prompt"));
    }

    #[test]
    fn cli_args_pass_model_and_system_through() {
        let argv = cli_args(Some("opus"), Some("be terse"));
        assert!(argv.windows(2).any(|w| w == ["--model", "opus"]));
        assert!(argv
            .windows(2)
            .any(|w| w == ["--system-prompt", "be terse"]));
    }

    #[test]
    fn cli_args_replace_rather_than_append_the_system_prompt() {
        let argv = cli_args(None, Some("be terse"));
        assert!(
            !argv.iter().any(|a| a == "--append-system-prompt"),
            "appending would leave Claude Code's agent prompt in place"
        );
    }

    #[test]
    fn a_lone_message_is_the_prompt() {
        let conversation = vec![message(ConversationRole::User, "why is the sky blue?")];
        assert_eq!(flatten_prompt(&conversation), "why is the sky blue?");
    }

    #[test]
    fn system_messages_never_reach_the_prompt() {
        let conversation = vec![
            message(ConversationRole::System, "be terse"),
            message(ConversationRole::User, "hello"),
        ];
        assert_eq!(flatten_prompt(&conversation), "hello");
    }

    #[test]
    fn history_becomes_a_transcript() {
        let conversation = vec![
            message(ConversationRole::User, "one"),
            message(ConversationRole::Assistant, "two"),
            message(ConversationRole::User, "three"),
        ];
        assert_eq!(
            flatten_prompt(&conversation),
            "Human: one\n\nAssistant: two\n\nHuman: three"
        );
    }

    #[test]
    fn an_empty_conversation_is_an_empty_prompt() {
        assert_eq!(flatten_prompt(&[]), "");
    }

    #[test]
    fn a_text_delta_yields_its_text() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Hey! 👋 How can"}},"session_id":"x","uuid":"y"}"#;
        assert_eq!(text_delta(line).as_deref(), Some("Hey! 👋 How can"));
    }

    #[test]
    fn a_thinking_delta_yields_nothing() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"hmm"}}}"#;
        assert_eq!(text_delta(line), None);
    }

    #[test]
    fn unrelated_lines_yield_nothing() {
        for line in [
            r#"{"type":"system","subtype":"init"}"#,
            r#"{"type":"assistant","message":{"content":[]}}"#,
            r#"{"type":"stream_event","event":{"type":"message_stop"}}"#,
            "not json at all",
            "",
        ] {
            assert_eq!(text_delta(line), None, "{line} was read as a delta");
        }
    }

    #[test]
    fn a_successful_result_is_not_an_error() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"result":"Hey there"}"#;
        assert_eq!(result_error(line), None);
    }

    #[test]
    fn a_failed_result_reports_the_servers_status() {
        let line = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"api_error_status":"rate limit exceeded","result":"whatever"}"#;
        assert_eq!(result_error(line).as_deref(), Some("rate limit exceeded"));
    }

    #[test]
    fn a_failed_result_falls_back_to_its_text() {
        let line = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"the model refused"}"#;
        assert_eq!(result_error(line).as_deref(), Some("the model refused"));
    }

    #[test]
    fn a_failed_result_falls_back_to_its_subtype() {
        let line = r#"{"type":"result","subtype":"error_max_turns","is_error":true}"#;
        assert_eq!(result_error(line).as_deref(), Some("error_max_turns"));
    }
}
