use config_file::FromConfigFile;
use futures::stream::{Stream, TryStreamExt};
use serde_json::Value;
use std::io::{BufRead, IsTerminal, Write};

pub use crate::args::{Api, Args};
pub use crate::config::Config;
pub use crate::conversation::*;
pub use crate::error::Error;

pub type Result<T> = std::result::Result<T, Error>;

const SYSTEM_TEMPLATE: &str = "system";
const PROMPT_TEMPLATE: &str = "prompt";
const CONTENT_TEMPLATE: &str = "template";

/// Handles the stream of text from the LLM and prints it to the terminal.
pub async fn handle_stream(
    mut stream: impl Stream<Item = std::result::Result<String, llm_stream::error::Error>>
        + std::marker::Unpin,
    mut args: Args,
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

    let language = args.language.clone().unwrap_or("markdown".to_string());
    let theme = Some(args.theme.clone().unwrap_or("ansi".to_string()));

    loop {
        let result = stream.try_next().await;

        match result {
            Ok(Some(text)) => {
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
            Ok(None) => break,
            Err(llm_stream::error::Error::EventsourceClient(
                llm_stream::error::EventsourceError::Eof,
            )) => break,
            Err(e) => {
                if is_terminal && sp.is_some() {
                    // TODO: Find a better way to clean the spinner from the terminal.
                    sp.take().unwrap().stop();
                    std::io::stdout().flush()?;
                    crossterm::execute!(std::io::stdout(), crossterm::cursor::MoveToColumn(0))?;
                    print!("                      ");
                    crossterm::execute!(std::io::stdout(), crossterm::cursor::MoveToColumn(0))?;
                }
                return Err(Error::from(e));
            }
        };
    }

    if !args.no_cache {
        let id = if args.fork {
            if args.from.is_some() {
                args.parent = args.from.clone();
            }
            xid::new().to_string()
        } else {
            args.from.clone().unwrap_or(xid::new().to_string())
        };

        args.conversation.push(ConversationMessage {
            role: ConversationRole::Assistant,
            content: String::from_utf8_lossy(&accumulated_content_bytes)
                .trim()
                .to_string(),
        });

        let cache_file = format!(
            "{}/cache/{}.toml",
            args.config_dir
                .clone()
                .unwrap_or("~/.config/llm-stream".to_string()),
            id
        );

        let cache_toml = toml::to_string(&args)?;

        std::fs::write(&cache_file, cache_toml)?;

        eprintln!("\n\nCache file: {}", &cache_file);
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
        return;
    }

    *a = b;
}

/// Reads the configuration file. If it or the config directory doesn't exist, they'll be created.
pub fn build_config(args: Args) -> Result<(Args, Config)> {
    let config_dir = args
        .config_dir
        .clone()
        .unwrap_or("~/.config/llm-stream".to_string());
    let config_file = args.config_file.clone().unwrap();

    log::info!("config_dir: {}", &config_dir);
    log::info!("config_file: {}", &config_file);

    let mut config = if !std::path::Path::new(&config_file).exists() {
        let config = Config::new();
        let config_toml = toml::to_string(&config)?;
        // Store `config_toml` in the `&config_file` path.
        std::fs::write(&config_file, config_toml)?;

        config
    } else {
        Config::from_config_file(&config_file)?
    };

    let templates_dir = format!("{}/templates", &config_dir);
    if !std::path::Path::new(&templates_dir).exists() {
        std::fs::create_dir_all(&templates_dir)?;
    }

    let cache_dir = format!("{}/cache", &config_dir);
    if !std::path::Path::new(&cache_dir).exists() {
        std::fs::create_dir_all(&cache_dir)?;
    }

    let templates = std::fs::read_dir(&templates_dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension()?.to_str()? == "toml" {
                let contents = std::fs::read_to_string(&path).ok()?;
                let template: crate::config::Template = toml::from_str(&contents).ok()?;

                Some(template)
            } else {
                None
            }
        })
        .collect::<Vec<crate::config::Template>>();

    config.templates = Some(if let Some(config_templates) = config.templates {
        config_templates.into_iter().chain(templates).collect()
    } else {
        templates
    });

    Ok((args, config))
}

