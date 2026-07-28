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

/// Printed when the operator passes sampling flags this endpoint discards.
pub const SAMPLING_WARNING: &str =
    "warning: the chatgpt provider discards --temperature, --top-p, and --top-k; \
     use --reasoning-effort instead";

/// Printed when the operator passes API-key flags this provider does not use.
pub const CREDENTIAL_WARNING: &str =
    "warning: the chatgpt provider uses subscription credentials; \
     --api-key and --api-env are ignored (see --login)";

/// Lists the warnings the operator should see for flags that carry no meaning
/// for this provider.
///
/// Pure so the decision is testable; `run` does the printing.
#[must_use]
pub fn inert_flag_warnings(args: &Args) -> Vec<&'static str> {
    let mut warnings = Vec::new();

    if args.temperature.is_some() || args.top_p.is_some() || args.top_k.is_some() {
        warnings.push(SAMPLING_WARNING);
    }

    if args.api_key.is_some() || args.api_env.is_some() {
        warnings.push(CREDENTIAL_WARNING);
    }

    warnings
}

/// The effort a request carries when the operator asked for a summary but named
/// no effort. `medium` is the Responses API's own default, so this changes
/// nothing about the model's behaviour — it exists because our `Reasoning` type
/// makes `effort` non-optional, which is what stops an empty reasoning object
/// from being representable.
const DEFAULT_EFFORT: api::Effort = api::Effort::Medium;

/// Turns an effort string into the enum the wire accepts.
///
/// `clap` rejects bad values typed at the shell, but the same string can arrive
/// from `config.toml` or a preset where nothing has checked it. This is the one
/// place the conversion happens, and it happens once.
///
/// The `Err` is the sentence the operator should read; `run` prints it.
pub fn parse_effort(raw: &str) -> std::result::Result<api::Effort, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "low" => Ok(api::Effort::Low),
        "medium" => Ok(api::Effort::Medium),
        "high" => Ok(api::Effort::High),
        "xhigh" => Ok(api::Effort::XHigh),
        other => Err(format!(
            "unknown reasoning effort {other:?}; expected one of: low, medium, high, xhigh"
        )),
    }
}

/// Builds the request's `reasoning` field, or `None` when the operator asked for
/// neither an effort nor a summary.
///
/// Returning `None` in the default case is deliberate and load-bearing: it keeps
/// a plain `--api chatgpt "hi"` byte-for-byte identical to the request V2
/// verified against the live endpoint. This endpoint is undocumented, so there
/// is no specification to appeal to if an unrequested field turns out to matter.
/// Opting in costs the operator one flag.
pub fn reasoning_of(
    effort: Option<&str>,
    summary: bool,
) -> std::result::Result<Option<api::Reasoning>, String> {
    if effort.is_none() && !summary {
        return Ok(None);
    }

    Ok(Some(api::Reasoning {
        effort: effort.map_or(Ok(DEFAULT_EFFORT), parse_effort)?,
        // `detailed` rather than `auto`: the model decides whether to summarise
        // at all, and in live comparison `auto` produced nothing where
        // `detailed` produced a summary. Someone who typed --reasoning-summary
        // wants the odds on their side.
        summary: summary.then_some(api::Summary::Detailed),
    }))
}

/// Slugs `--models` asks about.
///
/// **Not an allowlist.** `--model` still accepts any string and the server
/// decides — see the note on [`DEFAULT_MODEL`]. This list only bounds what the
/// probe *asks*, which makes being wrong about it cheap: a slug this account
/// cannot use simply shows up refused, and a slug missing from the list is
/// still perfectly usable via `--model`.
///
/// Verified accepted on a Plus account on 2026-07-28: `gpt-5.6-sol`,
/// `gpt-5.6-terra`, `gpt-5.6-luna`, `gpt-5.5`, `gpt-5.4`.
pub const CANDIDATE_MODELS: &[&str] = &[
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
    "gpt-5.5",
    "gpt-5.4",
    "gpt-5.3",
    "gpt-5.1",
    "gpt-5-codex",
];

