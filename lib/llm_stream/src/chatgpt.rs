//! OpenAI Responses API over a ChatGPT subscription.
//!
//! This is **not** the Chat Completions API used by [`crate::openai`]. The
//! system prompt travels in a top-level `instructions` field, history travels
//! as `input` items, and text arrives as `response.output_text.delta` events.
//!
//! The endpoint is private to OpenAI's Codex client and undocumented. Every
//! constant here came from live measurement, not from a specification.

use eventsource_client::{Client as EsClient, ClientBuilder, ReconnectOptions, SSE};
use futures::stream::{Stream, StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::error::{map_stream_error, Error};
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

/// The error payload carried by a failure event.
///
/// Only `message` is modelled, because only `message` is shown to the operator.
/// The rest of the object varies by failure kind and reading it would be
/// guessing at an undocumented shape.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct EventError {
    pub message: String,
}

/// The `response` object on a `response.failed` event.
///
/// `error` is `Option` because the server does not always populate it — and a
/// failure with no explanation is still a failure, so the absence must not be
/// mistaken for success.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct FailedResponse {
    #[serde(default)]
    pub error: Option<EventError>,
}

/// One event off the Responses SSE stream, as one closed set of cases.
///
/// The `Unknown` arm is load-bearing. A live stream carries more than ten event
/// types and OpenAI adds more without notice, so an unrecognised event must be
/// inert rather than fatal.
///
/// `StreamError` and `ResponseFailed` are the two exceptions: they are the only
/// events that mean the request failed, and they must never be inert. See
/// [`failure_message`].
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum ResponseEvent {
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta { delta: String },
    #[serde(rename = "response.reasoning_summary_text.delta")]
    ReasoningDelta {
        delta: String,
        /// Which summary part this delta belongs to. The server numbers the
        /// parts and sends no separator between them, so this is the only
        /// evidence that one part ended and the next began. Defaulted rather
        /// than required: a delta that arrives without it is still a delta.
        #[serde(default)]
        summary_index: u32,
    },
    #[serde(rename = "response.completed")]
    Completed,
    /// A failure reported mid-stream, on an otherwise successful HTTP 200.
    #[serde(rename = "error")]
    StreamError { error: EventError },
    /// The terminal form of the same thing: the response ended as `failed`.
    #[serde(rename = "response.failed")]
    ResponseFailed { response: FailedResponse },
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

/// What the operator is told when the server ends a response as `failed` but
/// sends no explanation with it.
///
/// Silence would otherwise be indistinguishable from a successful empty answer,
/// which is the one thing a failure must never look like.
pub const UNSPECIFIED_FAILURE: &str = "the model stopped without producing an answer";

/// The sentence to show the operator, when an event means the request failed.
///
/// `None` for every ordinary event. `Some` for the two the Codex backend uses to
/// report a mid-stream failure — both of which arrive **inside a successful HTTP
/// 200 stream**, so no layer above this one can tell that anything went wrong.
///
/// Measured live on 2026-07-29: an overloaded backend sends `error`, then
/// `response.failed`, then closes the stream cleanly. Treating either as inert
/// makes the CLI exit 0 having printed nothing.
#[must_use]
pub fn failure_message(event: &ResponseEvent) -> Option<String> {
    match event {
        ResponseEvent::StreamError { error } => Some(error.message.clone()),
        ResponseEvent::ResponseFailed { response } => Some(
            response
                .error
                .as_ref()
                .map_or_else(|| UNSPECIFIED_FAILURE.to_string(), |e| e.message.clone()),
        ),
        _ => None,
    }
}

/// Projects a Responses event onto the provider-neutral event the CLI handles.
#[must_use]
pub fn to_reason_event(event: ResponseEvent) -> ReasonEvent {
    if let Some(message) = failure_message(&event) {
        return ReasonEvent::Err(Error::ApiError(message));
    }

    match event {
        ResponseEvent::OutputTextDelta { delta } => ReasonEvent::Delta(delta),
        ResponseEvent::ReasoningDelta { delta, .. } => ReasonEvent::Reasoning(delta),
        ResponseEvent::Completed | ResponseEvent::Unknown => ReasonEvent::Empty,
        // Unreachable: `failure_message` returned `Some` for these above. Listed
        // rather than folded into a `_` arm so that a new variant is a compile
        // error here instead of a silent `Empty`.
        ResponseEvent::StreamError { .. } | ResponseEvent::ResponseFailed { .. } => {
            ReasonEvent::Empty
        }
    }
}

/// What goes between two reasoning summary parts.
///
/// The server streams parts, not prose. Each `summary_index` is its own block,
/// none of them ends in a newline, and nothing on the wire marks the seam — so
/// writing the deltas out in arrival order ran the parts together, which is how
/// a summary of nine headings reached the terminal as
/// `**One****Two****Three**` on a single row.
pub const PART_BREAK: &str = "\n\n";

/// Remembers which summary part the stream is in.
///
/// The only state [`Client::reason`] keeps, and it exists because the seam
/// between two parts is implied by `summary_index` changing rather than
/// announced: no function that sees one event at a time can find it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SummaryJoin {
    seen: Option<u32>,
}

