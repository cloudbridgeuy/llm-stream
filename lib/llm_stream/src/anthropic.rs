use eventsource_client::{Client as EsClient, ClientBuilder, ReconnectOptions, SSE};
use futures::stream::{Stream, TryStreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

use crate::error::Error;

// Messages API
const MESSAGES_CREATE: &str = "/messages";

#[derive(Debug, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Content {
    /// Determines the content shape.
    pub r#type: String,
    /// Response content
    pub text: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Assistant,
    User,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct MessageBody {
    /// The model that will complete your prompt.
    /// See this link for additional details and options: https://docs.anthropic.com/claude/docs/models-overview
    pub model: String,
    /// Input messages.
    pub messages: Vec<Message>,
    /// The maximum number of tokens to generate before stopping.
    pub max_tokens: u32,
    /// An object describing metadata about the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
    /// Custom text sequences that will cause the model to stop generating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// Whether to incrementally stream the response using server-sent events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// System prompt
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Amount of randomness injected into the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Only sample from the top K options for each subsequent token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Use nucleus sampling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
}

impl MessageBody {
    /// Creates a new `MessageBody`
    #[must_use]
    pub fn new(model: &str, messages: Vec<Message>, max_tokens: u32) -> Self {
        Self {
            model: model.into(),
            messages,
            max_tokens,
            stream: Some(true),
            ..Default::default()
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageResponse {
    /// Unique object identifier.
    pub id: String,
    /// Object type.
    pub r#type: String,
    /// Conversational role of the generated message.
    pub role: String,
    /// Content generated by the model.
    pub content: Vec<Content>,
    /// The model that handled the request.
    pub model: String,
    /// The reason that the model stopped.
    pub stop_reason: Option<String>,
    /// Which custom stop sequence was generated, if any.
    pub stop_sequence: Option<String>,
    /// Billing and rate-limit usage.
    pub usage: Usage,
}

#[derive(Debug, Serialize, Deserialize)]
struct MessageEventResponse {
    /// Unique object identifier.
    pub id: String,
    /// Object type.
    pub r#type: String,
    /// Conversational role of the generated message.
    pub role: String,
    /// Content messages.
    pub content: Vec<Content>,
    /// The model that handled the request.
    pub model: String,
    /// The reason that the model stopped.
    pub stop_reason: Option<String>,
    /// Which custom stop sequence was generated, if any.
    pub stop_sequence: Option<String>,
    /// Billing and rate-limit usage.
    pub usage: Usage,
}

#[derive(Debug, Serialize, Deserialize)]
struct Delta {
    /// Determines the content shape.
    pub r#type: Option<String>,
    /// Response content
    pub text: Option<String>,
    pub stop_reason: Option<String>,
    pub end_turn: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum MessageEventType {
    #[default]
    Error,
    MessageStart,
    MessageDelta,
    MessageStop,
    Ping,
    ContentBlockStart,
    ContentBlockDelta,
    ContentBlockStop,
    Comment,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct MessageEvent {
    /// Event type
    pub r#type: MessageEventType,
    /// Init message
    pub message: Option<MessageEventResponse>,
    /// Event index
    pub index: Option<i32>,
    /// Content block
    pub content_block: Option<Content>,
    /// Delta block
    pub delta: Option<Delta>,
    /// Usage
    pub usage: Option<Usage>,
    /// Comment
    pub comment: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Auth {
    pub api_key: String,
    pub version: Option<String>,
}

impl Auth {
    #[must_use]
    pub fn new(api_key: String, version: Option<String>) -> Self {
        Self { api_key, version }
    }

    pub fn from_env() -> Result<Self, Error> {
        let api_key = match std::env::var("ANTHROPIC_API_KEY") {
            Ok(key) => key,
            Err(_) => return Err(Error::AuthError("ANTHROPIC_API_KEY not found".to_string())),
        };
        let version = std::env::var("ANTHROPIC_API_VERSION").ok();
        Ok(Self { api_key, version })
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

        let anthropic_version = self.auth.version.as_deref().unwrap_or("2023-06-01");

        let client = ClientBuilder::for_url(&(self.api_url.clone() + MESSAGES_CREATE))?
            .header("anthropic-version", anthropic_version)?
            .header("content-type", "application/json")?
            .header("x-api-key", &self.auth.api_key)?
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
                SSE::Event(ev) => match serde_json::from_str::<MessageEvent>(&ev.data) {
                    Ok(ev) => {
                        if matches!(ev.r#type, MessageEventType::ContentBlockDelta) {
                            if let Some(delta) = ev.delta {
                                delta.text.map_or_else(String::default, |text| text)
                            } else {
                                String::default()
                            }
                        } else {
                            String::default()
                        }
                    }
                    Err(e) => {
                        log::error!("Error parsing event: {:#?}", ev);
                        log::error!("Error: {:#?}", e);
                        String::default()
                    }
                },
                SSE::Comment(comment) => {
                    log::debug!("Comment: {:#?}", comment);
                    String::default()
                }
            });

        Ok(stream)
    }
}
