use serde::{Deserialize, Serialize};

/// LLM-Stream Role to define the actor currently speaking.
///
/// This enum should be converted to the appropriate API Role for each implementation.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConversationRole {
    #[default]
    User,
    Assistant,
    System,
}

/// LLM-Stream Convversation message.
///
/// THis struct should be converted to the appropriate API struct for each implementation.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationMessage {
    pub role: ConversationRole,
    pub content: String,
}

/// Simplified type that identifies a conversation as a vector of Conversation Messages.
pub type Conversation = Vec<ConversationMessage>;
