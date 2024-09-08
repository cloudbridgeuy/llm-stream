mod anthropic;
mod args;
mod config;
mod conversation;
mod error;
mod google;
mod mistral;
mod mistral_fim;
mod openai;
mod prelude;
mod printer;

use crate::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let args = build_args()?;

    match args.api {
        Some(Api::OpenAi) => openai::run(args).await,
        Some(Api::Anthropic) => anthropic::run(args).await,
        Some(Api::Google) => google::run(args).await,
        Some(Api::Mistral) => mistral::run(args).await,
        Some(Api::MistralFim) => mistral_fim::run(args).await,
        None => Err(Error::ApiNotSpecified),
    }
}
