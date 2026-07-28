use cli_table::{format::Justify, print_stdout, Color, ColorChoice, Row, Table, Title, WithTitle};
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
    let mut accumulated_text = String::new();
    let mut previous_output = String::new();

    let is_terminal = atty::is(atty::Stream::Stdout);

    let mut sp = if args.quiet.is_none() || (args.quiet == Some(false) && is_terminal) {
        Some(spinners::Spinner::new(
            spinners::Spinners::OrangeBluePulse,
            "Loading...".into(),
        ))
    } else {
        None
    };

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

                accumulated_text.push_str(&text);
                let length = previous_output.lines().count();

                let output = crate::printer::highlight_markdown(&accumulated_text);

                let unprinted_lines = output
                    .lines()
                    .skip(if length == 0 { 0 } else { length - 1 })
                    .collect::<Vec<_>>()
                    .join("\n");

                crossterm::execute!(std::io::stdout(), crossterm::cursor::MoveToColumn(0))?;
                print!("{unprinted_lines}");
                std::io::stdout().flush()?;

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
            content: accumulated_text.clone(),
        });

        // log the `args.conversation` to `stdout`.
        log::info!("Conversation: {:#?}", &args.conversation);

        let config_dir = args
            .config_dir
            .clone()
            .unwrap_or("~/.config/llm-stream".to_string());
        let cache_file = format!("{}/cache/{}.toml", config_dir, id);

        if let Some(max_history_size) = args.max_history_size {
            // If there's a message in the conversation of role `System` take it and store it in
            // a variable.
            let system_message = args
                .conversation
                .iter()
                .find(|m| m.role == ConversationRole::System)
                .cloned();

            if system_message.is_some() {
                // Remove the `System` message from the conversation.
                args.conversation
                    .retain(|m| m.role != ConversationRole::System);
            }
            // Keep only the last `max_history_size` elements of args.conversation.
            args.conversation = args
                .conversation
                .into_iter()
                .rev()
                // Convert to usize
                .take(max_history_size.try_into().unwrap())
                .collect::<Vec<ConversationMessage>>()
                .into_iter()
                .rev()
                .collect();

            // Add back the system message if it exists as the first message.
            if let Some(message) = system_message {
                args.conversation.insert(0, message);
            }
        }

        let cache_toml = toml::to_string(&args)?;

        log::info!("Cache file: {}", &cache_file);
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
            if args.reasoning_effort.is_none() {
                args.reasoning_effort = p.reasoning_effort;
            }
        }
    };

    Ok((args, config))
}

fn get_latest_toml_file(cache_dir: &str) -> Result<Option<String>> {
    let cache_files = std::fs::read_dir(cache_dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension()?.to_str()? == "toml" {
                Some(path)
            } else {
                None
            }
        })
        .collect::<Vec<std::path::PathBuf>>();

    let latest_file = cache_files.iter().max_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });

    Ok(latest_file.and_then(|path| {
        path.file_stem()
            .and_then(|stem| stem.to_str().map(String::from))
    }))
}

