use llm_stream::mistral_fim;

use crate::prelude::*;

const DEFAULT_URL: &str = "https://api.mistral.ai/v1";
const DEFAULT_MODEL: &str = "codestral-2405";
const DEFAULT_ENV: &str = "MISTRAL_API_KEY";

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

    let auth = mistral_fim::Auth::new(key);

    log::info!("auth: {:#?}", auth);

    let client = mistral_fim::Client::new(auth, url);

    log::info!("client: {:#?}", client);

    let prompt = args
        .conversation
        .iter()
        .filter(|m| m.role == ConversationRole::User)
        .map(|m| m.content.clone())
        .collect::<Vec<String>>()
        .join("\n");

    let mut body = mistral_fim::MessageBody::new(
        args.model
            .take()
            .unwrap_or(DEFAULT_MODEL.to_string())
            .as_ref(),
        prompt,
        args.suffix.take(),
    );

    body.temperature = args.temperature;
    body.top_p = args.top_p;
    if let Some(max_tokens) = args.max_tokens {
        body.max_tokens = Some(max_tokens);
    };
    if let Some(min_tokens) = args.min_tokens {
        body.min_tokens = Some(min_tokens);
    };

    log::info!("body: {:#?}", body);

    let stream = client.delta(&body)?;

    handle_stream(stream, args).await
}
