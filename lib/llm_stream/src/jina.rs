use eventsource_client::{Client as EsClient, ClientBuilder, ReconnectOptions, SSE};
use futures::stream::{Stream, TryStreamExt};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::error::Error;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Body {
    pub url: String,
    pub html: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Options {
    /// URL of the HTML to transform.
    pub url: String,

    /// HTML to transform into markdown.
    pub html: String,

    /// Engine to use for parsing the content for the given URL.
    pub read_engine: Option<String>,

    /// Control the level of detail in the response to prevent over-filtering.
    pub content_format: Option<String>,

    /// Use ReaderLM-v2 for HTML to Markdown conversion.
    pub use_readerlm_v2: Option<bool>,

    /// Maximum time to wait for the webpage to load.
    pub timeout: Option<u32>,

    /// Limits the maximum number of tokens used for this request.
    pub token_budget: Option<u32>,

    /// List of comma separated CSS selectors to focus on more specific parts of the page.
    pub target_selector: Option<String>,

    /// List of comma separated CSS selectors to remove the specified elements of the page.
    pub wait_for_selector: Option<String>,

    /// List of comma separated CSS selectors to remove elements on the page.
    pub excluded_selector: Option<String>,

    /// Remove all images from the response.
    pub remove_all_images: Option<bool>,

    /// A "Buttons & Links" section will be created at the end.
    pub gather_all_links_at_the_end: Option<bool>,

    /// An "Images" section will be created at the end.
    pub gather_all_images_at_the_end: Option<bool>,
}

impl Options {
    /// Creates a new `Options` struct.
    #[must_use]
    pub fn new(url: impl Into<String>, html: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            html: html.into(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CompletionChunk {
    pub content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Auth {
    pub api_key: String,
}

impl Auth {
    #[must_use]
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    pub fn from_env() -> Result<Self, Error> {
        let api_key = match std::env::var("JINA_API_KEY") {
            Ok(key) => key,
            Err(_) => return Err(Error::AuthError("JINA_API_KEY not found".to_string())),
        };

        Ok(Self::new(api_key))
    }
}

#[derive(Debug, Clone)]
pub struct Client {
    pub auth: Auth,
    pub api_url: String,
}

impl Client {
    pub fn new(auth: Auth, api_url: Option<impl Into<String>>) -> Self {
        let api_url: String = if let Some(api_url) = api_url {
            api_url.into()
        } else {
            "https://r.jina.ai".to_string()
        };

        Self { auth, api_url }
    }
}

impl Client {
    pub fn delta<'a>(
        &'a self,
        options: &'a Options,
    ) -> Result<impl Stream<Item = Result<String, Error>> + 'a, Error> {
        log::debug!("options: {:#?}", options);

        let request_body = serde_json::json!({
                    "url": options.url.to_string(),
                    "html": options.html.to_string(),
        });
        log::debug!("request_body: {:#?}", request_body);

        let authorization: &str = &format!("Bearer {}", self.auth.api_key);

        let mut builder = ClientBuilder::for_url(&self.api_url.clone())?
            .header("content-type", "application/json")?
            .header("authorization", authorization)?
            .method("POST".into())
            .body(request_body.to_string())
            .reconnect(
                ReconnectOptions::reconnect(true)
                    .retry_initial(false)
                    .delay(Duration::from_secs(1))
                    .backoff_factor(2)
                    .delay_max(Duration::from_secs(60))
                    .build(),
            );

        if let Some(engine) = options.read_engine.as_ref() {
            builder = builder.header("x-engine", engine)?;
        }

        if let Some(content_format) = options.content_format.as_ref() {
            builder = builder.header("x-return-format", content_format)?;
        }

        if let Some(true) = options.use_readerlm_v2.as_ref() {
            builder = builder.header("x-respond-with", "readerlm-v2")?;
        }

        if let Some(timeout) = options.timeout {
            builder = builder.header("x-timeout", &timeout.to_string())?;
        }

        if let Some(token_budget) = options.token_budget {
            builder = builder.header("x-token-budget", &token_budget.to_string())?;
        }

        if let Some(target_selector) = options.target_selector.as_ref() {
            builder = builder.header("x-target-selector", target_selector)?;
        }

        if let Some(wait_for_selector) = options.wait_for_selector.as_ref() {
            builder = builder.header("x-wait-for-selector", wait_for_selector)?;
        }

        if let Some(excluded_selector) = options.excluded_selector.as_ref() {
            builder = builder.header("x-remove-selector", excluded_selector)?;
        }

        if let Some(true) = options.remove_all_images.as_ref() {
            builder = builder.header("x-retain-images", "none")?;
        }

        if let Some(true) = options.gather_all_links_at_the_end.as_ref() {
            builder = builder.header("x-gather-links", "true")?;
        }

        let client = builder.build();

        let stream = Box::pin(client.stream())
            .map_err(Error::from)
            .map_ok(|event| match event {
                SSE::Connected(_) => String::default(),
                SSE::Event(ev) => match serde_json::from_str::<CompletionChunk>(&ev.data) {
                    Ok(chunk) => {
                        if let Some(content) = chunk.content.as_ref() {
                            content.clone()
                        } else {
                            String::default()
                        }
                    }
                    Err(_) => String::default(),
                },
                SSE::Comment(comment) => {
                    log::debug!("Comment: {:#?}", comment);
                    String::default()
                }
            });

        Ok(stream)
    }
}
