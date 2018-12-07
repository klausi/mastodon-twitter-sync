use chrono::prelude::*;
use mammut::Data;
use serde_derive::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::remove_file;
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
    pub delete_older_statuses: bool,
    #[serde(default = "config_false_default")]
    pub delete_older_favs: bool,
    pub app: Data,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TwitterConfig {
    pub consumer_key: String,
    pub consumer_secret: String,
    pub access_token: String,
    pub access_token_secret: String,
    pub user_id: u64,
    pub user_name: String,
    #[serde(default = "config_false_default")]
    pub delete_older_statuses: bool,
    #[serde(default = "config_false_default")]
    pub delete_older_favs: bool,
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

// Delete a list of dates from the given cache of dates and write the cache to
// disk if necessary.
pub fn remove_dates_from_cache(
    remove_dates: Vec<&DateTime<Utc>>,
    cached_dates: &BTreeMap<DateTime<Utc>, u64>,
    cache_file: &str,
) {
    if remove_dates.is_empty() {
        return;
    }

    let mut new_dates = cached_dates.clone();
    for remove_date in remove_dates {
        new_dates.remove(remove_date);
    }

    if new_dates.is_empty() {
        // If we have deleted all old dates from our cache file we can remove
        // it. On the next run all entries will be fetched and the cache
        // recreated.
        remove_file(cache_file).unwrap();
    } else {
        save_dates_to_cache(cache_file, &new_dates);
    }
}

#[cfg(test)]
mod tests {

    use crate::config::*;

    // Ensure that serializing/deserializing of the TOML config does not throw
    // errors.
    #[test]
    fn serialize_config() {
        let toml_config = r#"
[mastodon]
delete_older_statuses = true
delete_older_favs = true
[mastodon.app]
base = "https://mastodon.social"
client_id = "abcd"
client_secret = "abcd"
redirect = "urn:ietf:wg:oauth:2.0:oob"
token = "1234"
[twitter]
consumer_key = "abcd"
consumer_secret = "abcd"
access_token = "1234"
access_token_secret = "1234"
user_id = 0
user_name = " "
delete_older_statuses = true
delete_older_favs = true
"#;
        let config: Config = toml::from_str(toml_config).unwrap();
        toml::to_string(&config).unwrap();
    }
}
