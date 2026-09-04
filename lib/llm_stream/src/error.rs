use thiserror::Error;

pub use eventsource_client::Error as EventsourceError;

/// Error type returned from this library's functions
#[derive(Debug, Error)]
pub enum Error {
    /// An error when creating the SSE stream.
    #[error("Eventsource Client error: {0}")]
    EventsourceClient(#[from] eventsource_client::Error),
    /// An Error returned by the API
    #[error("AuthError Error: {0}")]
    AuthError(String),
    /// An Error returned by the API
    #[error("API Error: {0}")]
    ApiError(String),
    /// An Error not related to the API
    #[error("Request Error: {0}")]
    RequestError(String),
    /// De/serialization error
    #[error("de/serialize error: {0}")]
    Serde(#[from] serde_json::error::Error),
    /// An Error occurred when performing an IO operation.
    #[error("io error: {0}")]
    IO(#[from] std::io::Error),
}

/// Extracts the human-readable message from an error body.
///
/// The Codex endpoint answers a rejected request with
/// `{"detail":"The 'gpt-4o' model is not supported when using Codex with a
/// ChatGPT account."}`. Other OpenAI surfaces use `{"error":{"message":…}}`,
/// and some use a bare `{"message":…}`, so all three are read here — the point
/// of this function is that the operator sees a sentence, not a JSON blob.
///
/// Returns `None` when the body is not JSON or carries none of those keys; the
/// caller falls back to showing the body verbatim.
#[must_use]
pub fn detail_of(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;

    let message = value
        .get("detail")
        .or_else(|| value.get("error").and_then(|error| error.get("message")))
        .or_else(|| value.get("message"))?;

    message.as_str().map(str::to_owned)
}

/// Turns a transport error into ours, reading the response body when the server
/// rejected the request outright.
///
/// This is the impure half: it awaits the body off the wire. All of the
/// deciding lives in [`detail_of`], which is pure and tested.
pub async fn map_stream_error(error: eventsource_client::Error) -> Error {
    let eventsource_client::Error::UnexpectedResponse(response, body) = error else {
        return Error::from(error);
    };

    let status = response.status();

    match body.body_bytes().await {
        Ok(bytes) => {
            let raw = String::from_utf8_lossy(&bytes).into_owned();
            detail_of(&raw).map_or_else(
                || {
                    if raw.trim().is_empty() {
                        Error::ApiError(format!("HTTP {status}"))
                    } else {
                        // No sentence we recognise — better the raw body than
                        // nothing, since the operator has to act on it.
                        Error::ApiError(format!("HTTP {status}: {raw}"))
                    }
                },
                Error::ApiError,
            )
        }
        Err(read_error) => Error::ApiError(format!(
            "HTTP {status} (response body could not be read: {read_error})"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_of_extracts_the_codex_rejection_sentence() {
        let body = r#"{"detail":"The 'gpt-4o' model is not supported when using Codex with a ChatGPT account."}"#;
        assert_eq!(
            detail_of(body).as_deref(),
            Some("The 'gpt-4o' model is not supported when using Codex with a ChatGPT account.")
        );
    }

    #[test]
    fn detail_of_reads_the_nested_openai_error_shape() {
        let body = r#"{"error":{"message":"Invalid token","type":"invalid_request_error"}}"#;
        assert_eq!(detail_of(body).as_deref(), Some("Invalid token"));
    }

    #[test]
    fn detail_of_reads_a_bare_message() {
        assert_eq!(detail_of(r#"{"message":"nope"}"#).as_deref(), Some("nope"));
    }

    #[test]
    fn detail_of_returns_none_when_there_is_no_sentence_to_show() {
        assert!(detail_of("not json").is_none());
        assert!(detail_of("").is_none());
        assert!(detail_of(r#"{"code":429}"#).is_none());
        // A non-string `detail` is not a sentence.
        assert!(detail_of(r#"{"detail":{"nested":"thing"}}"#).is_none());
    }
}
