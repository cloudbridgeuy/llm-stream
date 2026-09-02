# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Development Commands

### Build and Test
- `cargo build` - Build the entire workspace (library, CLI, and xtask)
- `cargo test` - Run all tests across the workspace
- `cargo run --bin llm-stream` - Run the CLI tool directly
- `cargo xtask build` - Custom build tasks via xtask

### Code Quality
- `cargo clippy --fix --all-targets -- -W clippy::pedantic -W clippy::nursery -W clippy::unwrap_used -W clippy::expect_used` - Lint with strict rules
- `cargo fmt` - Format code
- `bacon` - Continuous build/check/test (uses bacon.toml configuration)
- `bacon clippy` - Continuous linting
- `bacon test` - Continuous testing

### Documentation
- `cargo doc --no-deps` - Generate documentation
- `cargo doc --no-deps --open` - Generate and open documentation

## Architecture Overview

This is a Rust workspace with three main components:

### Library (`lib/llm_stream/`)
- Core streaming library for LLM interactions
- Provider-agnostic API that supports multiple LLM services
- Uses Server-Sent Events (SSE) via a custom eventsource-client fork
- Providers: OpenAI, Anthropic, Google, Mistral, Ollama, Groq, Jina, DeepSeek, and ChatGPT (subscription)
- The `claude` provider is CLI-only — it has no library half, because it drives a child process
  rather than an HTTP endpoint (see below)
- Simple, synchronous-style API that abstracts away complex async operations

### CLI (`crates/llm_stream/`)
- Terminal interface for the library
- Configuration management with TOML files in `~/.config/llm-stream/`
- Features: conversation history, templates, presets, external editor support
- Syntax highlighting using syntect with Tokyo Night themes
- Streaming output with spinners and colored terminal output
- ChatGPT subscription sign-in (`--login`, `--login-status`, `--logout`), stored at `<config_dir>/auth.json` with mode `0600`
- Reasoning controls for the `chatgpt` provider: `--reasoning-effort`, `--reasoning-summary`, `--models`
- A reasoning summary is written to stdout on a terminal and to stderr when stdout is piped, so
  `llm-stream --last | pbcopy` still copies the answer alone. It is never cached either way, and the
  server decides whether to produce one at all: measured live on 2026-07-30, `high` produced none
  where `xhigh` did.
- The summary arrives as numbered *parts*, one heading each, with no separator between them on the
  wire — `summary_index` changing is the only evidence of a seam. `chatgpt::SummaryJoin` inserts the
  blank line, without which nine headings printed as `**One****Two**…` on one row. On a terminal the
  summary is then rendered through `stream_render`, the same markdown renderer as the answer.

### Build Tools (`xtask/`)
- Custom build automation following the cargo-xtask pattern
- Commands: build, publish, github, install, changelog
- Integrated into cargo via alias in `.cargo/config`

## Key Implementation Details

### Provider Architecture
Each LLM provider has a dedicated module (e.g., `anthropic.rs`, `openai.rs`) that implements:
- Authentication handling
- Request/response structures
- Streaming delta processing
- Provider-specific error handling

### The ChatGPT Provider
Unlike every other provider, `--api chatgpt` authenticates with a ChatGPT subscription's
OAuth credentials rather than an API key, over an undocumented endpoint internal to
OpenAI's Codex CLI. Two rules are load-bearing and easy to break by accident:

- **Never send a `version` header** to that endpoint — it gates model access, and any value
  we send is compared against Codex's own release train. The guard comment lives at
  `lib/llm_stream/src/chatgpt.rs:337`.
- **Never print raw access or refresh tokens**, in logs, errors, or debug output.

There is deliberately no local model allowlist: `--model` passes through verbatim and the
server's own refusal reaches the operator. `--models` probes the server for the answer.

### The Claude Provider

`--api claude` is the only provider that reaches no network endpoint. It spawns the `claude`
binary (Claude Code) in print mode and reads the JSON Lines it writes to stdout, so it bills a
Claude subscription rather than an API key. `LLM_STREAM_CLAUDE_BIN` overrides the binary.

Three things about it are deliberate and easy to undo by accident:

- **It lives only in `crates/llm_stream/src/claude.rs`**, with no half in `lib/llm_stream`. A
  child process needs Tokio, and the library crate is deliberately free of that dependency.
- **`--setting-sources ''` is load-bearing.** Without it the child loads the operator's
  settings, hooks and `CLAUDE.md` before answering — measured 2026-08-10, a bare `claude -p`
  emitted 35 `system` events and a `SessionStart` hook payload ahead of the first token, versus
  5 events and no hooks with the flag. `--strict-mcp-config` and `--no-session-persistence`
  round that out.
- **`--system-prompt`, not `--append-system-prompt`.** Appending would leave Claude Code's own
  agent prompt in place, and the operator asked for a model, not a coding agent.

Text only: `thinking_delta` events are dropped, and the CLI exposes no sampling controls, so
`--temperature`, `--top-p`, `--top-k`, `--max-tokens` and `--min-tokens` are ignored with a
one-line warning on stderr rather than silently. `claude -p` takes a single prompt and cannot be
handed a prior assistant turn, so multi-turn history is flattened into a `Human:`/`Assistant:`
transcript; a lone user message passes through untouched.

The child exits 0 on a refusal or an API error and reports it in a `result` line instead, so
`result_error` reads that line — exit status alone would let a failed run look like an empty
answer.

### Configuration System
- TOML-based configuration in `~/.config/llm-stream/config.toml`
- Supports templates for prompt engineering
- Presets for common model configurations
- Conversation persistence and management

### Error Handling
- Custom error types using thiserror
- Provider-specific error mapping
- Graceful degradation for network issues

### Async Runtime
- Uses Tokio for async operations
- Futures-based streaming with TryStream
- Proper error propagation through the stream

## Testing

```bash
cargo test                              # aborts at the first failing target — see below
cargo test -p llm-stream --bin llm-stream   # CLI unit tests
cargo test -p llm_stream --lib              # library unit tests
cargo test -p llm-stream --test cli         # offline CLI integration tests
```

`prelude::tests::test_preset_system_over_config_system` is a known, pre-existing failure. It
makes a bare `cargo test` stop before reaching `crates/llm_stream/tests/`, so name the
target explicitly.

The `claude` provider is covered offline in `tests/cli.rs`: `LLM_STREAM_CLAUDE_BIN` points at a
stub script that replays a fixture of the JSON Lines the real binary emits, so the whole
spawn/parse/print path runs with no subscription and no network.

Some providers' unit tests read API keys from the environment:

```bash
OPENAI_API_KEY=sk-... cargo test
```

### The live ChatGPT smoke test

`crates/llm_stream/tests/live_chatgpt.rs` performs one real round trip against a ChatGPT
subscription. It skips unless opted in, because every run spends the operator's quota:

```bash
LLM_STREAM_LIVE_TEST=1 cargo test -p llm-stream --test live_chatgpt -- --nocapture
```

It requires a completed `llm-stream --login`. **Never make it unconditional and never wire
it into CI.**

Examples are available in `lib/llm_stream/examples/` for each provider.