impl SummaryJoin {
    /// Projects one event the way [`to_reason_event`] does, and inserts the
    /// paragraph break the server implies but does not send.
    ///
    /// The break precedes the first delta of every part *after* the first, so a
    /// summary never opens with blank rows.
    pub fn project(&mut self, event: ResponseEvent) -> ReasonEvent {
        if let ResponseEvent::ReasoningDelta {
            delta,
            summary_index,
        } = event
        {
            let opens_a_part = self.seen.is_some_and(|seen| seen != summary_index);
            self.seen = Some(summary_index);
            return ReasonEvent::Reasoning(if opens_a_part {
                format!("{PART_BREAK}{delta}")
            } else {
                delta
            });
        }

        to_reason_event(event)
    }
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
            .map(|result| {
                result.and_then(|event| match event {
                    SSE::Event(ev) => {
                        let event = classify(&ev.data);
                        // Before the text projection: a failure event carries no
                        // delta, so flattening it to `String::new()` would end
                        // the stream silently with a zero exit code.
                        if let Some(message) = failure_message(&event) {
                            return Err(Error::ApiError(message));
                        }
                        Ok(match event {
                            ResponseEvent::OutputTextDelta { delta } => delta,
                            _ => String::new(),
                        })
                    }
                    SSE::Connected(_) => Ok(String::new()),
                    SSE::Comment(comment) => {
                        log::debug!("comment: {comment}");
                        Ok(String::new())
                    }
                })
            });

