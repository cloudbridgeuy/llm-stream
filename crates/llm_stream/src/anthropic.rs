use llm_stream::anthropic;

use crate::prelude::*;

const DEFAULT_URL: &str = "https://api.anthropic.com/v1";
const DEFAULT_MODEL: &str = "claude-3-5-sonnet-20240620";
const DEFAULT_ENV: &str = "ANTHROPIC_API_KEY";

// From ConversationRole to anthropic::Role
impl From<ConversationRole> for anthropic::Role {
    fn from(role: ConversationRole) -> Self {
        match role {
            ConversationRole::User => anthropic::Role::User,
            ConversationRole::Assistant => anthropic::Role::Assistant,
            ConversationRole::System => anthropic::Role::User,
        }
    }
}

pub async fn run(conversation: Conversation, mut args: Args) -> Result<()> {
    let key = match args.globals.api_key.take() {
        Some(key) => key,
        None => {
            let environment_variable = match args.globals.api_env.take() {
                Some(env) => env,
                None => DEFAULT_ENV.to_string(),
            };
            std::env::var(environment_variable)?
        }
    };
    log::info!("key: {}", key);

    let url = match args.globals.api_base_url.take() {
        Some(url) => url,
        None => DEFAULT_URL.to_string(),
    };
    log::info!("url: {}", url);

    let auth = anthropic::Auth::new(key, args.globals.api_version.clone());

    log::info!("auth: {:#?}", auth);

    let client = anthropic::Client::new(auth, url);

    log::info!("client: {:#?}", client);

    let mut messages: Vec<anthropic::Message> = Default::default();

    for message in conversation {
        if message.role == ConversationRole::System {
            continue;
        }

        messages.push(anthropic::Message {
            role: message.role.into(),
            content: message.content,
        });
    }

    let mut body = anthropic::MessageBody::new(
        args.globals
            .model
            .take()
            .unwrap_or(DEFAULT_MODEL.to_string())
            .as_ref(),
        messages,
        args.globals.max_tokens.unwrap_or(4096),
    );

    body.system = args.globals.system.take();
    body.temperature = args.globals.temperature;
    body.top_p = args.globals.top_p;
    body.top_k = args.globals.top_k;

    log::info!("body: {:#?}", body);

    let stream = client.delta(&body)?;

    handle_stream(stream, args.globals).await
}
