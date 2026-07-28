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
- Providers: OpenAI, Anthropic, Google, Mistral, Ollama, Groq, Jina, DeepSeek
- Simple, synchronous-style API that abstracts away complex async operations

### CLI (`crates/llm_stream/`)
- Terminal interface for the library
- Configuration management with TOML files in `~/.config/llm-stream/`
- Features: conversation history, templates, presets, external editor support
- Syntax highlighting using syntect with Tokyo Night themes
- Streaming output with spinners and colored terminal output
- ChatGPT subscription sign-in (`auth/` module): `--login`, `--login-status`, `--logout` — browser-based OAuth 2.0 + PKCE flow, credentials stored at `<config_dir>/auth.json` (mode `0600`)

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

Run tests with appropriate environment variables for API keys:
```bash
OPENAI_API_KEY=sk-... cargo test
```

Examples are available in `lib/llm_stream/examples/` for each provider.