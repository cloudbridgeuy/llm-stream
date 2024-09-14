use clap::Parser;

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

    let args = parse_prompt(Args::parse())?;

    log::info!("args: {:#?}", args);

    let (args, config) = build_config(args)?;

    log::info!("config: {:#?}", config);

    if args.config {
        if let Some(config_file) = args.config_file {
            println!("{}", config_file);
        } else {
            println!("{}/config.toml", args.config_dir);
        }
        return Ok(());
    }

    if args.dir {
        println!("{}", args.config_dir);
        return Ok(());
    }

    let args = merge_args_and_config(args, config)?;

    log::info!("merged args: {:#?}", args);

    if args.print_conversation {
        let json = serde_json::to_string_pretty(&args.conversation)?;

        println!("{}", &json);
    }

    if args.dry_run {
        return Ok(());
    }

    match args.api {
        Some(Api::OpenAi) => openai::run(args).await,
        Some(Api::Anthropic) => anthropic::run(args).await,
        Some(Api::Google) => google::run(args).await,
        Some(Api::Mistral) => mistral::run(args).await,
        Some(Api::MistralFim) => mistral_fim::run(args).await,
        None => Err(Error::ApiNotSpecified),
    }
}