/// Combines the existing arguments with the ones found on the cache file.
pub fn merge_args_and_cache(mut args: Args) -> Result<Args> {
    if args.from.is_none() && !args.from_last {
        log::info!("No merging of cached args necessary");
        return Ok(args);
    }

    log::info!("Merging args with cached args");

    let cache_dir = format!(
        "{}/cache",
        args.config_dir
            .clone()
            .unwrap_or("~/.config/llm-stream".to_string()),
    );

    if args.from_last {
        args.from = get_latest_toml_file(&cache_dir)?
    }

    let id = args.from.clone().expect("No cache file found");

    let cache_file = format!("{}/{}.toml", cache_dir, id);

    if !std::path::Path::new(&cache_file).exists() {
        log::info!("Cache file not found: {}", &cache_file);
        return Ok(args);
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
    if args.top_p.is_none() {
        args.top_p = cache_args.top_p;
    }
    if args.top_k.is_none() {
        args.top_k = cache_args.top_k;
    }
    if args.reasoning_effort.is_none() {
        args.reasoning_effort = cache_args.reasoning_effort;
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

                if message.role == ConversationRole::System && args.conversation.is_empty() {
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
                        role: message.role,
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
    // `reasoning_effort` describes the request, not the provider, so it sits
    // with `temperature`/`top_p` rather than inside the provider-scoped block
    // below. Only the `chatgpt` provider reads it; for everyone else it is an
    // inert string, so inheriting it across providers costs nothing and
    // surprises no one.
    if args.reasoning_effort.is_none() {
        args.reasoning_effort = config.reasoning_effort;
    }
    if args.conversation.is_empty()
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
    // `base_url`, `env`, `key`, `version`, and `model` describe one specific
    // provider. Inheriting them when the operator selected a different one
    // sends the request to the wrong endpoint — and, for a provider whose
    // credential is a bearer token rather than an API key, sends that token to
    // a host with no business receiving it. The remaining config defaults
    // (system, max_tokens, temperature, top_p, top_k, quiet) describe the
    // request rather than the provider, so they still apply to everyone.
    //
    // `config.api` is `Some` for any config file loaded from disk (serde fills
    // it via `default_api`), so the `is_none` arm only covers a `Config`
    // constructed in code — where preserving the old behaviour is safest.
    let config_describes_this_api =
        args.api.is_none() || config.api.is_none() || args.api == config.api;

    if config_describes_this_api {
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
    }
    if args.quiet.is_none() {
        args.quiet = config.quiet;
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
    fn test_args_override_config() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let args = Args {
            api: Some(Api::OpenAi),
            model: Some("gpt-4o".to_string()),
            max_tokens: Some(100),
            min_tokens: Some(10),
            api_env: Some("OPENAI_API_KEY".to_string()),
            api_version: Some("0.1.0".to_string()),
            api_key: Some("123".to_string()),
            api_base_url: Some("https://api.openai.com/v1".to_string()),
            quiet: Some(true),
            system: Some("Something Awesome".to_string()),
            temperature: Some(0.5),
            top_p: Some(0.5),
            top_k: Some(50),
            ..Default::default()
        };

        let mut expected = args.clone();
        expected.conversation = vec![
            ConversationMessage {
                role: ConversationRole::System,
                content: "Something Awesome".to_string(),
            },
            ConversationMessage::default(),
        ];

        let config = Config {
            api: Some(Api::Anthropic),
            model: Some("gpt-3".to_string()),
            max_tokens: Some(200),
            min_tokens: Some(20),
            env: Some("ANTHROPIC_API_KEY".to_string()),
            version: Some("0.2.0".to_string()),
            key: Some("456".to_string()),
            base_url: Some("https://api.anthropic.com/v1".to_string()),
            quiet: Some(false),
            system: Some("Something Awesome".to_string()),
            temperature: Some(0.7),
            top_p: Some(0.7),
            top_k: Some(70),
            ..Default::default()
        };

        let actual = merge_args_and_config(args, config)?;

        assert_eq!(
            expected, actual,
            "merge_args_and_config changed the default values"
        );

        Ok(())
    }

    #[test]
    fn config_provider_fields_do_not_leak_across_providers(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        // The real-world shape of this bug: a config whose top-level defaults
        // describe an OpenAI-compatible endpoint, and an operator who asked
        // for a different provider entirely.
        let args = Args {
            api: Some(Api::ChatGpt),
            ..Default::default()
        };

        let config = Config {
            api: Some(Api::OpenAi),
            base_url: Some("https://api.githubcopilot.com".to_string()),
            env: Some("COPILOT_API_KEY".to_string()),
            key: Some("secret".to_string()),
            version: Some("0.1.0".to_string()),
            model: Some("gpt-4o".to_string()),
            ..Default::default()
        };

        let actual = merge_args_and_config(args, config)?;

        assert_eq!(
            actual.api_base_url, None,
            "base_url leaked across providers"
        );
        assert_eq!(actual.model, None, "model leaked across providers");
        assert_eq!(actual.api_env, None, "env leaked across providers");
        assert_eq!(actual.api_key, None, "key leaked across providers");
        assert_eq!(actual.api_version, None, "version leaked across providers");

        Ok(())
    }

    #[test]
    fn config_provider_fields_apply_when_the_api_matches(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let args = Args {
            api: Some(Api::OpenAi),
            ..Default::default()
        };

        let config = Config {
            api: Some(Api::OpenAi),
            base_url: Some("https://api.githubcopilot.com".to_string()),
            model: Some("gpt-4o".to_string()),
            ..Default::default()
        };

        let actual = merge_args_and_config(args, config)?;

        assert_eq!(
            actual.api_base_url.as_deref(),
            Some("https://api.githubcopilot.com")
        );
        assert_eq!(actual.model.as_deref(), Some("gpt-4o"));

        Ok(())
    }

    #[test]
    fn config_provider_fields_apply_when_no_api_was_selected(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        // No `--api` flag: the config's own `api` is what gets selected a few
        // lines later, so its companion fields are the right ones to inherit.
        let args = Args::default();

        let config = Config {
            api: Some(Api::OpenAi),
            base_url: Some("https://api.githubcopilot.com".to_string()),
            model: Some("gpt-4o".to_string()),
            ..Default::default()
        };

        let actual = merge_args_and_config(args, config)?;

        assert_eq!(actual.api, Some(Api::OpenAi));
        assert_eq!(actual.model.as_deref(), Some("gpt-4o"));

        Ok(())
    }

    #[test]
    fn explicit_arguments_still_win_over_a_matching_config(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let args = Args {
            api: Some(Api::OpenAi),
            model: Some("o3-mini".to_string()),
            api_base_url: Some("https://example.test/v1".to_string()),
            ..Default::default()
        };

        let config = Config {
            api: Some(Api::OpenAi),
            base_url: Some("https://api.githubcopilot.com".to_string()),
            model: Some("gpt-4o".to_string()),
            ..Default::default()
        };

        let actual = merge_args_and_config(args, config)?;

        assert_eq!(actual.model.as_deref(), Some("o3-mini"));
        assert_eq!(
            actual.api_base_url.as_deref(),
            Some("https://example.test/v1")
        );

        Ok(())
    }

    #[test]
    fn config_reasoning_effort_applies_across_providers(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        // Unlike `base_url`/`key`/`model`, this one is *meant* to cross
        // provider lines: it describes the request, and only one provider
        // reads it. See the comment in `merge_args_and_config`.
        let args = Args {
            api: Some(Api::ChatGpt),
            ..Default::default()
        };

        let config = Config {
            api: Some(Api::OpenAi),
            reasoning_effort: Some("high".to_string()),
            ..Default::default()
        };

        let actual = merge_args_and_config(args, config)?;

        assert_eq!(actual.reasoning_effort.as_deref(), Some("high"));

        Ok(())
    }

    #[test]
    fn explicit_reasoning_effort_beats_the_config(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let args = Args {
            api: Some(Api::ChatGpt),
            reasoning_effort: Some("xhigh".to_string()),
            ..Default::default()
        };

        let config = Config {
            api: Some(Api::ChatGpt),
            reasoning_effort: Some("low".to_string()),
            ..Default::default()
        };

        let actual = merge_args_and_config(args, config)?;

        assert_eq!(actual.reasoning_effort.as_deref(), Some("xhigh"));

        Ok(())
    }

    #[test]
    fn test_system_arg_over_config_preset() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let system = "param system";
        let preset_name = "preset_name";

        let args = Args {
            system: Some(system.to_string()),
            preset: Some(preset_name.to_string()),
            ..Default::default()
        };

        let mut expected = args.clone();
        expected.conversation = vec![
            ConversationMessage {
                role: ConversationRole::System,
                content: system.to_string(),
            },
            ConversationMessage::default(),
        ];

        let config: Config = Config {
            presets: Some(vec![Preset {
                name: preset_name.to_string(),
                system: Some("preset system".to_string()),
                ..Default::default()
            }]),
            ..Default::default()
        };

        let actual = merge_args_and_config(args, config)?;

        assert_eq!(
            expected.conversation, actual.conversation,
            "The system arg should overwrite the preset system"
        );

        Ok(())
    }

    #[test]
    fn test_preset_system_over_config_system() -> std::result::Result<(), Box<dyn std::error::Error>>
    {
        let system = "preset system";
        let config_system = "config system";
        let preset_name = "preset_name";

        let args = Args {
            preset: Some(preset_name.to_string()),
            ..Default::default()
        };

        let mut expected = args.clone();
        expected.conversation = vec![
            ConversationMessage {
                role: ConversationRole::System,
                content: system.to_string(),
            },
            ConversationMessage::default(),
        ];

        let config: Config = Config {
            system: Some(config_system.to_string()),
            presets: Some(vec![Preset {
                name: preset_name.to_string(),
                system: Some(system.to_string()),
                ..Default::default()
            }]),
            ..Default::default()
        };

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

        let args = Args {
            system: Some(system.to_string()),
            template: Some(template_name.to_string()),
            ..Default::default()
        };

        let mut expected = args.clone();
        expected.conversation = vec![
            ConversationMessage {
                role: ConversationRole::System,
                content: system.to_string(),
            },
            ConversationMessage::default(),
        ];

        let config: Config = Config {
            templates: Some(vec![Template {
                name: template_name.to_string(),
                description: Some("test".to_string()),
                system: Some("template system".to_string()),
                template: Some("".to_string()),
                ..Default::default()
            }]),
            ..Default::default()
        };

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

        let args = Args {
            template: Some(template_name.to_string()),
            preset: Some(preset_name.to_string()),
            ..Default::default()
        };

        let mut expected = args.clone();
        expected.conversation = vec![
            ConversationMessage {
                role: ConversationRole::System,
                content: system.to_string(),
            },
            ConversationMessage::default(),
        ];

        let config: Config = Config {
            templates: Some(vec![Template {
                name: template_name.to_string(),
                description: Some("test".to_string()),
                system: Some(system.to_string()),
                template: Some("".to_string()),
                ..Default::default()
            }]),
            presets: Some(vec![Preset {
                name: preset_name.to_string(),
                system: Some("preset system".to_string()),
                ..Default::default()
            }]),
            ..Default::default()
        };

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

        let args = Args {
            template: Some(template_name.to_string()),
            ..Default::default()
        };

        let mut expected = args.clone();
        expected.conversation = vec![
            ConversationMessage {
                role: ConversationRole::System,
                content: system.to_string(),
            },
            ConversationMessage::default(),
        ];

        let config: Config = Config {
            templates: Some(vec![Template {
                name: template_name.to_string(),
                description: Some("test".to_string()),
                system: Some(system.to_string()),
                template: Some("".to_string()),
                ..Default::default()
            }]),
            ..Default::default()
        };

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
        let mut args = Args {
            system: Some(system_option.to_string()),
            ..Default::default()
        };

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

#[derive(Table)]
struct ConversationLine {
    #[table(title = "ID", justify = "Justify::Left", color = "Color::Cyan")]
    id: String,
    #[table(title = "Parent", justify = "Justify::Left", color = "Color::Magenta")]
    parent: String,
    #[table(title = "Title", justify = "Justify::Left")]
    title: String,
    #[table(title = "Description", justify = "Justify::Left")]
    description: String,
}

impl ConversationLine {
    pub fn new(
        id: String,
        parent: Option<String>,
        title: Option<String>,
        description: Option<String>,
    ) -> Self {
        Self {
            id,
            parent: parent.unwrap_or_default(),
            title: title.unwrap_or_default(),
            description: description.unwrap_or_default(),
        }
    }
}

#[derive(Table)]
pub struct PresetLine {
    #[table(title = "Name", justify = "Justify::Left", color = "Color::Cyan")]
    pub name: String,
    #[table(title = "Api", justify = "Justify::Left", color = "Color::Magenta")]
    pub api: String,
    #[table(title = "BaseUrl", justify = "Justify::Left")]
    pub base_url: String,
    #[table(title = "Model", justify = "Justify::Left")]
    pub model: String,
}

impl From<crate::config::Preset> for PresetLine {
    fn from(preset: crate::config::Preset) -> Self {
        PresetLine {
            name: preset.name,
            api: format!("{:?}", preset.api),
            base_url: preset.base_url.unwrap_or_default(),
            model: preset.model.unwrap_or_default(),
        }
    }
}

#[derive(Table)]
pub struct TemplateLine {
    #[table(title = "Name", justify = "Justify::Left", color = "Color::Cyan")]
    pub name: String,
    #[table(title = "Description", justify = "Justify::Left")]
    pub description: String,
}

impl From<crate::config::Template> for TemplateLine {
    fn from(template: crate::config::Template) -> Self {
        TemplateLine {
            name: template.name,
            description: template.description.unwrap_or_default(),
        }
    }
}

fn get_sorted_cache_files(cache_dir: &str) -> Result<Vec<std::path::PathBuf>> {
    let mut cache_files = std::fs::read_dir(cache_dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension()?.to_str()? == "toml" {
                Some(path)
            } else {
                None
            }
        })
        .collect::<Vec<std::path::PathBuf>>();

    cache_files.sort_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });

    Ok(cache_files)
}

/// Prints a list of rows as the table every listing command in this CLI shares.
///
/// Titles and colour are suppressed when stdout is not a terminal: piping
/// `--list` into another program should hand it data, not a header row and
/// escape codes. `--no-color` suppresses colour on a terminal too.
///
/// The `for<'a> &'a T: Row` bound is not decoration. `cli_table::WithTitle` is
/// implemented for `&Vec<T>` and `cli_table::Table` for `Vec<T>`; the call
/// sites relied on auto-ref to paper over that, and a generic function has to
/// name both.
pub fn print_table<T>(lines: Vec<T>, no_color: bool) -> Result<()>
where
    T: Row + Title + 'static,
    for<'a> &'a T: Row,
{
    let is_terminal: bool = atty::is(atty::Stream::Stdout);

    let table = if is_terminal {
        lines.with_title()
    } else {
        lines.table()
    };

    let vert_line = cli_table::format::VerticalLine::new(' ');
    let horz_line = cli_table::format::HorizontalLine::new(' ', ' ', ' ', ' ');
    let border = cli_table::format::Border::builder()
        .top(horz_line)
        .bottom(horz_line)
        .left(vert_line)
        .right(vert_line)
        .build();
    let separator = cli_table::format::Separator::builder()
        .row(None)
        .column(None)
        .title(None)
        .build();

    print_stdout(table.separator(separator).border(border).color_choice(
        if no_color || !is_terminal {
            ColorChoice::Never
        } else {
            ColorChoice::Always
        },
    ))?;

    Ok(())
}

/// Prints a list of existing conversations
pub fn list(args: Args) -> Result<()> {
    let config_dir = args.config_dir.clone().expect("can't find cache directory");
    let cache_dir = format!("{}/cache", &config_dir);

    // Get a list of all the `toml` files inside the `cache_dir`
    let cache_files = get_sorted_cache_files(&cache_dir)?;

    let lines = cache_files
        .iter()
        .map(|path| {
            let id = path.file_stem().unwrap().to_str().unwrap();
            let cache_toml = std::fs::read_to_string(path).unwrap();
            let args: Args = toml::from_str(&cache_toml).unwrap();
            let description = Some(
                if let Some(description) = args.description {
                    description
                } else if args.conversation.is_empty() {
                    "Empty".to_string()
                } else {
                    // Get the first message in `args.conversation` whose `role` is
                    // `ConversationRole::Assistant.
                    let message = args
                        .conversation
                        .iter()
                        .rev()
                        .find(|m| m.role == ConversationRole::Assistant)
                        .unwrap_or(args.conversation.first().expect("No messages"));
                    // Get the first non-empty line inside `message.content` or `Empty` if there
                    // are none
                    message
                        .content
                        .clone()
                        .split("\n")
                        .filter(|s| !s.is_empty() && !s.starts_with("```"))
                        .collect::<Vec<_>>()
                        .first()
                        .unwrap_or(&"Empty assistant message")
                        .to_string()
                }
                .chars()
                .take(120)
                .collect::<String>()
                .to_string(),
            );
            ConversationLine::new(id.to_string(), args.parent, args.title, description)
        })
        .collect::<Vec<ConversationLine>>();

    print_table(lines, args.no_color)
}

/// Prints the given conversation to stdout
pub fn show(args: Args) -> Result<()> {
    if args.last {
        println!("{}", args.conversation.last().unwrap().content);
        return Ok(());
    }

    // Read the cache file from `args.config_dir/args.from`
    let cache_file = format!(
        "{}/cache/{}.toml",
        args.config_dir.clone().expect("can't find cache directory"),
        args.from
            .clone()
            .expect("--from or --from-last needs to be defined when run with --show")
    );

    // Read the contents of cache_file
    let text = std::fs::read_to_string(&cache_file)?;

    if args.no_color {
        println!("{}", text);
    } else {
        let output = crate::printer::highlight_markdown(&text);
        println!("{}", output);
        std::io::stdout().flush()?;
    }

    Ok(())
}

/// Prints the presets table to `stdout`.
pub fn presets(lines: Vec<PresetLine>, no_color: bool) -> Result<()> {
    print_table(lines, no_color)
}

/// Prints the templates table to `stdout`.
pub fn templates(lines: Vec<TemplateLine>, no_color: bool) -> Result<()> {
    print_table(lines, no_color)
}

/// Whether the `---` rule between the reasoning summary and the answer is still
/// owed to the operator.
///
/// This replaces a single `bool`, which had to mean both "no reasoning has
/// arrived" and "a separator is owed" — and defaulted to the second, so a
/// stream with no reasoning at all printed a rule that separated nothing. A
/// stream *can* carry zero reasoning events even when one was requested: the
/// model decides whether to summarise. Measured live, `effort=low,
/// summary=auto` produced none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Separator {
    /// No reasoning has arrived. There is nothing to separate.
    #[default]
    NotNeeded,
    /// Reasoning arrived and the answer has not started. Print on first delta.
    Owed,
    /// Already resolved. Never print again.
    Printed,
}

