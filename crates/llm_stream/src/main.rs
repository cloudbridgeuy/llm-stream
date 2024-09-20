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

    let mut args = Args::parse();

    log::info!("args: {:#?}", args);

    let home = std::env::var("HOME")?;

    let config_dir = args
        .config_dir
        .clone()
        .unwrap_or("~/.config/llm-stream".to_string())
        .replace('~', &home);

    args.config_dir = Some(config_dir.clone());

    if !std::path::Path::new(&config_dir).exists() {
        std::fs::create_dir_all(&config_dir)?;
    }

    args.config_file = if let Some(config_file) = args.config_file {
        Some(config_file.clone().replace('~', &home))
    } else {
        Some(config_dir.to_string() + "/config.toml")
    };

    if args.config {
        if let Some(config_file) = args.config_file {
            println!("{}", config_file);
        } else {
            println!("{}/config.toml", args.config_dir.unwrap());
        }
        return Ok(());
    }

    if args.dir {
        println!("{}", args.config_dir.unwrap());
        return Ok(());
    }

    let (args, config) = build_config(args)?;

    log::info!("config: {:#?}", config);

    let (args, config) = parse_args(args, config)?;

    log::info!("parsed args: {:#?}", args);

    let args = merge_args_and_cache(args)?;

    log::info!("merged args and cache: {:#?}", args);

    if args.show {
        // Read the cache file from `args.config_dir/args.from`
        let cache_file = format!(
            "{}/cache/{}.toml",
            args.config_dir.clone().expect("can't find cache directory"),
            args.from
                .clone()
                .expect("--from or --from-last needs to be defined when run with --show")
        );

        // Read the contents of cache_file
        let cache = std::fs::read_to_string(&cache_file)?;

        show(args, &cache)?;

        return Ok(());
    }

    let args = merge_args_and_config(args, config)?;

    log::info!("merged args and config: {:#?}", args);

    if args.print_conversation {
        let json = serde_json::to_string_pretty(&args.conversation)?;

        eprintln!("{}", &json);
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
