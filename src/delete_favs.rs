extern crate chrono;
extern crate egg_mode;
extern crate mammut;
extern crate regex;
extern crate tokio_core;
extern crate toml;

use chrono::prelude::*;
use chrono::Duration;
use egg_mode::error::Error as EggModeError;
use egg_mode::error::TwitterErrors;
use mammut::Error as MammutError;
use mammut::Mastodon;
use std::collections::BTreeMap;
use std::str::FromStr;
use tokio_core::reactor::Core;

use config::*;

// Delete old favourites of this account that are older than 90 days.
pub fn mastodon_delete_older_favs(mastodon: &Mastodon) {
    // In order not to fetch old favs every time keep them in a cache file
    // keyed by their dates.
    let cache_file = "mastodon_fav_cache.json";
    let dates = mastodon_load_fav_dates(mastodon, cache_file);
    let mut remove_dates = Vec::new();
    let three_months_ago = Utc::now() - Duration::days(90);
    for (date, toot_id) in dates.range(..three_months_ago) {
        println!("Deleting Mastodon fav {} from {}", toot_id, date);
        remove_dates.push(date);
        // The status could have been deleted already by the user, ignore API
        // errors in that case.
        if let Err(error) = mastodon.unfavourite(*toot_id) {
            match error {
                MammutError::Api(_) => {}
                _ => Err(error).unwrap(),
            }
        }
    }
    remove_dates_from_cache(remove_dates, &dates, cache_file);
}

fn mastodon_load_fav_dates(mastodon: &Mastodon, cache_file: &str) -> BTreeMap<DateTime<Utc>, u64> {
    match load_dates_from_cache(cache_file) {
        Some(dates) => dates,
        None => mastodon_fetch_fav_dates(mastodon, cache_file),
    }
}

fn mastodon_fetch_fav_dates(mastodon: &Mastodon, cache_file: &str) -> BTreeMap<DateTime<Utc>, u64> {
    let mut dates = BTreeMap::new();
    let mut favourites_pager = mastodon.favourites().unwrap();
    for status in &favourites_pager.initial_items {
        let id = u64::from_str(&status.id).unwrap();
        dates.insert(status.created_at, id);
    }
    loop {
        let statuses = favourites_pager.next_page().unwrap();
        if statuses.is_none() {
            break;
        }
        for status in statuses.unwrap() {
            let id = u64::from_str(&status.id).unwrap();
            dates.insert(status.created_at, id);
        }
    }

    save_dates_to_cache(cache_file, &dates);

    dates
}

// Delete old likes of this account that are older than 90 days.
pub fn twitter_delete_older_favs(user_id: u64, token: &egg_mode::Token) {
    // In order not to fetch old likes every time keep them in a cache file
    // keyed by their dates.
    let cache_file = "twitter_fav_cache.json";
    let dates = twitter_load_fav_dates(user_id, token, cache_file);
    let mut core = Core::new().unwrap();
    let mut remove_dates = Vec::new();
    let three_months_ago = Utc::now() - Duration::days(90);
    for (delete_count, (date, tweet_id)) in dates.range(..three_months_ago).enumerate() {
        println!("Deleting Twitter fav {} from {}", tweet_id, date);
        remove_dates.push(date);
        let deletion = egg_mode::tweet::unlike(*tweet_id, token);
        let delete_result = core.run(deletion);
        match delete_result {
            // The like could have been deleted already by the user, ignore API
            // errors in that case.
            Err(EggModeError::TwitterError(TwitterErrors { errors: e })) => {
                // Error 144 is "No status found with that ID".
                if e.len() != 1 || e[0].code != 144 {
                    println!("{:#?}", e);
                    panic!("Twitter response error");
                }
            }
            Err(_) => {
                delete_result.unwrap();
            }
            Ok(_) => {}
        };
        // Only delete 100 likes in one run to not run into API limits or open
        // network port limits.
        if delete_count == 100 {
            println!(
                "Stopping Twitter fav deletion to not run into API limits. Just run me again!"
            );
            break;
        }
    }
    remove_dates_from_cache(remove_dates, &dates, cache_file);
}

fn twitter_load_fav_dates(
    user_id: u64,
    token: &egg_mode::Token,
    cache_file: &str,
) -> BTreeMap<DateTime<Utc>, u64> {
    match load_dates_from_cache(cache_file) {
        Some(dates) => dates,
        None => twitter_fetch_fav_dates(user_id, token, cache_file),
    }
}

fn twitter_fetch_fav_dates(
    user_id: u64,
    token: &egg_mode::Token,
    cache_file: &str,
) -> BTreeMap<DateTime<Utc>, u64> {
    let mut core = Core::new().unwrap();
    // Try to fetch as many tweets as possible at once, Twitter API docs say
    // that is 200.
    let timeline = egg_mode::tweet::liked_by(user_id, token).with_page_size(200);
    let mut max_id = None;
    let mut dates = BTreeMap::new();
    loop {
        let tweets = core.run(timeline.call(None, max_id)).unwrap();
        if tweets.is_empty() {
            break;
        }
        for tweet in tweets {
            dates.insert(tweet.created_at, tweet.id);
            if max_id.is_none() || tweet.id < max_id.unwrap() {
                max_id = Some(tweet.id - 1);
            }
        }
    }

    save_dates_to_cache(cache_file, &dates);

    dates
}