/// A reasoning delta arrived.
#[must_use]
pub const fn on_reasoning(state: Separator) -> Separator {
    match state {
        // Reasoning that trails the answer does not re-open the summary; the
        // rule would land in the middle of a sentence.
        Separator::Printed => Separator::Printed,
        Separator::NotNeeded | Separator::Owed => Separator::Owed,
    }
}

/// An answer delta arrived. Returns the next state and whether to print the rule
/// right now.
#[must_use]
pub const fn on_answer(state: Separator) -> (Separator, bool) {
    match state {
        Separator::Owed => (Separator::Printed, true),
        // Once the answer has begun the question is settled either way, so a
        // later reasoning event cannot reopen it.
        Separator::NotNeeded | Separator::Printed => (Separator::Printed, false),
    }
}

/// Handles a reasoning stream of text from the LLM and prints it to the terminal.
pub async fn handle_reason_stream(
    mut stream: impl Stream<Item = std::result::Result<llm_stream::event::ReasonEvent, llm_stream::error::Error>>
        + std::marker::Unpin,
    mut args: Args,
) -> Result<()> {
    let mut accumulated_text = String::new();
    let mut previous_output = String::new();

    let is_terminal = atty::is(atty::Stream::Stdout);

    let mut sp = if args.quiet.is_none() || (args.quiet == Some(false) && is_terminal) {
        Some(spinners::Spinner::new(
            spinners::Spinners::OrangeBluePulse,
            "Loading...".into(),
        ))
    } else {
        None
    };

    let mut separator = Separator::default();

    loop {
        let result = stream.try_next().await;

        match result {
            Ok(Some(event)) => match event {
                llm_stream::event::ReasonEvent::Delta(text) => {
                    if is_terminal && sp.is_some() {
                        // TODO: Find a better way to clean the spinner from the terminal.
                        sp.take().unwrap().stop();
                        std::io::stdout().flush()?;
                        crossterm::execute!(std::io::stdout(), crossterm::cursor::MoveToColumn(0))?;
                        print!("                      ");
                        crossterm::execute!(std::io::stdout(), crossterm::cursor::MoveToColumn(0))?;
                    }

                    let (next, print_separator) = on_answer(separator);
                    separator = next;
                    if print_separator {
                        // stderr, beside the reasoning it separates — stdout
                        // stays the answer and only the answer, so piping works.
                        eprint!("\n\n---\n\n");
                        std::io::stderr().flush()?;
                    }

                    if !is_terminal {
                        // If not a terminal, print each instance of `text` directly to `stdout`
                        print!("{}", text);
                        std::io::stdout().flush()?;
                        continue;
                    }

                    accumulated_text.push_str(&text);
                    let length = previous_output.lines().count();

                    let output = crate::printer::highlight_markdown(&accumulated_text);

                    let unprinted_lines = output
                        .lines()
                        .skip(if length == 0 { 0 } else { length - 1 })
                        .collect::<Vec<_>>()
                        .join("\n");

                    crossterm::execute!(std::io::stdout(), crossterm::cursor::MoveToColumn(0))?;
                    print!("{unprinted_lines}");
                    std::io::stdout().flush()?;

                    // Update the previous output
                    previous_output = output;
                }
                llm_stream::event::ReasonEvent::Reasoning(text) => {
                    if is_terminal && sp.is_some() {
                        // TODO: Find a better way to clean the spinner from the terminal.
                        sp.take().unwrap().stop();
                        std::io::stdout().flush()?;
                        crossterm::execute!(std::io::stdout(), crossterm::cursor::MoveToColumn(0))?;
                        print!("                      ");
                        crossterm::execute!(std::io::stdout(), crossterm::cursor::MoveToColumn(0))?;
                    }
                    separator = on_reasoning(separator);
                    eprint!("{}", text);
                }
                _ => log::debug!("{}", event),
            },
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
            content: accumulated_text.clone(),
        });

        // log the `args.conversation` to `stdout`.
        log::info!("Conversation: {:#?}", &args.conversation);

        let config_dir = args
            .config_dir
            .clone()
            .unwrap_or("~/.config/llm-stream".to_string());
        let cache_file = format!("{}/cache/{}.toml", config_dir, id);

        if let Some(max_history_size) = args.max_history_size {
            // If there's a message in the conversation of role `System` take it and store it in
            // a variable.
            let system_message = args
                .conversation
                .iter()
                .find(|m| m.role == ConversationRole::System)
                .cloned();

            if system_message.is_some() {
                // Remove the `System` message from the conversation.
                args.conversation
                    .retain(|m| m.role != ConversationRole::System);
            }
            // Keep only the last `max_history_size` elements of args.conversation.
            args.conversation = args
                .conversation
                .into_iter()
                .rev()
                // Convert to usize
                .take(max_history_size.try_into().unwrap())
                .collect::<Vec<ConversationMessage>>()
                .into_iter()
                .rev()
                .collect();

            // Add back the system message if it exists as the first message.
            if let Some(message) = system_message {
                args.conversation.insert(0, message);
            }
        }

        let cache_toml = toml::to_string(&args)?;

        log::info!("Cache file: {}", &cache_file);
        std::fs::write(&cache_file, cache_toml)?;

        eprintln!("\n\nCache file: {}", &cache_file);
    }

    Ok(())
}