/// What one probe learned about one model.
///
/// Three cases, not a `bool` plus an `Option<String>`: a probe that is both
/// accepted and carrying a rejection sentence is not a state this program
/// should be able to represent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// The server opened the stream. This account may use the model.
    Accepted,
    /// The server refused, in its own words.
    Refused(String),
    /// The connection closed before the server said anything either way.
    NoAnswer,
}

/// Reads the first thing one probe stream produced.
///
/// One event is the whole signal. A model this account cannot use is refused at
/// the HTTP layer, before any SSE arrives, so anything that made it out of the
/// stream as `Ok` already means the request was accepted.
#[must_use]
pub fn probe_of(first: Option<std::result::Result<(), String>>) -> Probe {
    match first {
        Some(Ok(())) => Probe::Accepted,
        Some(Err(message)) => Probe::Refused(message),
        None => Probe::NoAnswer,
    }
}

/// Whether a stream error means "the server stopped talking" rather than "the
/// server refused".
///
/// `handle_stream` in `prelude.rs` treats `Eof` as the ordinary end of a
/// stream, so a probe must not report it as a rejection. Every other transport
/// error is something the operator should see in the table — an unreadable
/// timeout beats a silent `OK`.
#[must_use]
pub const fn is_end_of_stream(error: &llm_stream::error::Error) -> bool {
    matches!(
        error,
        llm_stream::error::Error::EventsourceClient(llm_stream::error::EventsourceError::Eof)
    )
}

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

/// A resolved request: who we are, where we are sending it, and what it says.
///
/// `run` and `reason` differ only in which stream they ask for, so everything
/// before that decision is built once, here.
struct Prepared {
    client: api::Client,
    body: api::MessageBody,
}

/// Resolves credentials, endpoint, model, and request body from `args`.
///
/// The only impure part of this module: it reads the token store and may refresh
/// an access token over the network. Every decision it makes is delegated to a
/// pure function — `map_conversation` and `reasoning_of`.
fn prepare(args: &mut Args) -> Result<Prepared> {
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

    let client = api::Client::new(api::Auth::new(tokens.access_token, tokens.account_id), url);

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
    body.reasoning = reasoning_of(args.reasoning_effort.as_deref(), args.reasoning_summary)
        .map_err(Error::InvalidValue)?;

    log::info!("body: {body:#?}");

    Ok(Prepared { client, body })
}

/// Streams an answer from a `ChatGPT` subscription.
pub async fn run(mut args: Args) -> Result<()> {
    for warning in inert_flag_warnings(&args) {
        eprintln!("{warning}");
    }

    let prepared = prepare(&mut args)?;
    let stream = prepared.client.delta(&prepared.body)?;

    handle_stream(stream, args).await
}

