# llm-stream

[![Crates.io](https://img.shields.io/crates/v/llm-stream.svg)](https://crates.io/crates/llm-stream)
[![Docs.rs](https://docs.rs/llm-stream/badge.svg)](https://docs.rs/llm-stream/)

This library provides a streamlined approach to interacting with Large Language Model (LLM) streaming APIs from different providers.

## Supported Providers

- **OpenAI:** Access the powerful GPT models through OpenAI's API.
- **Anthropic:** Utilize Anthropic's Claude models for various language tasks.
- **Google:** Integrate Google's Gemini family of models.
- **Mistral:** Leverage Mistral's language models for advanced capabilities.
- **GitHub Copilot:** Access code-generation capabilities powered by GitHub Copilot.

## Key Features

- **Unified Interface:** Interact with different LLM providers using a consistent API.
- **Streaming Responses:** Receive model output as a stream of text, enabling real-time interactions and dynamic UI updates.
- **Simplified Authentication:** Easily authenticate with API keys for supported providers.
- **Customization Options:** Configure model parameters, such as temperature and top_p, to fine-tune output generation.

## Installation

Add the following dependency to your `Cargo.toml` file:

```toml
[dependencies]
llm-stream = "0.1.3"
```

## Usage

Here's a basic example demonstrating how to use the library to generate text with OpenAI's GPT-4 model:

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

For more in-depth examples and usage instructions, refer to the examples directory: [./lib/llm_stream/examples](./examples).

## 🔐 Authentication

Each provider requires an API key, typically set as an environment variable:

- OpenAI: `OPENAI_API_KEY`
- Google: `GOOGLE_API_KEY`
- Anthropic: `ANTHROPIC_API_KEY`
- Mistral: `MISTRAL_API_KEY`
