use llm_stream::ollama;

use crate::prelude::*;

const DEFAULT_URL: &str = "http://localhost:11434";
const DEFAULT_MODEL: &str = "llama3.2";

// From ConversationRole to ollama::Role
impl From<ConversationRole> for ollama::Role {
    fn from(role: ConversationRole) -> Self {
        match role {
            ConversationRole::User => ollama::Role::User,
            ConversationRole::Assistant => ollama::Role::Assistant,
            ConversationRole::System => ollama::Role::System,
        }
    }
}

pub async fn run(mut args: Args) -> Result<()> {
    let url = match args.api_base_url.take() {
        Some(url) => url,
        None => DEFAULT_URL.to_string(),
    };

    log::info!("url: {}", url);

    let client = ollama::Client::new(url);

    log::info!("client: {:#?}", client);

    let mut messages: Vec<ollama::Message> = Default::default();

    for message in &args.conversation {
        messages.push(ollama::Message {
            role: message.role.into(),
            content: message.content.clone(),
        });
    }

    let mut body = ollama::MessageBody::new(
        args.model
            .take()
            .unwrap_or(DEFAULT_MODEL.to_string())
            .as_ref(),
        messages,
    );

    if let Some(system) = args.system.take() {
        let system_message = ollama::Message {
            role: ollama::Role::System,
            content: system,
        };

        body.messages.insert(0, system_message);
    }

    body.options = Some(ollama::MessageBodyOptions {
        temperature: args.temperature,
        top_p: args.top_p,
        top_k: args.top_k,
        ..Default::default()
    });

    log::info!("body: {:#?}", body);

    let stream = client.delta(&body)?;

    handle_stream(stream, args).await
}
