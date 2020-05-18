use crate::errors::*;
use chrono::prelude::*;
use mammut::Data;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::fs::remove_file;

#[inline]
pub fn config_load(config: &str) -> Result<Config> {
    toml::from_str(config).map_err(Error::from)
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
    #[serde(default = "config_true_default")]
    pub sync_reblogs: bool,
    pub sync_hashtag: String,
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
    #[serde(default = "config_true_default")]
    pub sync_retweets: bool,
    pub sync_hashtag: String,
}

fn config_false_default() -> bool {
    false
}

fn config_true_default() -> bool {
    true
}

pub fn load_dates_from_cache(cache_file: &str) -> Result<Option<BTreeMap<DateTime<Utc>, u64>>> {
    if let Ok(json) = fs::read_to_string(cache_file) {
        let cache = serde_json::from_str(&json)?;
        Ok(Some(cache))
    } else {
        Ok(None)
    }
}

pub fn save_dates_to_cache(cache_file: &str, dates: &BTreeMap<DateTime<Utc>, u64>) -> Result<()> {
    let json = serde_json::to_string_pretty(&dates)?;
    fs::write(cache_file, json.as_bytes())?;
    Ok(())
}

// Delete a list of dates from the given cache of dates and write the cache to
// disk if necessary.
pub fn remove_dates_from_cache(
    remove_dates: Vec<&DateTime<Utc>>,
    cached_dates: &BTreeMap<DateTime<Utc>, u64>,
    cache_file: &str,
) -> Result<()> {
    if remove_dates.is_empty() {
        return Ok(());
    }

    let mut new_dates = cached_dates.clone();
    for remove_date in remove_dates {
        new_dates.remove(remove_date);
    }

    if new_dates.is_empty() {
        // If we have deleted all old dates from our cache file we can remove
        // it. On the next run all entries will be fetched and the cache
        // recreated.
        remove_file(cache_file)?;
    } else {
        save_dates_to_cache(cache_file, &new_dates)?;
    }

    Ok(())
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
sync_reblogs = false
sync_hashtag = ""
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
sync_retweets = false
sync_hashtag = ""
"#;
        let config: Config = toml::from_str(toml_config).unwrap();
        toml::to_string(&config).unwrap();
    }

    // Verify backwards compatibility with config files created for v1.3.3 or older
    // Ensure that serializing/deserializing of the TOML config does not throw
    // errors.
    #[test]
    fn serialize_config_v1_3_3() {
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
        // ^^notice sync_reblogs and sync_retweets is not set

        let config: Config = toml::from_str(toml_config).unwrap();
        assert_eq!(config.mastodon.sync_reblogs, true);
        assert_eq!(config.twitter.sync_retweets, true);
        toml::to_string(&config).unwrap();
    }
}
