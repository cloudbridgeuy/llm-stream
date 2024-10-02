use eventsource_client::{Client as EsClient, ClientBuilder, ReconnectOptions, SSE};
use futures::stream::{Stream, TryStreamExt};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::error::Error;

// Completion API
const CHAT_API: &str = "/api/chat";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ChatCompletionChunk {
    /// The model name.
    pub model: String,
    /// The response message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
    /// Flag that indicates that the stream is finished.
    pub done: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct MessageBody {
    /// The model name.
    pub model: String,
    /// If `false` the response will be returned as a single response object, rather than a stream
    /// of objects.
    pub stream: bool,
    /// Additional model parameters listed in the documentation for the Modelfile such as
    /// `temperature`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<MessageBodyOptions>,
    /// The messages of the chat, this can be used to keep a chat memory.
    pub messages: Vec<Message>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct MessageBodyOptions {
    /// The temperature of the model. Increasing the temperature will make the model answer more
    /// creative.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Sets the stop sequences to use. When this pattern is encountered the LLM will stop
    /// generating text and return. Multiple stop patterns may be set by specifying multiple
    /// separate `stop` parameters in a `Modelfile`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    /// Reduces the probabiluty of generating nonsense. A higher value (e.g., 100) will give more
    /// diverse answers, while a lower value (e.g., 10) will be more conservative text. (Default
    /// 40)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Works together with `top-k`. A higher value (e.g. 0.95) will lead to more diverse text,
    /// while a lower value (e.g., 0.5) will generate more focused and conservative text. (Default
    /// 0.9)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
}

impl MessageBody {
    /// Creates a new `MessageBody`
    #[must_use]
    pub fn new(model: &str, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            stream: true,
            messages,
            ..Default::default()
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Client {
    pub api_url: String,
}

impl Client {
    #[must_use]
    pub fn new(api_url: impl Into<String>) -> Self {
        Self {
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

        let client = ClientBuilder::for_url(&(self.api_url.clone() + CHAT_API))?
            .header("content-type", "application/json")?
            .header("Accept", "application/x-ndjson")?
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
                SSE::Event(ev) => {
                    log::info!("{:#?}", ev);
                    match serde_json::from_str::<ChatCompletionChunk>(&ev.data) {
                        Ok(chunk) => {
                            if chunk.message.is_none() {
                                String::default()
                            } else {
                                chunk.message.unwrap().content.clone()
                            }
                        }
                        Err(_) => String::default(),
                    }
                }
                SSE::Comment(comment) => {
                    log::debug!("Comment: {:#?}", comment);
                    String::default()
                }
            });

        Ok(stream)
    }
}
