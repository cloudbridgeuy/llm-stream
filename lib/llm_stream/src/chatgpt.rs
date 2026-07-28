//! OpenAI Responses API over a ChatGPT subscription.
//!
//! This is **not** the Chat Completions API used by [`crate::openai`]. The
//! system prompt travels in a top-level `instructions` field, history travels
//! as `input` items, and text arrives as `response.output_text.delta` events.
//!
//! The endpoint is private to OpenAI's Codex client and undocumented. Every
//! constant here came from live measurement, not from a specification.

use serde::{Deserialize, Serialize};

/// Base URL of the private Codex backend a ChatGPT subscription can reach.
pub const DEFAULT_URL: &str = "https://chatgpt.com/backend-api/codex";

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
}