/// Streams the model's reasoning summary to stderr, then its answer to stdout.
///
/// The summary may be empty — the model decides whether to produce one — in
/// which case this behaves exactly like [`run`], separator included (there
/// isn't one).
pub async fn reason(mut args: Args) -> Result<()> {
    for warning in inert_flag_warnings(&args) {
        eprintln!("{warning}");
    }

    let prepared = prepare(&mut args)?;
    let stream = prepared.client.reason(&prepared.body)?;

    handle_reason_stream(stream, args).await
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

    #[test]
    fn no_warnings_for_a_plain_invocation() {
        assert!(inert_flag_warnings(&Args::default()).is_empty());
    }

    #[test]
    fn sampling_flags_warn_once_between_them() {
        let args = Args {
            temperature: Some(0.2),
            top_p: Some(0.9),
            ..Default::default()
        };
        assert_eq!(inert_flag_warnings(&args), vec![SAMPLING_WARNING]);
    }

    #[test]
    fn top_k_alone_warns() {
        let args = Args {
            top_k: Some(40),
            ..Default::default()
        };
        assert_eq!(inert_flag_warnings(&args), vec![SAMPLING_WARNING]);
    }

    #[test]
    fn credential_flags_warn_separately() {
        let args = Args {
            api_key: Some("sk-...".to_string()),
            ..Default::default()
        };
        assert_eq!(inert_flag_warnings(&args), vec![CREDENTIAL_WARNING]);
    }

    #[test]
    fn both_categories_warn_in_order() {
        let args = Args {
            temperature: Some(0.2),
            api_env: Some("OPENAI_API_KEY".to_string()),
            ..Default::default()
        };
        assert_eq!(
            inert_flag_warnings(&args),
            vec![SAMPLING_WARNING, CREDENTIAL_WARNING]
        );
    }

    #[test]
    fn no_flags_send_no_reasoning_field_at_all() {
        // Guards V2's verified default request. If this ever starts returning
        // `Some`, the one request path known to work has changed shape.
        assert_eq!(reasoning_of(None, false), Ok(None));
    }

    #[test]
    fn an_effort_alone_asks_for_no_summary() {
        assert_eq!(
            reasoning_of(Some("xhigh"), false),
            Ok(Some(api::Reasoning {
                effort: api::Effort::XHigh,
                summary: None,
            }))
        );
    }

    #[test]
    fn a_summary_alone_defaults_the_effort() {
        assert_eq!(
            reasoning_of(None, true),
            Ok(Some(api::Reasoning {
                effort: api::Effort::Medium,
                summary: Some(api::Summary::Detailed),
            }))
        );
    }

    #[test]
    fn both_flags_combine() {
        assert_eq!(
            reasoning_of(Some("low"), true),
            Ok(Some(api::Reasoning {
                effort: api::Effort::Low,
                summary: Some(api::Summary::Detailed),
            }))
        );
    }

    #[test]
    fn effort_parsing_tolerates_case_and_stray_whitespace() {
        // A hand-edited config.toml is the realistic source of both.
        assert_eq!(parse_effort("  High "), Ok(api::Effort::High));
        assert_eq!(parse_effort("XHIGH"), Ok(api::Effort::XHigh));
    }

    #[test]
    fn an_unknown_effort_names_the_value_and_the_alternatives() {
        let message = parse_effort("turbo").unwrap_err();
        assert!(message.contains("turbo"), "got: {message}");
        assert!(
            message.contains("low, medium, high, xhigh"),
            "the operator cannot act on this: {message}"
        );
    }

    #[test]
    fn a_bad_effort_from_a_config_file_fails_the_request() {
        // `clap` guards the command line. Nothing guards config.toml, which is
        // the whole reason `parse_effort` exists.
        assert!(reasoning_of(Some("maximum"), true).is_err());
    }

    #[test]
    fn the_default_model_is_probed_first() {
        // Someone reading the table top-down should see the model they get
        // without `--model` on the first line.
        assert_eq!(CANDIDATE_MODELS.first(), Some(&DEFAULT_MODEL));
    }

    #[test]
    fn every_candidate_is_listed_once() {
        // A duplicate slug is a wasted request against someone's quota.
        let mut seen = std::collections::HashSet::new();
        for model in CANDIDATE_MODELS {
            assert!(seen.insert(model), "{model} is listed twice");
        }
    }

    #[test]
    fn an_opened_stream_means_the_model_is_usable() {
        assert_eq!(probe_of(Some(Ok(()))), Probe::Accepted);
    }

    #[test]
    fn a_refusal_keeps_the_servers_own_words() {
        let sentence =
            "The 'gpt-4o' model is not supported when using Codex with a ChatGPT account.";
        assert_eq!(
            probe_of(Some(Err(sentence.to_string()))),
            Probe::Refused(sentence.to_string())
        );
    }

    #[test]
    fn a_silent_stream_is_neither_accepted_nor_refused() {
        // Reporting this as `OK` would send someone off to use a model that
        // never answered.
        assert_eq!(probe_of(None), Probe::NoAnswer);
    }

    #[test]
    fn end_of_stream_is_not_a_refusal() {
        let eof =
            llm_stream::error::Error::EventsourceClient(llm_stream::error::EventsourceError::Eof);
        assert!(is_end_of_stream(&eof));
        assert!(!is_end_of_stream(&llm_stream::error::Error::ApiError(
            "nope".to_string()
        )));
    }
}
