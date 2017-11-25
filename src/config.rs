extern crate chrono;
extern crate mammut;
extern crate serde_json;
extern crate toml;

use chrono::prelude::*;
use mammut::Data;
use std::collections::BTreeMap;
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
    #[serde(default = "config_false_default")] pub delete_older_favs: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TwitterConfig {
    pub consumer_key: String,
    pub consumer_secret: String,
    pub access_token: String,
    pub access_token_secret: String,
    pub user_id: u64,
    pub user_name: String,
    #[serde(default = "config_false_default")] pub delete_older_statuses: bool,
}

fn config_false_default() -> bool {
    false
}

pub fn load_dates_from_cache(cache_file: &str) -> Option<BTreeMap<DateTime<Utc>, u64>> {
    let cache = match File::open(cache_file) {
        Ok(mut file) => {
            let mut json = String::new();
            file.read_to_string(&mut json).unwrap();
            serde_json::from_str(&json).unwrap()
        }
        Err(_) => return None,
    };
    Some(cache)
}

pub fn save_dates_to_cache(cache_file: &str, dates: &BTreeMap<DateTime<Utc>, u64>) {
    let json = serde_json::to_string(&dates).unwrap();
    let mut file = File::create(cache_file).unwrap();
    file.write_all(json.as_bytes()).unwrap();
}
