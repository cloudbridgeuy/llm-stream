use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Preset {
    pub name: String,

    // Api
    pub api: crate::args::Api,
    pub env: Option<String>,
    pub key: Option<String>,
    pub base_url: Option<String>,

    // Model
    pub model: Option<String>,

    // Model Configuration
    pub system: Option<String>,
    pub max_tokens: Option<u32>,
    pub min_tokens: Option<u32>,
    pub version: Option<String>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub enum Role {
    Assistant,
    Model,
    #[default]
    User,
    Human,
    System,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Template {
    pub name: String,
    pub description: Option<String>,
    pub template: String,
    pub default_vars: Option<Value>,
    pub system: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Config {
    // Api
    #[serde(default = "default_api")]
    pub api: Option<crate::args::Api>,
    #[serde(default = "default_base_url")]
    pub base_url: Option<String>,
    #[serde(default = "default_env")]
    pub env: Option<String>,
    pub key: Option<String>,

    // Presets
    pub presets: Option<Vec<Preset>>,

    // Templates
    pub templates: Option<Vec<Template>>,

    // Global
    #[serde(default = "default_false")]
    pub quiet: Option<bool>,
    #[serde(default = "default_language")]
    pub language: Option<String>,
    #[serde(default = "default_theme")]
    pub theme: Option<String>,

    // Model
    pub model: Option<String>,

    // Model Configuration
    pub system: Option<String>,
    pub max_tokens: Option<u32>,
    pub min_tokens: Option<u32>,
    pub version: Option<String>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
}

impl Config {
    pub fn new() -> Self {
        let config: Config = serde_json::from_str("{}").unwrap();
        config
    }
}

fn default_api() -> Option<crate::args::Api> {
    Some(crate::args::Api::OpenAi)
}

fn default_base_url() -> Option<String> {
    Some("https://api.openai.com/v1".to_string())
}

fn default_env() -> Option<String> {
    Some("OPENAI_API_KEY".to_string())
}

fn default_false() -> Option<bool> {
    Some(false)
}

fn default_language() -> Option<String> {
    Some("markdown".to_string())
}

fn default_theme() -> Option<String> {
    Some("ansi".to_string())
}
