use config_file::FromConfigFile;
use futures::stream::{Stream, TryStreamExt};
use serde_json::Value;
use std::io::Write;

pub use crate::args::{Api, Args};
pub use crate::config::Config;
pub use crate::conversation::*;
pub use crate::error::Error;

pub type Result<T> = std::result::Result<T, Error>;

const SYSTEM_TEMPLATE: &str = "system";
const PROMPT_TEMPLATE: &str = "prompt";

/// Handles the stream of text from the LLM and prints it to the terminal.
pub async fn handle_stream(
    mut stream: impl Stream<Item = std::result::Result<String, llm_stream::error::Error>>
        + std::marker::Unpin,
    args: Args,
) -> Result<()> {
    let mut previous_output = String::new();
    let mut accumulated_content_bytes: Vec<u8> = Vec::new();

    let is_terminal = atty::is(atty::Stream::Stdout);

    let mut sp = if args.quiet.is_none() || (args.quiet == Some(false) && is_terminal) {
        Some(spinners::Spinner::new(
            spinners::Spinners::OrangeBluePulse,
            "Loading...".into(),
        ))
    } else {
        None
    };

    let language = args.language.unwrap_or("markdown".to_string());
    let theme = Some(args.theme.unwrap_or("ansi".to_string()));

    while let Ok(Some(text)) = stream.try_next().await {
        if is_terminal && sp.is_some() {
            // TODO: Find a better way to clean the spinner from the terminal.
            sp.take().unwrap().stop();
            std::io::stdout().flush()?;
            crossterm::execute!(std::io::stdout(), crossterm::cursor::MoveToColumn(0))?;
            print!("                      ");
            crossterm::execute!(std::io::stdout(), crossterm::cursor::MoveToColumn(0))?;
        }

        if !is_terminal {
            // If not a terminal, print each instance of `text` directly to `stdout`
            print!("{}", text);
            std::io::stdout().flush()?;
            continue;
        }

        accumulated_content_bytes.extend_from_slice(text.as_bytes());

        let output = crate::printer::CustomPrinter::new(&language, theme.as_deref())?
            .input_from_bytes(&accumulated_content_bytes)
            .print()?;

        let unprinted_lines = output
            .lines()
            .skip(if previous_output.lines().count() == 0 {
                0
            } else {
                previous_output.lines().count() - 1
            })
            .collect::<Vec<_>>()
            .join("\n");

        crossterm::execute!(std::io::stdout(), crossterm::cursor::MoveToColumn(0))?;
        print!("{unprinted_lines}");
        std::io::stdout().flush()?;

        // Update the previous output
        previous_output = output;
    }

    Ok(())
}

/// Merges two JSON objects defined as `serde_json::Value`.
pub fn merge(a: &mut Value, b: Value) {
    if let Value::Object(a) = a {
        if let Value::Object(b) = b {
            for (k, v) in b {
                if v.is_null() {
                    a.remove(&k);
                } else {
                    merge(a.entry(k).or_insert(Value::Null), v);
                }
            }

            return;
        }
    }

    *a = b;
}

