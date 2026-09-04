use futures::StreamExt;
use llm_stream::event::ReasonEvent;
use llm_stream::openai;

use crate::prelude::*;

const DEFAULT_URL: &str = "https://integrate.api.nvidia.com/v1";
const DEFAULT_MODEL: &str = "moonshotai/kimi-k3";
const DEFAULT_ENV: &str = "NVIDIA_API_KEY";

pub async fn run(mut args: Args) -> Result<()> {
    let key = match args.api_key.take() {
        Some(key) => key,
        None => {
            let environment_variable = match args.api_env.take() {
                Some(env) => env,
                None => DEFAULT_ENV.to_string(),
            };
            std::env::var(&environment_variable).map_err(|_| {
                Error::Auth(format!(
                    "{environment_variable} is not set; export it or pass --api-key"
                ))
            })?
        }
    };

    let url = match args.api_base_url.take() {
        Some(url) => url,
        None => DEFAULT_URL.to_string(),
    };

    log::info!("url: {}", url);

    let auth = openai::Auth::new(key);

    let client = openai::Client::new(auth, url);

    let mut messages: Vec<openai::Message> = Default::default();

    for message in &args.conversation {
        messages.push(openai::Message {
            role: message.role.into(),
            content: message.content.clone(),
        });
    }

    let mut body = openai::MessageBody::new(
        args.model
            .take()
            .unwrap_or(DEFAULT_MODEL.to_string())
            .as_ref(),
        messages,
    );

    if let Some(system) = args.system.take() {
        let system_message = openai::Message {
            role: openai::Role::System,
            content: system,
        };

        body.messages.insert(0, system_message);
    }

    body.messages = body.messages.into_iter().fold(vec![], |mut acc, message| {
        if !acc.iter().any(|m| m.content == message.content) {
            acc.push(message);
        }
        acc
    });

    body.temperature = args.temperature;
    body.top_p = args.top_p;
    if let Some(max_tokens) = args.max_tokens {
        body.max_tokens = Some(max_tokens);
    }
    body.reasoning_effort = args.reasoning_effort.take();

    log::info!("body: {:#?}", body);

    let summary = args.reasoning_summary;
    let stream = client
        .reason(&body)?
        .map(move |result| result.map(|event| gate(event, summary)));

    handle_reason_stream(stream, args).await
}

/// Drops reasoning events unless the operator asked for the summary.
///
/// Reasoning consumes the stream but not the operator's screen, so without the
/// flag the model still thinks while nothing but the spinner shows it.
pub(crate) fn gate(event: ReasonEvent, summary: bool) -> ReasonEvent {
    match (event, summary) {
        (ReasonEvent::Reasoning(_), false) => ReasonEvent::Empty,
        (event, _) => event,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_is_dropped_without_the_summary_flag() {
        assert!(matches!(
            gate(ReasonEvent::Reasoning("x".into()), false),
            ReasonEvent::Empty
        ));
    }

    #[test]
    fn reasoning_survives_with_the_summary_flag() {
        assert!(matches!(
            gate(ReasonEvent::Reasoning("x".into()), true),
            ReasonEvent::Reasoning(_)
        ));
    }

    #[test]
    fn deltas_pass_through_untouched() {
        assert!(matches!(
            gate(ReasonEvent::Delta("y".into()), false),
            ReasonEvent::Delta(_)
        ));
    }

    #[test]
    fn empty_events_pass_through_untouched() {
        assert!(matches!(
            gate(ReasonEvent::Empty, false),
            ReasonEvent::Empty
        ));
    }
}
