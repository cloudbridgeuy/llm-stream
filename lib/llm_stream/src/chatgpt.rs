//! OpenAI Responses API over a ChatGPT subscription.
//!
//! This is **not** the Chat Completions API used by [`crate::openai`]. The
//! system prompt travels in a top-level `instructions` field, history travels
//! as `input` items, and text arrives as `response.output_text.delta` events.
//!
//! The endpoint is private to OpenAI's Codex client and undocumented. Every
//! constant here came from live measurement, not from a specification.

use eventsource_client::{Client as EsClient, ClientBuilder, ReconnectOptions, SSE};
use futures::stream::{Stream, TryStreamExt};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::error::Error;
use crate::event::ReasonEvent;

/// Base URL of the private Codex backend a ChatGPT subscription can reach.
pub const DEFAULT_URL: &str = "https://chatgpt.com/backend-api/codex";

/// Appended to the client's base URL to reach the Responses endpoint.
const RESPONSES_API: &str = "/responses";

/// How hard the model should think. The Responses endpoint discards
/// `temperature` and `top_p`; this is the only sampling control it honours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
    XHigh,
}

/// How verbose a reasoning summary should be, when one is requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Summary {
    Auto,
    Concise,
    Detailed,
}

/// Who produced an input item. There is no `System` variant: the Responses API
/// carries the system prompt in a top-level `instructions` field, so a system
/// message in the input list is not a thing that can exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// One piece of a message's content.
///
/// The wire type is derived from the role rather than being a settable field.
/// The API rejects `input_text` on an assistant turn and `output_text` on a
/// user turn, so letting a caller pick would make a rejected request
/// representable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentPart {
    kind: &'static str,
    pub text: String,
}

impl ContentPart {
    /// A part of something the operator said.
    #[must_use]
    pub fn input_text(text: impl Into<String>) -> Self {
        Self {
            kind: "input_text",
            text: text.into(),
        }
    }

    /// A part of something the model previously said.
    #[must_use]
    pub fn output_text(text: impl Into<String>) -> Self {
        Self {
            kind: "output_text",
            text: text.into(),
        }
    }
}

impl Serialize for ContentPart {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ContentPart", 2)?;
        state.serialize_field("type", self.kind)?;
        state.serialize_field("text", &self.text)?;
        state.end()
    }
}

/// One turn of conversation history, in the shape `input` expects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputItem {
    pub role: Role,
    pub content: Vec<ContentPart>,
}

impl InputItem {
    #[must_use]
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentPart::input_text(text)],
        }
    }

    #[must_use]
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentPart::output_text(text)],
        }
    }

    /// The text this item carries, joined across its parts.
    #[must_use]
    pub fn text(&self) -> String {
        self.content
            .iter()
            .map(|part| part.text.as_str())
            .collect::<Vec<_>>()
            .concat()
    }
}

impl Serialize for InputItem {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("InputItem", 3)?;
        state.serialize_field("type", "message")?;
        state.serialize_field("role", &self.role)?;
        state.serialize_field("content", &self.content)?;
        state.end()
    }
}

/// Reasoning controls. `summary` is absent unless the caller asked for one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reasoning {
    pub effort: Effort,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<Summary>,
}

/// A request to the Responses endpoint.
///
/// `stream`, `store`, `tools`, and `include` are private and set once by
/// [`MessageBody::new`]. They are not caller-settable on purpose: this client
/// can only read a stream, and flipping `store` would silently change the
/// data-retention posture of every request the tool makes. With no `Default`
/// impl and no public constructor besides `new`, no other combination can be
/// built from outside this module.
#[derive(Debug, Serialize)]
pub struct MessageBody {
    pub model: String,

    /// The system prompt. The Responses API carries it here, not in `input`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,

    pub input: Vec<InputItem>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Reasoning>,

    /// A stable key that lets the server reuse a cached prefix across turns of
    /// the same conversation. Omitted when there is no conversation to key on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,

    /// Always `true`.
    stream: bool,
    /// Always `false`.
    store: bool,
    /// Always empty: `llm-stream` performs no function calling.
    tools: Vec<serde_json::Value>,
    /// Always empty: no encrypted reasoning items are replayed.
    include: Vec<serde_json::Value>,
}