/// Handles the command prompt, adding support for reading from `stdin`, an argument, a file, or
/// a tuple of those three.
///
/// This function handles the following situations:
///
/// 1. Calling the binary with no arguments.
///
/// ```bash
/// llm-stream
/// ```
///
/// This will render in an empty value for both `prompt` and `stdin`.
///
/// 2. Calling the binary with `stdin` input.
///
/// ```bash
/// echo -n "Something" | llm-stream
/// ```
///
/// This will render in `prompt` to `Something` and `stdin` to be empty.
///
/// 3. Calling the binary with an argument.
///
/// ```bash
/// llm-stream "Something"
/// ```
///
/// This will render in `prompt` to `Something` and `stdin` to be empty.
///
/// 4. Calling the binary with an argument and `stdin` input.
///
/// ```bash
/// echo -n "Awesome" | llm-stream "Something"
/// ```
///
/// This will render `prompt` to be `Something, and `stdin` to be `Awesome`.
pub fn parse_args(mut args: Args, config: Config) -> Result<(Args, Config)> {
    let stdin = std::io::stdin();

    args.stdin = Some(if stdin.is_terminal() {
        "".to_string()
    } else {
        std::io::stdin()
            .lock()
            .lines()
            .collect::<std::result::Result<Vec<String>, std::io::Error>>()?
            .join("\n")
            .trim()
            .to_string()
    });

    if args.prompt.is_none() {
        args.prompt = Some(args.stdin.clone().unwrap_or_default().trim().to_string());
        args.stdin = None;
    }

    if let Some(preset) = args.preset.clone() {
        let p = config
            .presets
            .clone()
            .unwrap_or(vec![])
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
            if args.conversation.len() == 0
                || args.conversation.first().unwrap().role != ConversationRole::System
            {
                args.conversation.insert(
                    0,
                    ConversationMessage {
                        role: ConversationRole::System,
                        content: p.system.clone().unwrap_or_default(),
                    },
                );
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

    Ok((args, config))
}

/// Combines the existing arguments with the ones found on the cache file.
pub fn merge_args_and_cache(mut args: Args) -> Result<Args> {
    if args.from.is_none() && !args.from_last {
        return Ok(args);
    }

    let cache_dir = format!(
        "{}/cache",
        args.config_dir
            .clone()
            .unwrap_or("~/.config/llm-stream".to_string()),
    );

    if args.from_last {
        args.from = Some(
            std::fs::read_dir(&cache_dir)?
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    let path = entry.path();
                    if path.extension()?.to_str()? == "toml" {
                        Some(path)
                    } else {
                        None
                    }
                })
                .collect::<Vec<std::path::PathBuf>>()
                .iter()
                .max()
                .unwrap()
                .file_stem()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string(),
        );
    }

    let id = args.from.clone().expect("No cache file found");

    let cache_file = format!("{}/{}.toml", cache_dir, id);

    if !std::path::Path::new(&cache_file).exists() {
        return Err(Error::CacheNotFound);
    }

    let cache_args = toml::from_str::<Args>(&std::fs::read_to_string(&cache_file)?)?;

    args.conversation = cache_args.conversation;

    if args.api.is_none() {
        args.api = cache_args.api;
    }
    if args.model.is_none() {
        args.model = cache_args.model;
    }
    if args.api_version.is_none() {
        args.api_version = cache_args.api_version;
    }
    if args.api_env.is_none() {
        args.api_env = cache_args.api_env;
    }
    if args.api_key.is_none() {
        args.api_key = cache_args.api_key;
    }
    if args.temperature.is_none() {
        args.temperature = cache_args.temperature;
    }
    if args.max_tokens.is_none() {
        args.max_tokens = cache_args.max_tokens;
    }
    if args.quiet.is_none() {
        args.quiet = cache_args.quiet;
    }
    if args.language.is_none() {
        args.language = cache_args.language;
    }
    if args.theme.is_none() {
        args.theme = cache_args.theme;
    }
    if args.top_p.is_none() {
        args.top_p = cache_args.top_p;
    }
    if args.top_k.is_none() {
        args.top_k = cache_args.top_k;
    }

    Ok(args)
}

