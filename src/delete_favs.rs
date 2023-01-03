use anyhow::Result;
use chrono::prelude::*;
use chrono::Duration;
use egg_mode::error::Error as EggModeError;
use egg_mode::error::TwitterErrors;
use elefren::Error as ElefrenError;
use elefren::Mastodon;
use elefren::MastodonClient;
use std::collections::BTreeMap;
use std::str::FromStr;

use crate::cache_file;
use crate::config::*;

// Delete old favourites of this account that are older than 90 days.
pub fn mastodon_delete_older_favs(mastodon: &Mastodon, dry_run: bool) -> Result<()> {
    // In order not to fetch old favs every time keep them in a cache file
    // keyed by their dates.
    let cache_file = &cache_file("mastodon_fav_cache.json");
    let dates = mastodon_load_fav_dates(mastodon, cache_file)?;
    let mut remove_dates = Vec::new();
    let three_months_ago = Utc::now() - Duration::days(90);
    for (date, toot_id) in dates.range(..three_months_ago) {
        println!("Deleting Mastodon fav {toot_id} from {date}");
        // Do nothing on a dry run, just print what would be done.
        if dry_run {
            continue;
        }

        remove_dates.push(date);
        // The status could have been deleted already by the user, ignore API
        // errors in that case.
        if let Err(error) = mastodon.unfavourite(&format!("{toot_id}")) {
            match error {
                ElefrenError::Api(_) => {}
                _ => return Err(error.into()),
            }
        }
    }
    remove_dates_from_cache(remove_dates, &dates, cache_file)
}

fn mastodon_load_fav_dates(
    mastodon: &Mastodon,
    cache_file: &str,
) -> Result<BTreeMap<DateTime<Utc>, u64>> {
    match load_dates_from_cache(cache_file)? {
        Some(dates) => Ok(dates),
        None => mastodon_fetch_fav_dates(mastodon, cache_file),
    }
}

fn mastodon_fetch_fav_dates(
    mastodon: &Mastodon,
    cache_file: &str,
) -> Result<BTreeMap<DateTime<Utc>, u64>> {
    let mut dates = BTreeMap::new();
    let mut favourites_pager = mastodon.favourites()?;
    for status in &favourites_pager.initial_items {
        let id = u64::from_str(&status.id)?;
        dates.insert(status.created_at, id);
    }
    loop {
        let statuses = favourites_pager.next_page()?;
        if let Some(statuses) = statuses {
            for status in statuses {
                let id = u64::from_str(&status.id)?;
                dates.insert(status.created_at, id);
            }
        } else {
            break;
        }
    }

    save_dates_to_cache(cache_file, &dates)?;

    Ok(dates)
}

// Delete old likes of this account that are older than 90 days.
pub async fn twitter_delete_older_favs(
    user_id: u64,
    token: &egg_mode::Token,
    dry_run: bool,
) -> Result<()> {
    // In order not to fetch old likes every time keep them in a cache file
    // keyed by their dates.
    let cache_file = &cache_file("twitter_fav_cache.json");
    let dates = twitter_load_fav_dates(user_id, token, cache_file).await?;
    let mut remove_dates = Vec::new();
    let three_months_ago = Utc::now() - Duration::days(90);
    for (delete_count, (date, tweet_id)) in dates.range(..three_months_ago).enumerate() {
        println!("Deleting Twitter fav {tweet_id} from {date}");
        // Do nothing on a dry run, just print what would be done.
        if dry_run {
            continue;
        }

        remove_dates.push(date);
        let delete_result = egg_mode::tweet::unlike(*tweet_id, token).await;
        // The like could have been deleted already by the user, ignore API
        // errors in that case.
        if let Err(EggModeError::TwitterError(headers, TwitterErrors { errors: e })) = delete_result
        {
            // Error 144 is "No status found with that ID".
            if e.len() != 1 || e[0].code != 144 {
                return Err(anyhow::Error::from(EggModeError::TwitterError(
                    headers,
                    TwitterErrors { errors: e },
                )));
            }
        } else {
            delete_result?;
        }
        // Only delete 100 likes in one run to not run into API limits or open
        // network port limits.
        if delete_count == 100 {
            println!(
                "Stopping Twitter fav deletion to not run into API limits. Just run me again!"
            );
            break;
        }
    }
    remove_dates_from_cache(remove_dates, &dates, cache_file)
}

async fn twitter_load_fav_dates(
    user_id: u64,
    token: &egg_mode::Token,
    cache_file: &str,
) -> Result<BTreeMap<DateTime<Utc>, u64>> {
    match load_dates_from_cache(cache_file)? {
        Some(dates) => Ok(dates),
        None => twitter_fetch_fav_dates(user_id, token, cache_file).await,
    }
}

async fn twitter_fetch_fav_dates(
    user_id: u64,
    token: &egg_mode::Token,
    cache_file: &str,
) -> Result<BTreeMap<DateTime<Utc>, u64>> {
    // Try to fetch as many tweets as possible at once, Twitter API docs say
    // that is 200.
    let timeline = egg_mode::tweet::liked_by(user_id, token).with_page_size(200);
    let mut max_id = None;
    let mut dates = BTreeMap::new();
    loop {
        let tweets = timeline.call(None, max_id).await?;
        if tweets.is_empty() {
            break;
        }
        for tweet in tweets.iter() {
            dates.insert(tweet.created_at, tweet.id);
            if let Some(max) = max_id {
                if tweet.id < max {
                    max_id = Some(tweet.id - 1);
                }
            } else {
                max_id = Some(tweet.id - 1);
            }
        }
    }

    save_dates_to_cache(cache_file, &dates)?;

    Ok(dates)
}