impl MessageBody {
    #[must_use]
    pub fn new(model: &str, input: Vec<InputItem>) -> Self {
        Self {
            model: model.to_owned(),
            instructions: None,
            input,
            reasoning: None,
            prompt_cache_key: None,
            stream: true,
            store: false,
            tools: Vec::new(),
            include: Vec::new(),
        }
    }
}

/// One event off the Responses SSE stream, as one closed set of cases.
///
/// The `Unknown` arm is load-bearing. A live stream carries more than ten event
/// types and OpenAI adds more without notice, so an unrecognised event must be
/// inert rather than fatal.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum ResponseEvent {
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta { delta: String },
    #[serde(rename = "response.reasoning_summary_text.delta")]
    ReasoningDelta { delta: String },
    #[serde(rename = "response.completed")]
    Completed,
    #[serde(other)]
    Unknown,
}

/// Reads one SSE `data:` payload.
///
/// Anything we do not recognise — a new event type, a truncated line, an empty
/// payload — becomes `Unknown`. Returning a `Result` here would push the
/// decision to every caller, and there is only one correct decision.
#[must_use]
pub fn classify(data: &str) -> ResponseEvent {
    serde_json::from_str(data).unwrap_or(ResponseEvent::Unknown)
}

/// Projects a Responses event onto the provider-neutral event the CLI handles.
#[must_use]
pub fn to_reason_event(event: ResponseEvent) -> ReasonEvent {
    match event {
        ResponseEvent::OutputTextDelta { delta } => ReasonEvent::Delta(delta),
        ResponseEvent::ReasoningDelta { delta } => ReasonEvent::Reasoning(delta),
        ResponseEvent::Completed | ResponseEvent::Unknown => ReasonEvent::Empty,
    }
}

/// Extracts the human-readable message from an error body.
///
/// The Codex endpoint answers a rejected request with
/// `{"detail":"The 'gpt-4o' model is not supported when using Codex with a
/// ChatGPT account."}`. Other OpenAI surfaces use `{"error":{"message":…}}`,
/// and some use a bare `{"message":…}`, so all three are read here — the point
/// of this function is that the operator sees a sentence, not a JSON blob.
///
/// Returns `None` when the body is not JSON or carries none of those keys; the
/// caller falls back to showing the body verbatim.
#[must_use]
pub fn detail_of(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;

    let message = value
        .get("detail")
        .or_else(|| value.get("error").and_then(|error| error.get("message")))
        .or_else(|| value.get("message"))?;

    message.as_str().map(str::to_owned)
}

/// Subscription credentials for the Codex backend.
#[derive(Debug, Clone)]
pub struct Auth {
    pub access_token: String,
    /// Sent as `chatgpt-account-id` when present. Absence is normal.
    pub account_id: Option<String>,
}

impl Auth {
    #[must_use]
    pub const fn new(access_token: String, account_id: Option<String>) -> Self {
        Self {
            access_token,
            account_id,
        }
    }
}

