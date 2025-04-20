use clap::Parser;
use futures::stream::TryStreamExt;
use llm_stream::jina;
use std::io::Write;

mod args;
mod error;
mod prelude;
mod printer;

use crate::args::Args;
use crate::prelude::*;

const DEFAULT_ENV: &str = "JINA_API_KEY";

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let mut args = Args::parse();

    log::info!("args: {:#?}", args);

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

    let auth = jina::Auth::new(key);
    let client = jina::Client::new(auth, args.api_base_url.take());

    let theme = args.theme.take().unwrap_or("tokyonight-storm".to_string());
    let quiet = args.quiet;

    let options: jina::Options = args.into();

    let mut stream = client.delta(&options)?;

    let mut previous_output = String::new();
    let mut accumulated_content_bytes: Vec<u8> = Vec::new();

    let is_terminal = atty::is(atty::Stream::Stdout);

    let mut sp = if !quiet && is_terminal {
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
            Ok(Some(mut text)) => {
                if is_terminal && sp.is_some() {
                    // TODO: Find a better way to clean the spinner from the terminal.
                    sp.take().unwrap().stop();
                    std::io::stdout().flush()?;
                    crossterm::execute!(std::io::stdout(), crossterm::cursor::MoveToColumn(0))?;
                    print!("                      ");
                    crossterm::execute!(std::io::stdout(), crossterm::cursor::MoveToColumn(0))?;
                }

                // Avoid coloring the output if not working as a terminal.
                if !is_terminal {
                    // V2 of ReaderLM has the annoying feature of sending the entire file with each
                    // event instead of the parsed version. Thus we need to remove the accumulated
                    // text before printing.
                    if options.use_readerlm_v2.unwrap_or(false) {
                        let tmp = text.clone();
                        text = text.replace(
                            &String::from_utf8_lossy(&accumulated_content_bytes).to_string(),
                            "",
                        );
                        accumulated_content_bytes = tmp.into();
                    } else {
                        accumulated_content_bytes.extend_from_slice(text.as_bytes());
                    }
                    print!("{}", text);
                    std::io::stdout().flush()?;
                    continue;
                }

                if options.use_readerlm_v2.unwrap_or(false) {
                    accumulated_content_bytes = text.as_bytes().to_vec();
                } else {
                    accumulated_content_bytes.extend_from_slice(text.as_bytes());
                }

                let output = crate::printer::CustomPrinter::new("markdown", Some(&theme))?
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

    Ok(())
}
