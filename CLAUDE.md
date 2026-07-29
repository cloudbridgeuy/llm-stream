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
- Simple, synchronous-style API that abstracts away complex async operations

### CLI (`crates/llm_stream/`)
- Terminal interface for the library
- Configuration management with TOML files in `~/.config/llm-stream/`
- Features: conversation history, templates, presets, external editor support
- Syntax highlighting using syntect with Tokyo Night themes
- Streaming output with spinners and colored terminal output
- ChatGPT subscription sign-in (`--login`, `--login-status`, `--logout`), stored at `<config_dir>/auth.json` with mode `0600`
- Reasoning controls for the `chatgpt` provider: `--reasoning-effort`, `--reasoning-summary`, `--models`

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