/// Turns a transport error into ours, reading the response body when the server
/// rejected the request outright.
///
/// This is the impure half: it awaits the body off the wire. All of the
/// deciding lives in [`detail_of`], which is pure and tested.
async fn map_stream_error(error: eventsource_client::Error) -> Error {
    let eventsource_client::Error::UnexpectedResponse(response, body) = error else {
        return Error::from(error);
    };

    let status = response.status();

    match body.body_bytes().await {
        Ok(bytes) => {
            let raw = String::from_utf8_lossy(&bytes).into_owned();
            detail_of(&raw).map_or_else(
                || {
                    if raw.trim().is_empty() {
                        Error::ApiError(format!("HTTP {status}"))
                    } else {
                        // No sentence we recognise — better the raw body than
                        // nothing, since the operator has to act on it.
                        Error::ApiError(format!("HTTP {status}: {raw}"))
                    }
                },
                Error::ApiError,
            )
        }
        Err(read_error) => Error::ApiError(format!(
            "HTTP {status} (response body could not be read: {read_error})"
        )),
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

    /// Builds the SSE request. Split out so the streaming methods stay free of
    /// header plumbing.
    fn request(&self, message_body: &MessageBody) -> Result<impl EsClient, Error> {
        let request_body = serde_json::to_value(message_body)?;
        log::debug!("request_body: {request_body:#?}");

        let authorization = format!("Bearer {}", self.auth.access_token);

        // NEVER add a `version` header here. The server compares it against
        // Codex's release train and rejects current models with "requires a
        // newer version of Codex". Omitting it skips the check entirely.
        // `originator` is unchecked and free-form, so we identify honestly.
        let mut builder = ClientBuilder::for_url(&(self.api_url.clone() + RESPONSES_API))?
            .header("content-type", "application/json")?
            .header("accept", "text/event-stream")?
            .header("authorization", &authorization)?
            .header("openai-beta", "responses=experimental")?
            .header("originator", "llm_stream")?;

        if let Some(account_id) = self.auth.account_id.as_deref() {
            builder = builder.header("chatgpt-account-id", account_id)?;
        }

        Ok(builder
            .method("POST".to_string())
            .body(request_body.to_string())
            .reconnect(
                ReconnectOptions::reconnect(true)
                    .retry_initial(false)
                    .delay(Duration::from_secs(1))
                    .backoff_factor(2)
                    .delay_max(Duration::from_secs(60))
                    .build(),
            )
            .build())
    }

    /// Streams just the answer text. Every other event type collapses to an
    /// empty string, which the CLI's stream handler prints as nothing.
    pub fn delta<'a>(
        &'a self,
        message_body: &'a MessageBody,
    ) -> Result<impl Stream<Item = Result<String, Error>> + 'a, Error> {
        log::debug!("message_body: {message_body:#?}");

        let client = self.request(message_body)?;

        let stream = Box::pin(client.stream())
            .or_else(|error| async move { Err(map_stream_error(error).await) })
            .map_ok(|event| match event {
                SSE::Event(ev) => match classify(&ev.data) {
                    ResponseEvent::OutputTextDelta { delta } => delta,
                    _ => String::new(),
                },
                SSE::Connected(_) => String::new(),
                SSE::Comment(comment) => {
                    log::debug!("comment: {comment}");
                    String::new()
                }
            });

        // Boxed so the result is `Unpin` — see the note on `or_else` above.
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json_of<T: Serialize>(value: &T) -> serde_json::Value {
        serde_json::to_value(value).unwrap_or_else(|e| panic!("serialization failed: {e}"))
    }

    #[test]
    fn body_always_streams_and_never_stores() {
        let v = json_of(&MessageBody::new("gpt-5.6-sol", vec![]));
        assert_eq!(v["stream"], serde_json::json!(true));
        assert_eq!(v["store"], serde_json::json!(false));
        assert_eq!(v["tools"], serde_json::json!([]));
        assert_eq!(v["include"], serde_json::json!([]));
        assert_eq!(v["model"], serde_json::json!("gpt-5.6-sol"));
    }

    #[test]
    fn optional_fields_are_omitted_when_unset() {
        let v = json_of(&MessageBody::new("m", vec![]));
        assert!(v.get("instructions").is_none());
        assert!(v.get("reasoning").is_none());
        assert!(v.get("prompt_cache_key").is_none());
    }

    #[test]
    fn xhigh_effort_serializes_without_a_separator() {
        let v = json_of(&Reasoning {
            effort: Effort::XHigh,
            summary: None,
        });
        // Not "x_high", not "xHigh" — the server only accepts "xhigh".
        assert_eq!(v["effort"], serde_json::json!("xhigh"));
        assert!(v.get("summary").is_none());
    }

    #[test]
    fn user_items_carry_input_text_parts() {
        let v = json_of(&InputItem::user("hello"));
        assert_eq!(v["type"], serde_json::json!("message"));
        assert_eq!(v["role"], serde_json::json!("user"));
        assert_eq!(v["content"][0]["type"], serde_json::json!("input_text"));
        assert_eq!(v["content"][0]["text"], serde_json::json!("hello"));
    }

    #[test]
    fn assistant_items_carry_output_text_parts() {
        // The API rejects `input_text` on an assistant turn. If this ever gets
        // "simplified" to one shared constructor, multi-turn requests break.
        let v = json_of(&InputItem::assistant("hi there"));
        assert_eq!(v["role"], serde_json::json!("assistant"));
        assert_eq!(v["content"][0]["type"], serde_json::json!("output_text"));
    }

    #[test]
    fn item_text_joins_its_parts() {
        let item = InputItem {
            role: Role::User,
            content: vec![ContentPart::input_text("a"), ContentPart::input_text("b")],
        };
        assert_eq!(item.text(), "ab");
    }

    #[test]
    fn classifies_output_text_delta() {
        let ev = classify(include_str!("../tests/fixtures/output_text_delta.json"));
        assert_eq!(
            ev,
            ResponseEvent::OutputTextDelta {
                delta: "ONG".to_string()
            }
        );
    }

    #[test]
    fn classifies_reasoning_summary_delta() {
        let ev = classify(include_str!(
            "../tests/fixtures/reasoning_summary_delta.json"
        ));
        assert!(
            matches!(ev, ResponseEvent::ReasoningDelta { ref delta } if delta.contains("17 times 23")),
            "got {ev:?}"
        );
    }

    #[test]
    fn classifies_completion_despite_extra_fields() {
        // A unit variant on an internally tagged enum must ignore `response`.
        let ev = classify(include_str!("../tests/fixtures/response_completed.json"));
        assert_eq!(ev, ResponseEvent::Completed);
    }

    #[test]
    fn unknown_event_types_are_inert() {
        assert_eq!(
            classify(include_str!("../tests/fixtures/unknown_event.json")),
            ResponseEvent::Unknown
        );
        assert_eq!(
            classify(include_str!("../tests/fixtures/output_item_reasoning.json")),
            ResponseEvent::Unknown
        );
    }

    #[test]
    fn malformed_payloads_are_unknown_not_a_panic() {
        assert_eq!(classify("{not json"), ResponseEvent::Unknown);
        assert_eq!(classify(""), ResponseEvent::Unknown);
        assert_eq!(classify("[DONE]"), ResponseEvent::Unknown);
    }

    #[test]
    fn to_reason_event_maps_deltas_and_swallows_the_rest() {
        assert!(matches!(
            to_reason_event(ResponseEvent::OutputTextDelta { delta: "x".to_string() }),
            ReasonEvent::Delta(ref s) if s == "x"
        ));
        assert!(matches!(
            to_reason_event(ResponseEvent::ReasoningDelta { delta: "r".to_string() }),
            ReasonEvent::Reasoning(ref s) if s == "r"
        ));
        assert!(matches!(
            to_reason_event(ResponseEvent::Completed),
            ReasonEvent::Empty
        ));
        assert!(matches!(
            to_reason_event(ResponseEvent::Unknown),
            ReasonEvent::Empty
        ));
    }

    #[test]
    fn detail_of_extracts_the_codex_rejection_sentence() {
        let body = r#"{"detail":"The 'gpt-4o' model is not supported when using Codex with a ChatGPT account."}"#;
        assert_eq!(
            detail_of(body).as_deref(),
            Some("The 'gpt-4o' model is not supported when using Codex with a ChatGPT account.")
        );
    }

    #[test]
    fn detail_of_reads_the_nested_openai_error_shape() {
        let body = r#"{"error":{"message":"Invalid token","type":"invalid_request_error"}}"#;
        assert_eq!(detail_of(body).as_deref(), Some("Invalid token"));
    }

    #[test]
    fn detail_of_reads_a_bare_message() {
        assert_eq!(detail_of(r#"{"message":"nope"}"#).as_deref(), Some("nope"));
    }

    #[test]
    fn detail_of_returns_none_when_there_is_no_sentence_to_show() {
        assert!(detail_of("not json").is_none());
        assert!(detail_of("").is_none());
        assert!(detail_of(r#"{"code":429}"#).is_none());
        // A non-string `detail` is not a sentence.
        assert!(detail_of(r#"{"detail":{"nested":"thing"}}"#).is_none());
    }
}
