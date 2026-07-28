#[derive(thiserror::Error)]
pub enum Error {
    #[error("utf8 error")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("io error")]
    Io(#[from] std::io::Error),
    #[error("unable to get value from environment variable")]
    EnvVar(#[from] std::env::VarError),
    #[error("invalid api")]
    InvalidAPI,
    #[error("unable to coherce to u32")]
    TryFrom(#[from] std::num::TryFromIntError),
    #[error("api not specified")]
    ApiNotSpecified,
    #[error("cache not found")]
    CacheNotFound,
    #[error("config file error")]
    ConfigFile(#[from] config_file::ConfigFileError),
    #[error("infallible error")]
    Infallible(#[from] std::convert::Infallible),
    #[error("template not found")]
    TemplateNotFound,
    #[error("tera error")]
    Tera(#[from] tera::Error),
    #[error("toml deserialization error")]
    TomlDe(#[from] toml::de::Error),
    #[error("toml serialization error")]
    TomlSer(#[from] toml::ser::Error),
    #[error("json error")]
    Json(#[from] serde_json::Error),
    #[error("file or stdin error")]
    Stdin(#[from] clap_stdin::StdinError),
    #[error("unable to stream the api")]
    EsStream(#[from] llm_stream::error::Error),
    #[error("{0}")]
    Auth(String),
    #[error("http error: {0}")]
    Http(String),
}

pub(crate) fn format_error(
    e: &impl std::error::Error,
    f: &mut std::fmt::Formatter,
) -> std::fmt::Result {
    write!(f, "{e}")?;

    let mut source = e.source();

    if e.source().is_some() {
        writeln!(f, "\ncaused by:")?;
        let mut i: usize = 0;
        while let Some(inner) = source {
            writeln!(f, "{i: >5}: {inner}")?;
            source = inner.source();
            i += 1;
        }
    }

    Ok(())
}

impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        format_error(self, f)
    }
}

/// The single line an operator should see when the program fails.
///
/// Most errors have nothing better than their own chain, so that is the
/// fallback. Three carry a sentence written for a human — the sign-in
/// instruction, and the two the server itself sent us — and for those the
/// wrapper text is noise.
#[must_use]
pub fn user_message(error: &Error) -> String {
    match error {
        // `auth::flow` writes these for the operator; passing them through
        // unchanged is the whole point of using a `String` variant.
        Error::Auth(message) => message.clone(),

        // The server's own words about why it refused. Anything we add here
        // pushes the sentence that matters further down the screen.
        Error::EsStream(llm_stream::error::Error::ApiError(detail))
        | Error::EsStream(llm_stream::error::Error::AuthError(detail)) => detail.clone(),

        other => {
            let mut rendered = other.to_string();
            let mut source = std::error::Error::source(other);
            while let Some(inner) = source {
                rendered.push_str(&format!("\ncaused by: {inner}"));
                source = inner.source();
            }
            rendered
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_errors_pass_through_verbatim() {
        let error = Error::Auth("not signed in — run: llm-stream --login".to_string());
        assert_eq!(
            user_message(&error),
            "not signed in — run: llm-stream --login"
        );
    }

    #[test]
    fn api_errors_show_only_the_servers_sentence() {
        let sentence =
            "The 'gpt-4o' model is not supported when using Codex with a ChatGPT account.";
        let error = Error::EsStream(llm_stream::error::Error::ApiError(sentence.to_string()));
        assert_eq!(user_message(&error), sentence);
        assert!(
            !user_message(&error).contains("unable to stream"),
            "the wrapper text leaked into the operator-facing message"
        );
    }

    #[test]
    fn library_auth_errors_show_only_the_servers_sentence() {
        let error = Error::EsStream(llm_stream::error::Error::AuthError("bad token".to_string()));
        assert_eq!(user_message(&error), "bad token");
    }

    #[test]
    fn other_errors_keep_their_chain() {
        let error = Error::ApiNotSpecified;
        assert_eq!(user_message(&error), "api not specified");
    }

    #[test]
    fn wrapped_errors_render_their_cause() {
        let json_error = Error::Json(serde_json::from_str::<i32>("nope").unwrap_err());
        let rendered = user_message(&json_error);
        assert!(rendered.starts_with("json error"), "got {rendered}");
        assert!(rendered.contains("caused by: "), "got {rendered}");
    }
}