/// Builds the arguments struct based on a combination of the following inputs,
/// in this order.
///
/// 1. CLI options/Environment variables.
/// 2. Environment variable.
/// 3. Config preset and/or template options.
/// 4. Config file default options.
pub fn merge_args_and_config(mut args: Args, config: Config) -> Result<Args> {
    if let Some(ref template) = args.template {
        let t = config
            .templates
            .unwrap_or_default()
            .into_iter()
            .find(|t| t.name == *template);

        if t.is_none() {
            return Err(Error::TemplateNotFound);
        }

        let t = t.unwrap();

        let mut default_vars =
            if t.default_vars.is_none() || t.default_vars.as_ref().unwrap().is_null() {
                serde_json::json!("{}")
            } else {
                t.default_vars.unwrap()
            };

        let vars = if args.vars.is_none() || args.vars.as_ref().unwrap().is_null() {
            serde_json::json!("{}")
        } else {
            args.vars.take().unwrap()
        };

        merge(&mut default_vars, vars);

        let mut value = serde_json::json!({
            "prompt": args.prompt.clone().unwrap_or_default(),
            "stdin": args.stdin.clone().unwrap_or_default(),
            "suffix": args.suffix.clone().unwrap_or_default().to_string(),
            "language": args.language.clone(),
        });

        merge(&mut value, default_vars);

        let context = tera::Context::from_value(value)?;

        log::info!("context: {:#?}", &context);

        let mut tera = tera::Tera::default();

        if args.system.is_none() {
            if let Some(system) = t.system {
                tera.add_raw_template(SYSTEM_TEMPLATE, &system)?;
                args.system = Some(tera.render(SYSTEM_TEMPLATE, &context)?);
            }
        }

        if let Some(template) = t.template {
            tera.add_raw_template(PROMPT_TEMPLATE, &template)?;

            args.prompt = Some(tera.render(PROMPT_TEMPLATE, &context)?);
        }

        if let Some(conversation) = t.conversation {
            for message in conversation {
                tera.add_raw_template(CONTENT_TEMPLATE, &message.content)?;

                if message.role == ConversationRole::System && args.conversation.len() > 0 {
                    if args.conversation.first().unwrap().role != ConversationRole::System {
                        args.conversation.insert(
                            0,
                            ConversationMessage {
                                role: ConversationRole::System,
                                content: tera.render(CONTENT_TEMPLATE, &context)?,
                            },
                        );
                    } else {
                        continue;
                    }
                } else {
                    args.conversation.push(ConversationMessage {
                        role: message.role.clone(),
                        content: tera.render(CONTENT_TEMPLATE, &context)?,
                    });
                }
            }
        }
    } else if args.stdin.is_some() {
        args.prompt = Some(
            format!(
                "{}\n{}",
                args.stdin.clone().unwrap(),
                args.prompt.clone().unwrap_or_default()
            )
            .trim()
            .to_string(),
        );
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
    if args.conversation.len() == 0
        || args.conversation.first().unwrap().role != ConversationRole::System
    {
        args.conversation.insert(
            0,
            ConversationMessage {
                role: ConversationRole::System,
                content: config.system.clone().unwrap_or_default(),
            },
        );
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

    args.conversation.push(ConversationMessage {
        role: ConversationRole::User,
        content: args.prompt.clone().unwrap_or_default(),
    });

    if args.system.is_some() {
        if args.conversation.len() > 1
            && args.conversation.first().unwrap().role == ConversationRole::System
        {
            args.conversation[0].content = args.system.clone().unwrap();
        } else {
            args.conversation.insert(
                0,
                ConversationMessage {
                    role: ConversationRole::System,
                    content: args.system.clone().unwrap(),
                },
            );
        }
    };

    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Preset, Template};

    #[test]
    fn test_args_dont_change_on_empty_config() -> std::result::Result<(), Box<dyn std::error::Error>>
    {
        let args = Args::default();
        let config: Config = Config::default();

        let mut expected = args.clone();
        expected.conversation = vec![ConversationMessage::default()];

        let actual = merge_args_and_config(args, config)?;

        assert_eq!(
            expected, actual,
            "merge_args_and_config changed the default values"
        );

        Ok(())
    }

    #[test]
    fn test_args_override_config() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let mut args = Args::default();
        args.api = Some(Api::OpenAi);
        args.model = Some("gpt-4o".to_string());
        args.max_tokens = Some(100);
        args.min_tokens = Some(10);
        args.api_env = Some("OPENAI_API_KEY".to_string());
        args.api_version = Some("0.1.0".to_string());
        args.api_key = Some("123".to_string());
        args.api_base_url = Some("https://api.openai.com/v1".to_string());
        args.quiet = Some(true);
        args.language = Some("markdown".to_string());
        args.system = Some("Something Awesome".to_string());
        args.temperature = Some(0.5);
        args.top_p = Some(0.5);
        args.top_k = Some(50);

        let mut expected = args.clone();
        expected.conversation = vec![
            ConversationMessage {
                role: ConversationRole::System,
                content: "Something Awesome".to_string(),
            },
            ConversationMessage::default(),
        ];

        let mut config: Config = Config::default();
        config.api = Some(Api::Anthropic);
        config.model = Some("gpt-3".to_string());
        config.max_tokens = Some(200);
        config.min_tokens = Some(20);
        config.env = Some("ANTHROPIC_API_KEY".to_string());
        config.version = Some("0.2.0".to_string());
        config.key = Some("456".to_string());
        config.base_url = Some("https://api.anthropic.com/v1".to_string());
        config.quiet = Some(false);
        config.language = Some("html".to_string());
        config.system = Some("Something Awesome".to_string());
        config.temperature = Some(0.7);
        config.top_p = Some(0.7);
        config.top_k = Some(70);

        let actual = merge_args_and_config(args, config)?;

        assert_eq!(
            expected, actual,
            "merge_args_and_config changed the default values"
        );

        Ok(())
    }

    #[test]
    fn test_system_arg_over_config_preset() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let system = "param system";
        let preset_name = "preset_name";

        let mut args = Args::default();
        args.system = Some(system.to_string());
        args.preset = Some(preset_name.to_string());

        let mut expected = args.clone();
        expected.conversation = vec![
            ConversationMessage {
                role: ConversationRole::System,
                content: system.to_string(),
            },
            ConversationMessage::default(),
        ];

        let mut config: Config = Config::default();
        config.presets = Some(vec![Preset {
            name: preset_name.to_string(),
            system: Some("preset system".to_string()),
            ..Default::default()
        }]);

        let actual = merge_args_and_config(args, config)?;

        assert_eq!(
            expected.conversation, actual.conversation,
            "The system arg should overwrite the preset system"
        );

        Ok(())
    }

    #[test]
    fn test_prest_system_over_config_system() -> std::result::Result<(), Box<dyn std::error::Error>>
    {
        let system = "preset system";
        let config_system = "config system";
        let preset_name = "preset_name";

        let mut args = Args::default();
        args.preset = Some(preset_name.to_string());

        let mut expected = args.clone();
        expected.conversation = vec![
            ConversationMessage {
                role: ConversationRole::System,
                content: system.to_string(),
            },
            ConversationMessage::default(),
        ];

        let mut config: Config = Config::default();
        config.system = Some(config_system.to_string());
        config.presets = Some(vec![Preset {
            name: preset_name.to_string(),
            system: Some(system.to_string()),
            ..Default::default()
        }]);

        let actual = merge_args_and_config(args, config)?;

        assert_eq!(
            expected.conversation, actual.conversation,
            "The system arg should overwrite the preset system"
        );

        Ok(())
    }

    #[test]
    fn test_system_arg_over_system_template() -> std::result::Result<(), Box<dyn std::error::Error>>
    {
        let system = "param system";
        let template_name = "template_name";

        let mut args = Args::default();
        args.system = Some(system.to_string());
        args.template = Some(template_name.to_string());

        let mut expected = args.clone();
        expected.conversation = vec![
            ConversationMessage {
                role: ConversationRole::System,
                content: system.to_string(),
            },
            ConversationMessage::default(),
        ];

        let mut config: Config = Config::default();
        config.templates = Some(vec![Template {
            name: template_name.to_string(),
            description: Some("test".to_string()),
            system: Some("template system".to_string()),
            template: Some("".to_string()),
            ..Default::default()
        }]);

        let actual = merge_args_and_config(args, config)?;

        assert_eq!(
            expected.conversation, actual.conversation,
            "The system arg should overwrite the template system"
        );

        Ok(())
    }

    #[test]
    fn test_system_template_over_system_preset(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let system = "template system";
        let template_name = "template_name";
        let preset_name = "preset_name";

        let mut args = Args::default();
        args.template = Some(template_name.to_string());
        args.preset = Some(preset_name.to_string());

        let mut expected = args.clone();
        expected.conversation = vec![
            ConversationMessage {
                role: ConversationRole::System,
                content: system.to_string(),
            },
            ConversationMessage::default(),
        ];

        let mut config: Config = Config::default();
        config.templates = Some(vec![Template {
            name: template_name.to_string(),
            description: Some("test".to_string()),
            system: Some(system.to_string()),
            template: Some("".to_string()),
            ..Default::default()
        }]);
        config.presets = Some(vec![Preset {
            name: preset_name.to_string(),
            system: Some("preset system".to_string()),
            ..Default::default()
        }]);

        let actual = merge_args_and_config(args, config)?;

        assert_eq!(
            expected.conversation, actual.conversation,
            "The template system should overwrite the preset system"
        );

        Ok(())
    }

    #[test]
    fn test_template_system_should_not_be_duplicated(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let system = "template system";
        let template_name = "template_name";

        let mut args = Args::default();
        args.template = Some(template_name.to_string());

        let mut expected = args.clone();
        expected.conversation = vec![
            ConversationMessage {
                role: ConversationRole::System,
                content: system.to_string(),
            },
            ConversationMessage::default(),
        ];

        let mut config: Config = Config::default();
        config.templates = Some(vec![Template {
            name: template_name.to_string(),
            description: Some("test".to_string()),
            system: Some(system.to_string()),
            template: Some("".to_string()),
            ..Default::default()
        }]);

        let actual = merge_args_and_config(args, config)?;

        assert_eq!(
            expected.conversation, actual.conversation,
            "The system arg should overwrite the template system"
        );

        Ok(())
    }

    #[test]
    fn test_resulting_conversation_has_single_system_message_at_index_zero(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let system_option = "system option";
        let system_conversation = "system conversation";
        let mut args = Args::default();
        args.system = Some(system_option.to_string());

        let mut expected = args.clone();
        expected.conversation = vec![
            ConversationMessage {
                role: ConversationRole::System,
                content: system_option.to_string(),
            },
            ConversationMessage::default(),
        ];

        args.conversation = vec![ConversationMessage {
            role: ConversationRole::System,
            content: system_conversation.to_string(),
        }];

        let config: Config = Config::default();
        let actual = merge_args_and_config(args, config)?;

        assert_eq!(
            expected.conversation, actual.conversation,
            "There should be a single `system` message"
        );

        Ok(())
    }
}

/// Prints the given conversation to stdout
pub fn show(args: Args, text: &str) -> Result<()> {
    let language = "toml";
    let theme = Some(args.theme.clone().unwrap_or("ansi".to_string()));

    if args.no_color {
        println!("{}", text);
    } else {
        let output = crate::printer::CustomPrinter::new(&language, theme.as_deref())?
            .input_from_bytes(&text.as_bytes())
            .print()?;

        println!("{}", output);
        std::io::stdout().flush()?;
    }

    Ok(())
}
