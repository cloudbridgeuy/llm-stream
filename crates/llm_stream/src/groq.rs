use llm_stream::groq;

use crate::prelude::*;

const DEFAULT_URL: &str = "https://api.groq.com";
const DEFAULT_MODEL: &str = "llama-3.1-70b-versatile";
const DEFAULT_ENV: &str = "GROQ_API_KEY";

// From ConversationRole to groq::Role
impl From<ConversationRole> for groq::Role {
    fn from(role: ConversationRole) -> Self {
        match role {
            ConversationRole::User => groq::Role::User,
            ConversationRole::Assistant => groq::Role::Assistant,
            ConversationRole::System => groq::Role::System,
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

    let auth = groq::Auth::new(key);

    log::info!("auth: {:#?}", auth);

    let client = groq::Client::new(auth, url);

    log::info!("client: {:#?}", client);

    let mut messages: Vec<groq::Message> = Default::default();

    for message in &args.conversation {
        messages.push(groq::Message {
            role: message.role.into(),
            content: message.content.clone(),
        });
    }

    let mut body = groq::MessageBody::new(
        args.model
            .take()
            .unwrap_or(DEFAULT_MODEL.to_string())
            .as_ref(),
        messages,
    );

    if let Some(system) = args.system.take() {
        let system_message = groq::Message {
            role: groq::Role::System,
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