        // Boxed so the result is `Unpin` — see the note on `or_else` above.
        Ok(Box::pin(stream))
    }

    /// Streams the answer *and* the reasoning summary as separate events.
    ///
    /// Same connection, same headers, same error mapping as [`Client::delta`] —
    /// the only difference is that events are projected through the pure
    /// [`to_reason_event`] instead of being flattened to text, so the caller can
    /// tell a summary token from an answer token.
    ///
    /// A stream may carry **zero** reasoning events even when the request asked
    /// for a summary: the model decides. Callers must not assume otherwise.
    pub fn reason<'a>(
        &'a self,
        message_body: &'a MessageBody,
    ) -> Result<impl Stream<Item = Result<ReasonEvent, Error>> + 'a, Error> {
        log::debug!("message_body: {message_body:#?}");

        let client = self.request(message_body)?;

        // Carries the summary part boundary across events. See `SummaryJoin`.
        let mut join = SummaryJoin::default();

        let stream = Box::pin(client.stream())
            .or_else(|error| async move { Err(map_stream_error(error).await) })
            .map(move |result| {
                result.and_then(|event| match event {
                    SSE::Event(ev) => {
                        // The endpoint is undocumented and adds event types
                        // without notice, so the raw payload is the only way to
                        // tell "the server sent nothing" from "we classified it
                        // as `Unknown`". Carries no credentials: the tokens
                        // travel in the request headers, never in an SSE body.
                        log::debug!("event: {}", ev.data);

                        let event = classify(&ev.data);
                        // Raised to the `Result` rather than left as
                        // `ReasonEvent::Err`: the CLI's reasoning handler has no
                        // arm for that variant, so it would be dropped.
                        if let Some(message) = failure_message(&event) {
                            return Err(Error::ApiError(message));
                        }
                        Ok(join.project(event))
                    }
                    SSE::Connected(_) => Ok(ReasonEvent::Connected),
                    SSE::Comment(comment) => Ok(ReasonEvent::Comment(comment)),
                })
            });

        // Boxed so the result is `Unpin` — `or_else` with an async block is not,
        // and the CLI's stream handlers require it.
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json_of<T: Serialize>(value: &T) -> serde_json::Value {
        serde_json::to_value(value).unwrap_or_else(|e| panic!("serialization failed: {e}"))
    }

    /// The sentence an overloaded Codex backend sends.
    const OVERLOADED: &str = "Our servers are currently overloaded. Please try again later.";

    /// Captured live on 2026-07-29, verbatim. Both of these arrive on an HTTP
    /// 200 stream, one after the other, and then the stream closes cleanly.
    const ERROR_EVENT: &str = r#"{"type":"error","error":{"type":"service_unavailable_error","code":"server_is_overloaded","message":"Our servers are currently overloaded. Please try again later.","param":null},"sequence_number":2}"#;

    const FAILED_EVENT: &str = r#"{"type":"response.failed","response":{"id":"resp_09","object":"response","status":"failed","error":{"code":"server_is_overloaded","message":"Our servers are currently overloaded. Please try again later."},"incomplete_details":null}}"#;

    #[test]
    fn an_error_event_is_a_failure_carrying_the_servers_sentence() {
        assert_eq!(
            failure_message(&classify(ERROR_EVENT)).as_deref(),
            Some(OVERLOADED)
        );
    }

    #[test]
    fn a_failed_response_is_a_failure_carrying_the_servers_sentence() {
        assert_eq!(
            failure_message(&classify(FAILED_EVENT)).as_deref(),
            Some(OVERLOADED)
        );
    }

    #[test]
    fn a_failed_response_without_an_error_object_still_reports_failure() {
        // The absence of an explanation must not read as success.
        let event = classify(r#"{"type":"response.failed","response":{"status":"failed"}}"#);
        assert_eq!(
            failure_message(&event).as_deref(),
            Some(UNSPECIFIED_FAILURE)
        );
    }

    #[test]
    fn ordinary_events_are_never_failures() {
        // Including event types we do not model: `Unknown` must stay inert, or
        // every unrecognised event would abort the stream.
        for data in [
            r#"{"type":"response.output_text.delta","delta":"PONG"}"#,
            r#"{"type":"response.reasoning_summary_text.delta","delta":"thinking"}"#,
            r#"{"type":"response.completed","response":{"status":"completed"}}"#,
            r#"{"type":"response.created","response":{"status":"in_progress"}}"#,
            r#"{"type":"response.output_item.added"}"#,
            "not json at all",
        ] {
            assert_eq!(failure_message(&classify(data)), None, "data: {data}");
        }
    }

    #[test]
    fn a_failure_reaches_the_reasoning_stream_as_an_api_error() {
        // `ApiError` is the variant the CLI prints verbatim, with no wrapper.
        match to_reason_event(classify(ERROR_EVENT)) {
            ReasonEvent::Err(Error::ApiError(message)) => assert_eq!(message, OVERLOADED),
            other => panic!("expected an ApiError, got: {other}"),
        }
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
            matches!(ev, ResponseEvent::ReasoningDelta { ref delta, summary_index: 0 } if delta.contains("17 times 23")),
            "got {ev:?}"
        );
    }

    #[test]
    fn a_delta_without_a_summary_index_still_classifies() {
        // Every field on this endpoint is one the server may stop sending.
        let ev = classify(r#"{"type":"response.reasoning_summary_text.delta","delta":"x"}"#);
        assert_eq!(
            ev,
            ResponseEvent::ReasoningDelta {
                delta: "x".to_string(),
                summary_index: 0,
            }
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
            to_reason_event(ResponseEvent::ReasoningDelta { delta: "r".to_string(), summary_index: 0 }),
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

    /// Collects the reasoning text one join produced for a run of deltas, the
    /// way the CLI concatenates them on screen.
    fn joined(deltas: &[(u32, &str)]) -> String {
        let mut join = SummaryJoin::default();
        let mut out = String::new();
        for &(summary_index, delta) in deltas {
            if let ReasonEvent::Reasoning(text) = join.project(ResponseEvent::ReasoningDelta {
                delta: delta.to_string(),
                summary_index,
            }) {
                out.push_str(&text);
            }
        }
        out
    }

    #[test]
    fn deltas_within_one_part_are_concatenated_untouched() {
        // A part arrives in pieces mid-word. Inserting anything between them
        // would break the word.
        assert_eq!(
            joined(&[(0, "**Calc"), (0, "ulating"), (0, "**")]),
            "**Calculating**"
        );
    }

    #[test]
    fn a_new_part_opens_with_a_blank_line() {
        // The bug report: nine summary headings printed as one unreadable row.
        assert_eq!(
            joined(&[(0, "**One**"), (1, "**Two**"), (2, "**Three**")]),
            "**One**\n\n**Two**\n\n**Three**"
        );
    }

    #[test]
    fn the_first_part_is_not_preceded_by_a_break() {
        // A summary that opened with blank rows would push the first heading
        // off the top of a short terminal.
        assert_eq!(joined(&[(0, "**One**")]), "**One**");
    }

    #[test]
    fn a_summary_that_starts_at_a_nonzero_index_still_opens_flush() {
        // Nothing promises the first part we see is numbered 0 — a resumed or
        // re-ordered stream need not start there, and "first seen" is the only
        // thing that matters for the leading break.
        assert_eq!(
            joined(&[(3, "**One**"), (4, "**Two**")]),
            "**One**\n\n**Two**"
        );
    }

    #[test]
    fn an_unrelated_event_between_parts_does_not_lose_the_seam() {
        // `response.completed`, a comment, an unknown type — plenty arrives
        // between two parts, and none of it may reset the boundary.
        let mut join = SummaryJoin::default();
        join.project(ResponseEvent::ReasoningDelta {
            delta: "**One**".to_string(),
            summary_index: 0,
        });
        assert!(matches!(
            join.project(ResponseEvent::Unknown),
            ReasonEvent::Empty
        ));
        assert!(matches!(
            join.project(ResponseEvent::ReasoningDelta {
                delta: "**Two**".to_string(),
                summary_index: 1,
            }),
            ReasonEvent::Reasoning(ref s) if s == "\n\n**Two**"
        ));
    }

    #[test]
    fn joining_leaves_answer_deltas_alone() {
        // The answer is cached verbatim. A break leaking into it would be
        // written to disk.
        let mut join = SummaryJoin::default();
        assert!(matches!(
            join.project(ResponseEvent::OutputTextDelta { delta: "x".to_string() }),
            ReasonEvent::Delta(ref s) if s == "x"
        ));
    }
}
