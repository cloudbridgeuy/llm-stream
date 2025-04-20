use clap::Parser;

#[derive(Clone, Debug, Parser)]
#[command(name = "jina", version = "0.1.0")]
#[command(about = "Interact with the Jina Ai API to convert HTML into Markdown")]
pub struct Args {
    /// HTML code to transform.
    #[clap(hide = true, default_value = "-")]
    pub html: clap_stdin::FileOrStdin,

    /// URL of the HTML to transform.
    #[clap(short, long)]
    pub url: String,

    /// Engine to use for parsing the content for the given URL.
    #[clap(long)]
    pub read_engine: Option<String>,

    /// Control the level of detail in the response to prevent over-filtering.
    #[clap(long)]
    pub content_format: Option<String>,

    /// Use ReaderLM-v2 for HTML to Markdown conversion.
    #[clap(long)]
    pub use_readerlm_v2: bool,

    /// Maximum time to wait for the webpage to load.
    #[clap(long)]
    pub timeout: Option<u32>,

    /// Limits the maximum number of tokens used for this request.
    #[clap(long)]
    pub token_budget: Option<u32>,

    /// List of comma separated CSS selectors to focus on more specific parts of the page.
    #[clap(long)]
    pub target_selector: Option<String>,

    /// List of comma separated CSS selectors to remove the specified elements of the page.
    #[clap(long)]
    pub wait_for_selector: Option<String>,

    /// List of comma separated CSS selectors to remove elements on the page.
    #[clap(long)]
    pub excluded_selector: Option<String>,

    /// Remove all images from the response.
    #[clap(long)]
    pub remove_all_images: bool,

    /// A "Buttons & Links" section will be created at the end.
    #[clap(long)]
    pub gather_all_links_at_the_end: bool,

    /// An "Images" section will be created at the end.
    #[clap(long)]
    pub gather_all_images_at_the_end: bool,

    /// The api key to use (will override the value of the environment variable.)
    #[clap(long)]
    pub api_key: Option<String>,

    /// The api base url.
    #[clap(long)]
    pub api_base_url: Option<String>,

    /// The environment variable to use to get the access token for the api.
    #[clap(long)]
    pub api_env: Option<String>,

    /// Don't run the spinner
    #[clap(long)]
    pub quiet: bool,

    /// Language to use for syntax highlight
    #[clap(long, default_value = "tokyonight-storm")]
    pub theme: Option<String>,
}

impl From<Args> for llm_stream::jina::Options {
    fn from(args: Args) -> Self {
        Self {
            wait_for_selector: args.wait_for_selector,
            target_selector: args.target_selector,
            excluded_selector: args.excluded_selector,
            gather_all_links_at_the_end: Some(args.gather_all_links_at_the_end),
            token_budget: args.token_budget,
            use_readerlm_v2: Some(args.use_readerlm_v2),
            timeout: args.timeout,
            content_format: args.content_format,
            gather_all_images_at_the_end: Some(args.gather_all_images_at_the_end),
            read_engine: args.read_engine,
            remove_all_images: Some(args.remove_all_images),
            url: args.url,
            html: args.html.contents().expect("HTML to be readable"),
        }
    }
}
