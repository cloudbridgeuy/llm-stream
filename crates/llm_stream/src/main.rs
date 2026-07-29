#![allow(clippy::result_large_err)]
#![allow(dead_code)]
use clap::Parser;
use std::io::{Read, Write};
use std::process::Command;
use tempfile::tempdir;

mod anthropic;
mod args;
mod auth;
mod chatgpt;
mod config;
mod conversation;
mod error;
mod google;
mod groq;
mod mistral;
mod mistral_fim;
mod ollama;
mod openai;
mod prelude;
mod printer;

use crate::prelude::*;

#[tokio::main]
async fn main() {
    env_logger::init();

    if let Err(error) = try_main().await {
        eprintln!("{}", crate::error::user_message(&error));
        std::process::exit(1);
    }
}

/// Everything `main` used to do.
///
/// Split out so that the one place an error is rendered for a human is a single
/// pure function, rather than Rust's default `Debug` printing of whatever
/// bubbled up.
async fn try_main() -> Result<()> {
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

    if args.login {
        let tokens = auth::flow::login(std::path::Path::new(&config_dir))?;
        let info = auth::token::account_info(&tokens.id_token).unwrap_or_default();
        println!(
            "signed in as {} ({})",
            info.email.unwrap_or_else(|| "unknown".to_string()),
            info.plan_type.unwrap_or_else(|| "unknown plan".to_string())
        );
        return Ok(());
    }

    if args.logout {
        auth::store::clear(std::path::Path::new(&config_dir))?;
        println!("signed out");
        return Ok(());
    }

    if args.login_status {
        match auth::flow::status(std::path::Path::new(&config_dir))? {
            Some((info, expiry)) => {
                let remaining = expiry
                    .duration_since(std::time::SystemTime::now())
                    .map_or(0, |d| d.as_secs());
                println!(
                    "signed in as {} ({}) — access token valid for {remaining}s",
                    info.email.unwrap_or_else(|| "unknown".to_string()),
                    info.plan_type.unwrap_or_else(|| "unknown plan".to_string())
                );
            }
            None => println!("{}", auth::flow::NOT_SIGNED_IN),
        }
        return Ok(());
    }

    // Dispatched here, with the auth flags, rather than after `build_config`:
    // `parse_args` reads stdin when stdin is not a terminal, and
    // `echo x | llm-stream --models` has no prompt for it to read. The cost is
    // that `--models` sees only command-line values — a `base_url` in
    // `config.toml` is not consulted, which is fine for a command that targets
    // one specific endpoint by construction.
    if args.models {
        return chatgpt::probe_models(args).await;
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

    if args.templates {
        let template_lines = config
            .templates
            .unwrap_or_default()
            .iter()
            .map(|template| (*template).clone().into())
            .collect::<Vec<TemplateLine>>();
        return templates(template_lines, args.no_color);
    }

    if args.presets {
        let preset_lines = config
            .presets
            .unwrap_or_default()
            .iter()
            .map(|preset| (*preset).clone().into())
            .collect::<Vec<PresetLine>>();
        return presets(preset_lines, args.no_color);
    }

    log::info!("parsed args: {:#?}", args);

    let args = merge_args_and_cache(args)?;

    log::info!("merged args and cache: {:#?}", args);

    if args.list {
        return list(args);
    }

    // `--last` is a narrowing of `--show`, not a modifier that only counts when
    // `--show` is also present. On its own it used to fall through to the
    // prompt path and print nothing at all, which read as a no-op.
    if args.show || args.last {
        return show(args);
    }

    let mut args = merge_args_and_config(args, config)?;

    log::info!("merged args and config: {:#?}", args);

    if !args.conversation.is_empty() {
        let last = args.conversation[args.conversation.len() - 1].clone();
        if last.content.is_empty() {
            args.conversation.pop();
        }
    }

    if args.print_conversation {
        let json = serde_json::to_string_pretty(&args.conversation)?;

        eprintln!("{}", &json);
    }

    if args.dry_run {
        return Ok(());
    }

    if args.editor && std::env::var("EDITOR").is_ok() {
        let editor = std::env::var("EDITOR").unwrap();

        // Create a directory inside of `env::temp_dir()`.
        let dir = tempdir()?;

        let file_path = dir.path().join("prompt.toml");
        let mut file = std::fs::File::create(&file_path)?;
        let cache_toml = toml::to_string(&args)?;

        writeln!(file, "{}", &cache_toml)?;

        Command::new(editor)
            .arg(&file_path)
            .status()
            .expect("Something failed while editing the prompt");

        let mut editable = String::new();
        std::fs::File::open(file_path)
            .expect("Could not open file")
            .read_to_string(&mut editable)?;

        let edited_args: Args = toml::from_str(&editable)?;

        args.conversation = edited_args.conversation;
    }

    match args.api {
        Some(Api::OpenAi) => openai::run(args).await,
        Some(Api::Anthropic) => anthropic::run(args).await,
        Some(Api::Google) => google::run(args).await,
        Some(Api::Mistral) => mistral::run(args).await,
        Some(Api::MistralFim) => mistral_fim::run(args).await,
        Some(Api::Ollama) => ollama::run(args).await,
        Some(Api::Groq) => groq::run(args).await,
        Some(Api::DeepSeek) => openai::reason(args).await,
        // The guard must precede the plain arm; `match` takes the first that
        // fits. This mirrors `Api::DeepSeek`, the other provider with a
        // reasoning stream.
        Some(Api::ChatGpt) if args.reasoning_summary => chatgpt::reason(args).await,
        Some(Api::ChatGpt) => chatgpt::run(args).await,
        None => Err(Error::ApiNotSpecified),
    }
}
