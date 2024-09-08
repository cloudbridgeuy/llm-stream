use llm_stream::google;

use crate::prelude::*;

const DEFAULT_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
const DEFAULT_MODEL: &str = "gemini-1.5-pro";
const DEFAULT_ENV: &str = "GOOGLE_API_KEY";

// From ConversationRole to google::Role
impl From<ConversationRole> for google::Role {
    fn from(role: ConversationRole) -> Self {
        match role {
            crate::ConversationRole::User => google::Role::User,
            crate::ConversationRole::Assistant => google::Role::Model,
            crate::ConversationRole::System => google::Role::User,
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

    let auth = google::Auth::new(key);
    log::info!("auth: {:#?}", auth);

    let client = google::Client::new(auth, url);
    log::info!("client: {:#?}", client);

    let mut contents: Vec<google::Content> = Default::default();

    for message in &args.conversation {
        if message.role == ConversationRole::System {
            contents.insert(
                0,
                google::Content {
                    parts: vec![google::Part {
                        text: message.content.clone(),
                    }],
                    role: message.role.into(),
                },
            );
            continue;
        }

        contents.push(google::Content {
            parts: vec![google::Part {
                text: message.content.clone(),
            }],
            role: message.role.into(),
        });
    }

    let mut body = google::MessageBody::new(
        args.model
            .take()
            .unwrap_or(DEFAULT_MODEL.to_string())
            .as_ref(),
        contents,
    );

    if let Some(system) = args.system.take() {
        let system_message = google::Content {
            parts: vec![google::Part { text: system }],
            role: google::Role::User,
        };

        body.contents.insert(0, system_message);
    }

    body.generation_config = Some(google::GenerationConfig {
        max_output_tokens: Some(args.max_tokens.unwrap_or(4096)),
        temperature: args.temperature,
        top_p: args.top_p,
        top_k: args.top_k,
        ..Default::default()
    });

    log::info!("body: {:#?}", body);

    let stream = client.delta(&body)?;

    handle_stream(stream, args).await
}
