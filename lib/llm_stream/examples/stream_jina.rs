use anyhow::Result;
use futures::stream::TryStreamExt;
use llm_stream::jina::{Auth, Client, Options};
use std::io::Write;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let key = std::env::var("JINA_API_KEY")?;

    let auth = Auth::new(key);
    let client = Client::new(auth, None::<String>);

    let url = "http://captive.apple.com";
    let html = "<HTML><HEAD><TITLE>Apple Captive Portal</TITLE></HEAD><BODY>Success</BODY></HTML>";
    let options = Options::new(url, html);

    // let mut stream = client.message_stream(&body)?;
    let mut stream = client.delta(&options)?;

    while let Ok(Some(text)) = stream.try_next().await {
        print!("{text}");
        std::io::stdout().flush()?;
    }

    Ok(())
}
