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
        return;
    }

    *a = b;
}

/// Reads the configuration file. If it or the config directory doesn't exist, they'll be created.
pub fn build_config(mut args: Args) -> Result<(Args, Config)> {
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
pub fn parse_prompt(mut args: Args) -> Result<Args> {
    let stdin = std::io::stdin();

    args.stdin = Some(if stdin.is_terminal() {
        "".to_string()
    } else {
        std::io::stdin()
            .lock()
            .lines()
            .collect::<std::result::Result<Vec<String>, std::io::Error>>()?
            .join("\n")
    });

    if args.prompt.is_none() {
        args.prompt = args.stdin.clone();
        args.stdin = None;
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
    let prompt = args.prompt.clone();
    let stdin = args.stdin.clone();

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

        let suffix = args.suffix.clone().unwrap_or_default().to_string();
        let language = args.language.clone();

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
            "prompt": prompt,
            "stdin": stdin,
            "suffix": suffix,
            "language": language,
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

        tera.add_raw_template(PROMPT_TEMPLATE, t.template.as_ref())?;

        tera.render(PROMPT_TEMPLATE, &context)?
    } else if !stdin.is_none() {
        format!("{}\n{}", &stdin.unwrap(), &prompt.unwrap_or_default())
    } else {
        prompt.unwrap_or_default()
    };

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
            template: "".to_string(),
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
            template: "".to_string(),
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
            template: "".to_string(),
            ..Default::default()
        }]);

        let actual = merge_args_and_config(args, config)?;

        assert_eq!(
            expected.conversation, actual.conversation,
            "The system arg should overwrite the template system"
        );

        Ok(())
    }
}
