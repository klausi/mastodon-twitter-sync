extern crate chrono;
extern crate egg_mode;
extern crate mammut;
extern crate regex;
extern crate tokio_core;
extern crate toml;

use chrono::Duration;
use chrono::prelude::*;
use egg_mode::error::TwitterErrors;
use egg_mode::error::Error as EggModeError;
use mammut::Mastodon;
use mammut::Error as MammutError;
use mammut::entities::account::Account;
use std::collections::BTreeMap;
use std::fs::remove_file;
use tokio_core::reactor::Core;

use config::*;

// Delete old statuses of this account that are older than 90 days.
pub fn mastodon_delete_older_statuses(mastodon: &Mastodon, account: &Account) {
    // In order not to fetch old toots every time keep them in a cache file
    // keyed by their dates.
    let cache_file = "mastodon_cache.json";
    let dates = mastodon_load_toot_dates(mastodon, account, cache_file);
    let mut remove_dates = Vec::new();
    let three_months_ago = Utc::now() - Duration::days(90);
    for (date, toot_id) in dates.range(..three_months_ago) {
        println!("Deleting toot {} from {}", toot_id, date);
        remove_dates.push(date);
        // The status could have been deleted already by the user, ignore API
        // errors in that case.
        if let Err(error) = mastodon.delete_status(*toot_id) {
            match error {
                MammutError::Api(_) => {}
                _ => Err(error).unwrap(),
            }
        }
    }

    let mut new_dates = dates.clone();
    for remove_date in remove_dates {
        new_dates.remove(remove_date);
    }

    if new_dates.is_empty() {
        // If we have deleted all old toots from our cache file we can remove
        // it. On the next run all toots will be fetched and the cache
        // recreated.
        remove_file(cache_file).unwrap();
    } else {
        save_dates_to_cache(cache_file, &new_dates);
    }
}

fn mastodon_load_toot_dates(
    mastodon: &Mastodon,
    account: &Account,
    cache_file: &str,
) -> BTreeMap<DateTime<Utc>, u64> {
    match load_dates_from_cache(cache_file) {
        Some(dates) => dates,
        None => mastodon_fetch_toot_dates(mastodon, account, cache_file),
    }
}

fn mastodon_fetch_toot_dates(
    mastodon: &Mastodon,
    account: &Account,
    cache_file: &str,
) -> BTreeMap<DateTime<Utc>, u64> {
    let mut max_id = None;
    let mut dates = BTreeMap::new();
    loop {
        let statuses = mastodon
            .statuses(account.id, false, false, None, max_id)
            .unwrap();
        if statuses.is_empty() {
            break;
        }
        max_id = Some(statuses.last().unwrap().id);
        for status in statuses {
            dates.insert(status.created_at, status.id);
        }
    }

    save_dates_to_cache(cache_file, &dates);

    dates
}

// Delete old statuses of this account that are older than 90 days.
pub fn twitter_delete_older_statuses(user_id: u64, token: &egg_mode::Token) {
    // In order not to fetch old toots every time keep them in a cache file
    // keyed by their dates.
    let cache_file = "twitter_cache.json";
    let dates = twitter_load_tweet_dates(user_id, token, cache_file);
    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let mut remove_dates = Vec::new();
    let three_months_ago = Utc::now() - Duration::days(90);
    for (date, tweet_id) in dates.range(..three_months_ago) {
        println!("Deleting tweet {} from {}", tweet_id, date);
        remove_dates.push(date);
        let deletion = egg_mode::tweet::delete(*tweet_id, token, &handle);
        let delete_result = core.run(deletion);
        match delete_result {
            // The status could have been deleted already by the user, ignore API
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
        }
    }

    let mut new_dates = dates.clone();
    for remove_date in remove_dates {
        new_dates.remove(remove_date);
    }

    if new_dates.is_empty() {
        // If we have deleted all old tweets from our cache file we can remove
        // it. On the next run all tweets will be fetched and the cache
        // recreated.
        remove_file(cache_file).unwrap();
    } else {
        save_dates_to_cache(cache_file, &new_dates);
    }
}

fn twitter_load_tweet_dates(
    user_id: u64,
    token: &egg_mode::Token,
    cache_file: &str,
) -> BTreeMap<DateTime<Utc>, u64> {
    match load_dates_from_cache(cache_file) {
        Some(dates) => dates,
        None => twitter_fetch_tweet_dates(user_id, token, cache_file),
    }
}

fn twitter_fetch_tweet_dates(
    user_id: u64,
    token: &egg_mode::Token,
    cache_file: &str,
) -> BTreeMap<DateTime<Utc>, u64> {
    let mut core = Core::new().unwrap();
    let handle = core.handle();
    // Try to fetch as many tweets as possible at once, Twitter API docs say
    // that is 200.
    let timeline =
        egg_mode::tweet::user_timeline(user_id, true, true, token, &handle).with_page_size(200);
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