#[cfg(test)]
mod separator_tests {
    use super::{on_answer, on_reasoning, Separator};

    #[test]
    fn an_answer_with_no_reasoning_prints_no_rule() {
        // The regression. Before the fix this printed unconditionally.
        let (state, print) = on_answer(Separator::default());
        assert!(!print, "printed a rule that separated nothing");
        assert_eq!(state, Separator::Printed);
    }

    #[test]
    fn reasoning_then_an_answer_prints_exactly_one_rule() {
        let state = on_reasoning(on_reasoning(Separator::default()));
        assert_eq!(state, Separator::Owed);

        let (state, print) = on_answer(state);
        assert!(print, "the summary ran straight into the answer");

        let (_, print_again) = on_answer(state);
        assert!(!print_again, "printed a second rule mid-answer");
    }

    #[test]
    fn reasoning_after_the_answer_started_does_not_reopen_the_rule() {
        let (state, _) = on_answer(Separator::default());
        let state = on_reasoning(state);
        let (_, print) = on_answer(state);
        assert!(!print, "a trailing reasoning event cut the answer in half");
    }

    #[test]
    fn a_stream_that_is_only_reasoning_leaves_the_rule_owed() {
        // Owed, never printed: there is no answer to separate the summary from,
        // so the stream ends with no rule. That is correct.
        assert_eq!(on_reasoning(Separator::default()), Separator::Owed);
    }
}
