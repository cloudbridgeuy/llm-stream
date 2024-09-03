# llm-stream

[![Crate](https://img.shields.io/crates/v/llm-stream.svg)](https://crates.io/crates/llm-stream)
[![Docs](https://docs.rs/llm-stream/badge.svg)](https://docs.rs/llm-stream)
[![CLI](https://img.shields.io/crates/v/llm-stream-cli.svg)](https://crates.io/crates/llm-stream-cli)

llm-stream is a Rust library and CLI tool for streaming interactions with Large Language Models (LLMs).

## Features

- Streaming support for various LLM providers
- Easy-to-use API for integrating LLM capabilities into Rust applications
- Command-line interface for quick interactions with LLMs

## Installation

### Library

To use llm-stream in your Rust project, add the following to your `Cargo.toml`:

```toml
[dependencies]
llm-stream = "0.1.0"
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

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgements

- Thanks to the [`bat`](https://github.com/sharkdp/bat) and [`tera`](https://github.com/Keats/tera) crates for being awesome.
- Special thanks to the Rust community for their excellent tools and resources.
