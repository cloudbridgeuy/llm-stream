use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::str::FromStr;

use crate::prelude::*;

/// Custom parser function for JSON values
fn parse_json(s: &str) -> std::result::Result<Value, serde_json::Error> {
    serde_json::from_str(s)
}

/// Custom parser function to serialize conversations in JSON formats to the Conversation struct.
fn parse_conversation(s: &str) -> std::result::Result<Conversation, serde_json::Error> {
    let conversation: Conversation = serde_json::from_str(s)?;
    Ok(conversation)
}

#[derive(ValueEnum, Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Api {
    OpenAi,
    #[default]
    Anthropic,
    Google,
    Mistral,
    MistralFim,
}

// From string to API enum
impl FromStr for Api {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "OpenAi" => Ok(Api::OpenAi),
            "openai" => Ok(Api::OpenAi),
            "Anthropic" => Ok(Api::Anthropic),
            "anthropic" => Ok(Api::Anthropic),
            "google" => Ok(Api::Google),
            "Google" => Ok(Api::Google),
            "gemini" => Ok(Api::Google),
            "Gemini" => Ok(Api::Google),
            "mistral" => Ok(Api::Mistral),
            "Mistral" => Ok(Api::Mistral),
            "mistral-fim" => Ok(Api::MistralFim),
            "mistral_fim" => Ok(Api::MistralFim),
            "Mistral-FIM" => Ok(Api::MistralFim),
            "Mistral-Fim" => Ok(Api::MistralFim),
            "MistralFim" => Ok(Api::MistralFim),
            "Mistral_FIM" => Ok(Api::MistralFim),
            "Mistral_Fim" => Ok(Api::MistralFim),
            "MistralFIM" => Ok(Api::MistralFim),
            _ => Err(Error::InvalidAPI),
        }
    }
}

#[derive(Default, Clone, Debug, Parser, PartialEq, Serialize, Deserialize)]
#[command(name = "e", version = "0.1.0")]
#[command(about = "Interact with LLMs through the terminal")]
#[command(
    long_about = "This Rust-based CLI enables users to interact with various Large Language Models
(LLMs) directly from the terminal. Through this tool, you can send prompts to different
APIs, such as OpenAI, Anthropic, Google, Mistral, and Mistral FIM, and receive and handle
responses from these models.

The tool offers extensive configuration options, allowing you
to specify parameters like model type, maximum and minimum tokens, temperature, top-p
sampling, system messages, and more. You can set these options via command line arguments
or environment variables. Additionally, it supports preset configurations and prompt
templates, enabling more advanced and customizable usage scenarios.

The CLI can format and
highlight the model's responses using syntax highlighting, making it easier to read the
output in the terminal. It also includes functionality to handle streaming responses
efficiently, ensuring a smooth user experience when interacting with the LLMs."
)]
pub struct Args {
    /// The user message prompt. If `-` is provided, `stdin` will be read instead.
    #[serde(skip_serializing)]
    pub prompt: Option<String>,

    /// Additional file input to add to the prompt. If `-` is provided, `stdin` will be read
    /// instead.
    #[clap(hide = true)]
    #[serde(skip_serializing)]
    pub stdin: Option<String>,

    /// Suffix prompt
    #[clap(long)]
    #[serde(skip_serializing)]
    pub suffix: Option<String>,

    /// The API provider to use.
    #[clap(short, long, value_enum)]
    pub api: Option<Api>,

    /// The LLM Model to use
    #[clap(short, long)]
    pub model: Option<String>,

    /// The maximum amount of tokens to return.
    #[clap(long)]
    pub max_tokens: Option<u32>,

    /// The minimum amount of tokens to return.
    #[clap(long)]
    pub min_tokens: Option<u32>,

    /// The environment variable to use to get the access token for the api.
    #[clap(long)]
    pub api_env: Option<String>,

    /// The api version to use.
    #[clap(long)]
    pub api_version: Option<String>,

    /// The api key to use (will override the value of the environment variable.)
    #[clap(long)]
    pub api_key: Option<String>,

    /// The api base url.
    #[clap(long)]
    pub api_base_url: Option<String>,

    /// Don't run the spinner
    #[clap(long)]
    #[serde(skip_serializing)]
    pub quiet: Option<bool>,

    /// Language to use for syntax highlight
    #[clap(long, default_value = "markdown")]
    pub language: Option<String>,

    /// Add a system message to the request.
    #[clap(long)]
    #[serde(skip_serializing)]
    pub system: Option<String>,

    /// Temperature value.
    #[clap(long)]
    pub temperature: Option<f32>,

    /// Top-P value.
    #[clap(long)]
    pub top_p: Option<f32>,

    /// Top-K value.
    #[clap(long)]
    pub top_k: Option<u32>,

    /// Prompt template to use
    #[clap(short, long)]
    #[serde(skip_serializing)]
    pub template: Option<String>,

    /// Additional variables in JSON format
    #[clap(long, default_value="{}", value_parser = parse_json)]
    #[serde(skip_serializing)]
    pub vars: Option<Value>,

    /// Conversation to append to the model.
    #[clap(long, default_value="[]", value_parser = parse_conversation)]
    pub conversation: Conversation,

    /// Language to use for syntax highlight
    #[clap(long, default_value = "ansi")]
    #[serde(skip_serializing)]
    pub theme: Option<String>,

    /// Config dir where the configuration and conversation history will be stored.
    #[clap(long, default_value = "~/.config/llm-stream")]
    #[serde(skip_serializing)]
    pub config_dir: Option<String>,

    /// Config file. If undefined, it will be set as `config_dir/config.toml`.
    #[clap(long)]
    #[serde(skip_serializing)]
    pub config_file: Option<String>,

    /// Preset configuration
    #[clap(short, long)]
    #[serde(skip_serializing)]
    pub preset: Option<String>,

    /// Prints the configuration directories
    #[clap(long, default_value = "false")]
    #[serde(skip_serializing, default)]
    pub dir: bool,

    /// Prints the configuration file in use.
    #[clap(long, default_value = "false")]
    #[serde(skip_serializing, default)]
    pub config: bool,

    /// Prints the conversation to be sent to the LLM.
    #[clap(long, default_value = "false")]
    #[serde(skip_serializing, default)]
    pub print_conversation: bool,

    /// Don't call the LLM.
    #[clap(long, default_value = "false")]
    #[serde(skip_serializing, default)]
    pub dry_run: bool,

    /// Don't cache the conversation details.
    #[clap(long, default_value = "false")]
    #[serde(skip_serializing, default)]
    pub no_cache: bool,

    /// Continue the conversation identified by its id.
    #[clap(long)]
    #[serde(skip_serializing)]
    pub from: Option<String>,

    /// Continue from the last conversation
    #[clap(long)]
    #[serde(skip_serializing, default)]
    pub from_last: bool,

    /// Fork the conversation into a new one when using --from or --from-last options.
    #[clap(long)]
    #[serde(skip_serializing, default)]
    pub fork: bool,

    /// Conversation parent.
    #[clap(hide = true)]
    pub parent: Option<String>,

    /// Conversation description.
    #[clap(long)]
    pub description: Option<String>,

    /// Conversation title.
    #[clap(long)]
    pub title: Option<String>,

    /// Print the conversation defined in --from or --from-last to stdout
    #[clap(long)]
    #[serde(skip_serializing, default)]
    pub show: bool,

    /// Print the list of existing conversations.
    #[clap(long)]
    #[serde(skip_serializing, default)]
    pub list: bool,

    /// Don't use colors to print the output.
    #[clap(long)]
    #[serde(skip_serializing, default)]
    pub no_color: bool,
}
