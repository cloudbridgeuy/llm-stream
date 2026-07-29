# llm-stream

[![Crate](https://img.shields.io/crates/v/llm-stream.svg)](https://crates.io/crates/llm-stream)
[![Docs](https://docs.rs/llm-stream/badge.svg)](https://docs.rs/llm-stream)
[![CLI](https://img.shields.io/crates/v/llm-stream-cli.svg)](https://crates.io/crates/llm-stream-cli)

llm-stream is a Rust library and CLI tool for streaming interactions with Large Language Models (LLMs).

## Features

- Streaming support for various LLM providers
- Sign in with a ChatGPT subscription — reach GPT models without a metered `OPENAI_API_KEY`
- Reasoning-model support: effort levels and optional reasoning summaries
- Easy-to-use API for integrating LLM capabilities into Rust applications
- Command-line interface for quick interactions with LLMs

## Installation

### Library

To use llm-stream in your Rust project, add the following to your `Cargo.toml`:

```toml
[dependencies]
llm-stream = "0.5.0"
```

### CLI Tool

To install the llm-stream CLI tool, run:

```bash
cargo install llm-stream-cli
```

## Usage

### Library

Here's a quick example of how to use the llm-stream library in your Rust code:

```rust
use anyhow::Result;
use futures::stream::TryStreamExt;
use llm_stream::openai::{Auth, Client, Message, MessageBody, Role};
use std::io::Write;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let key = std::env::var("OPENAI_API_KEY")?;

    let auth = Auth::new(key);
    let client = Client::new(auth, "https://api.openai.com/v1");

    let messages = vec![Message {
        role: Role::User,
        content: "What is the capital of the United States?".to_string(),
    }];

    let body = MessageBody::new("gpt-4o", messages);

    let mut stream = client.delta(&body)?;

    while let Ok(Some(text)) = stream.try_next().await {
        print!("{text}");
        std::io::stdout().flush()?;
    }

    Ok(())
}
```

For more detailed usage and API documentation, please refer to the [documentation on docs.rs](https://docs.rs/llm-stream).

### CLI Tool

To use the llm-stream CLI tool:

```bash
llm-stream "Tell me a joke" --api openai --model gpt-3.5-turbo --max-tokens 100
```

For more CLI options and usage information, run:

```bash
llm-stream --help
```

Stored conversation metadata can be changed locally, without contacting a
provider. Name the conversation with `--from <id>` or `--from-last`; both
metadata flags may be used together:

```bash
llm-stream --from <id> --set-title "A better title"
llm-stream --from <id> --set-description "What this conversation is about"
llm-stream --from <id> --set-title "Title" --set-description "Description"
```

These commands update only the requested top-level keys in the cache TOML, so
hand-written comments and unknown keys are preserved. Confirmation is printed
to stderr, and stdout remains quiet for scripting.

### ChatGPT subscription

`llm-stream` can reach GPT models through a ChatGPT subscription instead of a metered
`OPENAI_API_KEY`. Sign in once, in a browser:

```bash
llm-stream --login          # opens a browser; prints: signed in as you@example.com (plus)
llm-stream --login-status   # who is signed in, and how long the access token has left
llm-stream --logout         # deletes the stored credentials
```

Then talk to it with `--api chatgpt` (aliases: `chat-gpt`, `codex`):

```bash
llm-stream --api chatgpt "say PONG"
llm-stream --api chatgpt --model gpt-5.6-sol --reasoning-effort high "explain this diff"
llm-stream --api chatgpt --reasoning-summary "what is 17 * 23?"
```

| Flag | What it does |
| --- | --- |
| `--login` | Browser sign-in (OAuth 2.0 + PKCE). Stores credentials at `~/.config/llm-stream/auth.json`, mode `0600`. |
| `--login-status` | Prints the signed-in account, plan, and remaining access-token lifetime. Refreshes nothing. |
| `--logout` | Deletes the stored credentials. Succeeds whether or not any existed. |
| `--models` | Asks the server which models this account may use. Spends a little quota — see below. |
| `--reasoning-effort <low\|medium\|high\|xhigh>` | How hard the model should think. Also settable per preset, or as `reasoning_effort` in `config.toml`. |
| `--reasoning-summary` | Streams the model's reasoning summary to **stderr**, then a `---` rule, then the answer on stdout. The model decides whether to produce a summary at all. |

This endpoint ignores `--temperature`, `--top-p`, and `--top-k`; use `--reasoning-effort`
instead. It does not use `--api-key` or `--api-env`. Passing any of them prints a warning
to stderr and continues.

Answers always go to stdout and everything else to stderr, so `llm-stream --api chatgpt
"..." | cat` gives you the answer and nothing else.

#### Which models work

The server decides, per plan and per rollout, so `llm-stream` ships **no local allowlist**:
`--model` is passed through verbatim and the server's own refusal is what you see. Ask your
own account:

```bash
llm-stream --models
```

It probes one model at a time and prints a table. Each accepted probe opens a real request
and spends a small amount of your subscription's Codex allowance, so it warns you before it
starts.

Accepted on a Plus account on **2026-07-28**: `gpt-5.6-sol` (the default), `gpt-5.6-terra`,
`gpt-5.6-luna`, `gpt-5.5`, `gpt-5.4`. Refused on the same account: `gpt-5.3`, `gpt-5.1`,
`gpt-5-codex`. **That list will drift.** It is a snapshot of one account on one day, not a
contract — and a model missing from it is still worth trying with `--model`.

#### What this actually talks to

This provider posts to `https://chatgpt.com/backend-api/codex/responses`, an **undocumented
endpoint internal to OpenAI's Codex CLI**. It is not a public API and there is no
specification to appeal to. OpenAI may change, gate, or block it at any time without notice,
and using it from a client other than Codex is a gray area under OpenAI's terms of use.
Requests draw on your ChatGPT subscription's Codex allowance, not on a metered API key.

Use it knowing that.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgements

- Special thanks to the Rust community for their excellent tools and resources.
