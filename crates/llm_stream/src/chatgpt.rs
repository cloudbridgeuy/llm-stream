//! The `ChatGPT` subscription provider.
//!
//! Streams GPT models through a ChatGPT subscription's OAuth credentials rather
//! than a metered API key. Credentials come from `crate::auth`, which
//! `--login` populates.

use llm_stream::chatgpt as api;

use crate::prelude::*;

/// The model used when `--model` is absent. There is deliberately no local
/// allowlist: the server decides which models a plan may use, the naming is
/// unpredictable, and a hardcoded list would lock users out of newly enabled
/// models. `--model` is passed through verbatim.
pub const DEFAULT_MODEL: &str = "gpt-5.6-sol";

/// A conversation split the way the Responses API wants it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MappedConversation {
    pub instructions: Option<String>,
    pub input: Vec<api::InputItem>,
}

/// Splits a flat conversation into a system prompt and a list of input items.
///
/// System messages never reach `input`: the Responses API carries the system
/// prompt in `instructions`, and `prelude.rs` has already folded the configured
/// system prompt into the conversation by the time we get here.
#[must_use]
pub fn map_conversation(
    conversation: &Conversation,
    system_override: Option<&str>,
) -> MappedConversation {
    let from_conversation = conversation
        .iter()
        .filter(|message| message.role == ConversationRole::System)
        .map(|message| message.content.as_str())
        .filter(|content| !content.is_empty())
        .collect::<Vec<_>>();

    let instructions = match system_override {
        Some(text) if !text.is_empty() => Some(text.to_owned()),
        _ if from_conversation.is_empty() => None,
        _ => Some(from_conversation.join("\n\n")),
    };

    // Parity with `openai.rs`: drop a turn whose text repeats one already
    // queued. This also drops an assistant turn that echoes a user turn
    // verbatim, which is the existing behaviour and not worth diverging on.
    let input = conversation
        .iter()
        .filter(|message| message.role != ConversationRole::System)
        .fold(Vec::new(), |mut acc: Vec<api::InputItem>, message| {
            if !acc.iter().any(|item| item.text() == message.content) {
                acc.push(match message.role {
                    ConversationRole::Assistant => {
                        api::InputItem::assistant(message.content.as_str())
                    }
                    _ => api::InputItem::user(message.content.as_str()),
                });
            }
            acc
        });

    MappedConversation {
        instructions,
        input,
    }
}

/// Streams an answer from a `ChatGPT` subscription.
pub async fn run(mut args: Args) -> Result<()> {
    let config_dir = args
        .config_dir
        .clone()
        .ok_or_else(|| Error::Auth("the config directory was not resolved".to_string()))?;

    // Loads `<config_dir>/auth.json`, refreshing the access token if it is
    // close to expiry. Errors with the `--login` instruction when signed out.
    let tokens = crate::auth::flow::ensure_valid(std::path::Path::new(&config_dir))?;

    let url = args
        .api_base_url
        .take()
        .unwrap_or_else(|| api::DEFAULT_URL.to_string());
    log::info!("url: {url}");

    let client = api::Client::new(
        api::Auth::new(tokens.access_token, tokens.account_id),
        url,
    );

    let mapped = map_conversation(&args.conversation, args.system.as_deref());

    let model = args
        .model
        .take()
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());

    let mut body = api::MessageBody::new(&model, mapped.input);
    body.instructions = mapped.instructions;
    // `args.from` is `Some(id)` exactly when a conversation is being continued,
    // which is when a stable cache key is worth having.
    body.prompt_cache_key = args.from.clone();

    log::info!("body: {body:#?}");

    let stream = client.delta(&body)?;

    handle_stream(stream, args).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: ConversationRole, content: &str) -> ConversationMessage {
        ConversationMessage {
            role,
            content: content.to_owned(),
        }
    }

    #[test]
    fn first_system_message_becomes_instructions() {
        let c = vec![
            msg(ConversationRole::System, "be terse"),
            msg(ConversationRole::User, "hi"),
        ];
        let m = map_conversation(&c, None);
        assert_eq!(m.instructions.as_deref(), Some("be terse"));
        assert_eq!(m.input.len(), 1);
    }

    #[test]
    fn instructions_are_none_when_no_system_message() {
        let m = map_conversation(&vec![msg(ConversationRole::User, "hi")], None);
        assert!(m.instructions.is_none());
        assert_eq!(m.input.len(), 1);
    }

    #[test]
    fn multiple_system_messages_join_with_blank_lines() {
        let c = vec![
            msg(ConversationRole::System, "a"),
            msg(ConversationRole::User, "hi"),
            msg(ConversationRole::System, "b"),
        ];
        let m = map_conversation(&c, None);
        assert_eq!(m.instructions.as_deref(), Some("a\n\nb"));
        // No system message leaks into `input`, wherever it sat in the list.
        assert_eq!(m.input.len(), 1);
    }

    #[test]
    fn system_override_takes_precedence() {
        let c = vec![msg(ConversationRole::System, "from conversation")];
        let m = map_conversation(&c, Some("from flag"));
        assert_eq!(m.instructions.as_deref(), Some("from flag"));
        assert!(m.input.is_empty());
    }

    #[test]
    fn duplicate_contents_are_removed() {
        let c = vec![
            msg(ConversationRole::User, "same"),
            msg(ConversationRole::User, "same"),
            msg(ConversationRole::User, "other"),
        ];
        assert_eq!(map_conversation(&c, None).input.len(), 2);
    }

    #[test]
    fn roles_map_to_the_right_content_part_type() {
        let c = vec![
            msg(ConversationRole::User, "u"),
            msg(ConversationRole::Assistant, "a"),
        ];
        let m = map_conversation(&c, None);
        assert_eq!(m.input[0], api::InputItem::user("u"));
        assert_eq!(m.input[1], api::InputItem::assistant("a"));
    }

    #[test]
    fn an_empty_conversation_maps_to_an_empty_request() {
        let m = map_conversation(&vec![], None);
        assert!(m.instructions.is_none());
        assert!(m.input.is_empty());
    }
}
