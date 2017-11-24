extern crate mammut;
extern crate toml;

use mammut::Data;
use std::fs::File;
use std::io::prelude::*;

pub fn config_load(mut file: File) -> Config {
    let mut config = String::new();
    file.read_to_string(&mut config).unwrap();
    toml::from_str(&config).unwrap()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub mastodon: MastodonConfig,
    pub twitter: TwitterConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MastodonConfig {
    pub app: Data,
    pub delete_older_statuses: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TwitterConfig {
    pub consumer_key: String,
    pub consumer_secret: String,
    pub access_token: String,
    pub access_token_secret: String,
    pub user_id: u64,
    pub user_name: String,
    #[serde(default = "twitter_config_delete_default")] pub delete_older_statuses: bool,
}

fn twitter_config_delete_default() -> bool {
    false
}