/// Builds the arguments struct based on a combination of the following inputs,
/// in this order.
///
/// 1. CLI options/Environment variables.
/// 2. Environment variable.
/// 3. Config preset and/or template options.
/// 4. Config file default options.
pub fn build_args(mut args: Args) -> Result<Args> {
    let prompt = args.prompt.to_string();
    let stdin = if args.file.is_some() {
        args.file.clone().unwrap().contents()?
    } else {
        "".to_string()
    };

    log::info!("info: {:#?}", args);

    let home = std::env::var("HOME")?;
    args.config_dir = args.config_dir.clone().replace('~', &home);

    if !std::path::Path::new(&args.config_dir).exists() {
        std::fs::create_dir_all(args.config_dir.clone())?;
    }

    let config_dir = args.config_dir.clone();

    args.config_file = if let Some(config_file) = args.config_file {
        Some(config_file.clone().replace('~', &home))
    } else {
        Some(config_dir.to_string() + "/config.toml")
    };

    let config_file = args.config_file.clone().unwrap();

    log::info!("config_dir: {}", &config_dir);
    log::info!("config_file: {}", &config_file);

    // Check if `path` exists
    let config = if !std::path::Path::new(&config_file).exists() {
        let config = Config::new();
        let config_toml = toml::to_string(&config)?;
        // Store `config_toml` in the `&config_file` path.
        std::fs::write(&config_file, config_toml)?;

        config
    } else {
        Config::from_config_file(&config_file)?
    };

    log::info!("config: {:#?}", config);

    if let Some(preset) = args.preset.clone() {
        let p = config
            .presets
            .unwrap_or_default()
            .into_iter()
            .find(|p| p.name == preset);

        if let Some(p) = p {
            if args.api.is_none() {
                args.api = Some(p.api);
            }

            if args.top_p.is_none() {
                args.top_p = p.top_p;
            }
            if args.top_k.is_none() {
                args.top_k = p.top_k;
            }
            if args.temperature.is_none() {
                args.temperature = p.temperature;
            }
            if args.system.is_none() {
                args.system = p.system;
            }
            if args.max_tokens.is_none() {
                args.max_tokens = p.max_tokens;
            }
            if args.api_version.is_none() {
                args.api_version = p.version;
            }
            if args.api_env.is_none() {
                args.api_env = p.env;
            }
            if args.api_key.is_none() {
                args.api_key = p.key;
            }
            if args.api_base_url.is_none() {
                args.api_base_url = p.base_url;
            }
            if args.model.is_none() {
                args.model = p.model;
            }
        }
    };

    if args.top_p.is_none() {
        args.top_p = config.top_p;
    }
    if args.top_k.is_none() {
        args.top_k = config.top_k;
    }
    if args.temperature.is_none() {
        args.temperature = config.temperature;
    }
    if args.system.is_none() {
        args.system = config.system;
    }
    if args.max_tokens.is_none() {
        args.max_tokens = config.max_tokens;
    }
    if args.api_version.is_none() {
        args.api_version = config.version;
    }
    if args.api_env.is_none() {
        args.api_env = config.env;
    }
    if args.api_key.is_none() {
        args.api_key = config.key;
    }
    if args.api_base_url.is_none() {
        args.api_base_url = config.base_url;
    }
    if args.model.is_none() {
        args.model = config.model;
    }
    if args.quiet.is_none() {
        args.quiet = config.quiet;
    }
    if args.language.is_none() {
        args.language = config.language;
    }
    if args.theme.is_none() {
        args.theme = config.theme;
    }
    if args.api.is_none() {
        args.api = config.api;
    }

    log::info!("globals: {:#?}", args);

    let prompt: String = if let Some(ref template) = args.template {
        let t = config
            .templates
            .unwrap_or_default()
            .into_iter()
            .find(|t| t.name == *template);

        if t.is_none() {
            return Err(Error::TemplateNotFound);
        }

        let t = t.unwrap();

        log::info!("template: {:#?}", t);

        let system = args.system.clone().unwrap_or_default().to_string();
        let suffix = args.suffix.clone().unwrap_or_default().to_string();
        let language = args.language.clone();

        let mut default_vars = t.default_vars.unwrap_or_default();
        let vars = args.vars.take().unwrap_or_default();
        merge(&mut default_vars, vars);

        let mut value = serde_json::json!({
            "prompt": prompt,
            "system": system,
            "stdin": stdin,
            "suffix": suffix,
            "language": language,
        });

        merge(&mut value, default_vars);

        let context = tera::Context::from_value(value)?;

        log::info!("context: {:#?}", context);

        let mut tera = tera::Tera::default();

        if let Some(system) = t.system {
            tera.add_raw_template(SYSTEM_TEMPLATE, &system)?;
            args.system = Some(tera.render(SYSTEM_TEMPLATE, &context)?);
        }

        tera.add_raw_template(PROMPT_TEMPLATE, t.template.as_ref())?;

        tera.render(PROMPT_TEMPLATE, &context)?
    } else if !stdin.is_empty() {
        format!("{}\n{}", stdin, prompt)
    } else {
        prompt
    };

    args.conversation.push(ConversationMessage {
        role: ConversationRole::User,
        content: prompt.clone(),
    });

    if args.system.is_some() {
        args.conversation.insert(
            0,
            ConversationMessage {
                role: ConversationRole::System,
                content: args.system.clone().unwrap(),
            },
        )
    };

    Ok(args)
}
