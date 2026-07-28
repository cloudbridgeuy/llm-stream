//! Provider-agnostic streaming events.
//!
//! `ReasonEvent` lives here rather than in `openai` because more than one
//! provider streams reasoning, and the CLI's stream handler needs a single type
//! that covers all of them.

use crate::error::Error;

pub enum ReasonEvent {
    Reasoning(String),
    Delta(String),
    Empty,
    Connected,
    Comment(String),
    Err(Error),
}

impl std::fmt::Display for ReasonEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReasonEvent::Reasoning(content) => write!(f, "Reasoning: {}", content),
            ReasonEvent::Delta(content) => write!(f, "Delta: {}", content),
            ReasonEvent::Empty => write!(f, "Empty"),
            ReasonEvent::Connected => write!(f, "Connected"),
            ReasonEvent::Comment(comment) => write!(f, "Comment: {}", comment),
            ReasonEvent::Err(e) => write!(f, "Error: {}", e),
        }
    }
}
