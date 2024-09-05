use eventsource_client::{Client as EsClient, ClientBuilder, ReconnectOptions, SSE};
use futures::stream::{Stream, TryStreamExt};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::error::Error;

// Chat Completion API
const CHAT_API: &str = "/chat/completions";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    Assistant,
    User,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct MessageBody {
    /// ID of the model to use. You can use the [List Available Models API](https://docs.mistral.ai/api/#tag/models/operation/list_models_v1_models_get) to see all of your available models, or see our [Model overview](https://docs.mistral.ai/models) for model descriptions.
    pub model: String,
    /// What sampling temperature to use, between 0.0 and 1.0. Higher values like 0.8 will make the output more random, while lower values like 0.2 will make it more focused and deterministic. We generally recommend altering this or top_p but not both.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Nucleus sampling, where the model considers the results of the tokens with top_p probability mass. So 0.1 means only the tokens comprising the top 10% probability mass are considered. We generally recommend altering this or temperature but not both.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    /// The maximum number of tokens to generate in the completion. The token count of your prompt plus max_tokens cannot exceed the model's context length.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// The minimum number of tokens to generate in the completion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_tokens: Option<u32>,
    /// Stop generation if this token is detected. Or if one of these tokens is detected when providing an array
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    /// The prompt(s) to generate completions for, encoded as a list of dict with role and content.
    pub messages: Vec<Message>,
    /// Whether to stream back partial progress. If set, tokens will be sent as data-only server-side events as they become available, with the stream terminated by a data: [DONE] message. Otherwise, the server will hold the request open until the timeout or until completion, with the response containing the full result as JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// The seed to use for random sampling. If set, different calls will generate deterministic results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub random_seed: Option<u32>,
}

impl MessageBody {
    /// Creates a new `MessageBody`
    #[must_use]
    pub fn new(model: &str, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            stream: Some(true),
            ..Default::default()
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Choice {
    pub index: u32,
    pub delta: Delta,
    pub finish_reason: Option<String>,
    pub logprobs: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Delta {
    pub role: Option<String>,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub total_tokens: u32,
    pub completion_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Auth {
    pub api_key: String,
}

impl Auth {
    #[must_use]
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    pub fn from_env() -> Result<Self, Error> {
        let api_key = match std::env::var("MISTRAL_API_KEY") {
            Ok(key) => key,
            Err(_) => return Err(Error::AuthError("MISTRAL_API_KEY not found".to_string())),
        };
        Ok(Self { api_key })
    }
}

#[derive(Debug, Clone)]
pub struct Client {
    pub auth: Auth,
    pub api_url: String,
}

impl Client {
    pub fn new(auth: Auth, api_url: impl Into<String>) -> Self {
        Self {
            auth,
            api_url: api_url.into(),
        }
    }
}

impl Client {
    pub fn delta<'a>(
        &'a self,
        message_body: &'a MessageBody,
    ) -> Result<impl Stream<Item = Result<String, Error>> + 'a, Error> {
        log::debug!("message_body: {:#?}", message_body);

        let request_body = match serde_json::to_value(message_body) {
            Ok(body) => body,
            Err(e) => return Err(Error::Serde(e)),
        };
        log::debug!("request_body: {:#?}", request_body);

        let authorization: &str = &format!("Bearer {}", self.auth.api_key);

        let client = ClientBuilder::for_url(&(self.api_url.clone() + CHAT_API))?
            .header("content-type", "application/json")?
            .header("authorization", authorization)?
            .method("POST".into())
            .body(request_body.to_string())
            .reconnect(
                ReconnectOptions::reconnect(true)
                    .retry_initial(false)
                    .delay(Duration::from_secs(1))
                    .backoff_factor(2)
                    .delay_max(Duration::from_secs(60))
                    .build(),
            )
            .build();

        let stream = Box::pin(client.stream())
            .map_err(Error::from)
            .map_ok(|event| match event {
                SSE::Connected(_) => String::default(),
                SSE::Event(ev) => match serde_json::from_str::<ChatCompletionChunk>(&ev.data) {
                    Ok(chunk) => {
                        if chunk.choices.is_empty() {
                            String::default()
                        } else {
                            chunk.choices.first().unwrap().delta.content.clone()
                        }
                    }
                    Err(_) => String::default(),
                },
                SSE::Comment(comment) => {
                    log::debug!("Comment: {:#?}", comment);
                    String::default()
                }
            });

        Ok(stream)
    }
}
