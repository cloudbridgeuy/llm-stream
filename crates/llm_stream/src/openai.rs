use llm_stream::openai;

use crate::prelude::*;

const DEFAULT_URL: &str = "https://api.openai.com/v1";
const DEFAULT_MODEL: &str = "gpt-4o";
const DEFAULT_ENV: &str = "OPENAI_API_KEY";

// From ConversationRole to openai::Role
impl From<ConversationRole> for openai::Role {
    fn from(role: ConversationRole) -> Self {
        match role {
            ConversationRole::User => openai::Role::User,
            ConversationRole::Assistant => openai::Role::Assistant,
            ConversationRole::System => openai::Role::System,
        }
    }
}

pub async fn run(mut args: Args) -> Result<()> {
    let key = match args.api_key.take() {
        Some(key) => key,
        None => {
            let environment_variable = match args.api_env.take() {
                Some(env) => env,
                None => DEFAULT_ENV.to_string(),
            };
            std::env::var(environment_variable)?
        }
    };
    log::info!("key: {}", key);

    let url = match args.api_base_url.take() {
        Some(url) => url,
        None => DEFAULT_URL.to_string(),
    };

    log::info!("url: {}", url);

    let auth = openai::Auth::new(key);

    log::info!("auth: {:#?}", auth);

    let client = openai::Client::new(auth, url);

    log::info!("client: {:#?}", client);

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

    body.temperature = args.temperature;
    body.top_p = args.top_p;
    if let Some(max_tokens) = args.max_tokens {
        body.max_tokens = Some(max_tokens);
    };

    log::info!("body: {:#?}", body);

    let stream = client.delta(&body)?;

    handle_stream(stream, args).await
}
