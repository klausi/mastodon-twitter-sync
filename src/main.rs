extern crate chrono;
extern crate dissolve;
extern crate egg_mode;
extern crate mammut;
extern crate regex;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate tokio_core;
extern crate toml;

use chrono::Duration;
use chrono::prelude::*;
use egg_mode::text::character_count;
use egg_mode::tweet::DraftTweet;
use egg_mode::tweet::Tweet;
use mammut::{Data, Mastodon, Registration};
use mammut::Error as MammutError;
use mammut::apps::{AppBuilder, Scope};
use mammut::entities::account::Account;
use mammut::entities::status::Status;
use mammut::status_builder::StatusBuilder;
use regex::Regex;
use std::collections::BTreeMap;
use std::io;
use std::fs::File;
use std::fs::remove_file;
use std::io::prelude::*;
use tokio_core::reactor::Core;

fn main() {
    let config = match File::open("mastodon-twitter-sync.toml") {
        Ok(f) => config_load(f),
        Err(_) => {
            let mastodon = mastodon_register();
            let twitter_config = twitter_register();
            let config = Config {
                mastodon: MastodonConfig {
                    app: (*mastodon).clone(),
                    // Do not delete older status per default, users should
                    // enable this explicitly.
                    delete_older_statuses: false,
                },
                twitter: twitter_config,
            };

            // Save config for using on the next run.
            let toml = toml::to_string(&config).unwrap();
            let mut file = File::create("mastodon-twitter-snyc.toml").unwrap();
            file.write_all(toml.as_bytes()).unwrap();

            config
        }
    };

    let mastodon = Mastodon::from_data(config.mastodon.app);

    let account = mastodon.verify().unwrap();
    let mastodon_statuses = mastodon
        .statuses(account.id, false, true, None, None)
        .unwrap();

    let con_token =
        egg_mode::KeyPair::new(config.twitter.consumer_key, config.twitter.consumer_secret);
    let access_token = egg_mode::KeyPair::new(
        config.twitter.access_token,
        config.twitter.access_token_secret,
    );
    let token = egg_mode::Token::Access {
        consumer: con_token,
        access: access_token,
    };

    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let mut timeline =
        egg_mode::tweet::user_timeline(config.twitter.user_id, false, true, &token, &handle)
            .with_page_size(50);

    let tweets = core.run(timeline.start()).unwrap();
    let posts = determine_posts(&mastodon_statuses, &*tweets);

    for toot in posts.toots {
        println!("Posting to Mastodon: {}", toot);
        mastodon.new_status(StatusBuilder::new(toot)).unwrap();
    }

    for tweet in posts.tweets {
        println!("Posting to Twitter: {}", tweet);
        core.run(DraftTweet::new(&tweet).send(&token, &handle))
            .unwrap();
    }

    // Delete old mastodon statuses if that option is enabled.
    if config.mastodon.delete_older_statuses {
        mastodon_delete_older_statuses(mastodon, account);
    }
    if config.twitter.delete_older_statuses {
        twitter_delete_older_statuses(config.twitter.user_id, &token);
    }
}

// Represents new status updates that should be posted to Twitter (tweets) and
// Mastodon (toots).
#[derive(Debug)]
struct StatusUpdates {
    tweets: Vec<String>,
    toots: Vec<String>,
}

fn determine_posts(mastodon_statuses: &[Status], twitter_statuses: &[Tweet]) -> StatusUpdates {
    let mut updates = StatusUpdates {
        tweets: Vec::new(),
        toots: Vec::new(),
    };
    let mut tweets = Vec::new();
    for tweet in twitter_statuses {
        // Replace those ugly t.co URLs in the tweet text.
        tweets.push(tweet_unshorten_decode(tweet));
    }

    let compare_toots = prepare_compare_toots(mastodon_statuses);

    'tweets: for tweet in &tweets {
        for toot in &compare_toots {
            // If the tweet already exists we can stop here and know that we are
            // synced.
            if toot == tweet {
                break 'tweets;
            }
        }
        // The tweet is not on Mastodon yet, let's post it.
        updates.toots.push(tweet.to_string());
    }

    'toots: for toot in mastodon_statuses {
        // Shorten and prepare the toots to be ready for posting on Twitter.
        let toot_text = mastodon_toot_get_text(toot);
        let shortened_toot = tweet_shorten(&toot_text, &toot.url);
        for tweet in &tweets {
            // If the toot already exists we can stop here and know that we are
            // synced.
            if &toot_text == tweet || &shortened_toot == tweet {
                break 'toots;
            }
        }
        // The toot is not on Twitter yet, let's post it.
        updates.tweets.push(shortened_toot);
    }
    updates
}

// Prepare a list of variations of Mastodon toots that could all be synced
// already.
fn prepare_compare_toots(mastodon_statuses: &[Status]) -> Vec<String> {
    let mut toots = Vec::new();
    for toot in mastodon_statuses {
        // Prepare the toots to be comparable with tweets.
        let toot_text = mastodon_toot_get_text(toot);
        // Mastodon allows up to 500 characters, so we might need to shorten the
        // toot. Also add the shortened version of the toot for comparison.
        let shortened_toot = tweet_shorten(&toot_text, &toot.url);
        if toot_text != shortened_toot {
            toots.push(shortened_toot);
        }
        toots.push(toot_text);
    }
    toots
}

// Replace t.co URLs and HTML entity decode &amp;
fn tweet_unshorten_decode(tweet: &Tweet) -> String {
    let (mut tweet_text, urls) = match tweet.retweeted_status {
        None => (tweet.text.clone(), &tweet.entities.urls),
        Some(ref retweet) => (
            format!(
                "RT @{}: {}",
                retweet.clone().user.unwrap().screen_name,
                retweet.text
            ),
            &retweet.entities.urls,
        ),
    };
    for url in urls {
        tweet_text = tweet_text.replace(&url.url, &url.expanded_url);
    }
    // Twitterposts have HTML entities such as &amp;, we need to decode them.
    dissolve::strip_html_tags(&tweet_text).join("")
}

fn tweet_shorten(text: &str, toot_url: &str) -> String {
    let (mut char_count, _) = character_count(text, 23, 23);
    let re = Regex::new(r"[^\s]+$").unwrap();
    let mut shortened = text.trim().to_string();
    let mut with_link = shortened.clone();

    // Twitter should allow 280 characters, but their counting is unpredictable.
    // Use 40 characters less and hope it works ¯\_(ツ)_/¯
    while char_count > 240 {
        // Remove the last word.
        shortened = re.replace_all(&shortened, "").trim().to_string();
        // Add a link to the toot that has the full text.
        with_link = shortened.clone() + "… " + toot_url;
        let (new_count, _) = character_count(&with_link, 23, 23);
        char_count = new_count;
    }
    with_link.to_string()
}

// Prefix boost toots with the author and strip HTML tags.
fn mastodon_toot_get_text(toot: &Status) -> String {
    let mut replaced = match toot.reblog {
        None => toot.content.clone(),
        Some(ref reblog) => format!("RT @{}: {}", reblog.account.username, reblog.content),
    };
    replaced = replaced.replace("<br />", "\n");
    replaced = replaced.replace("<br>", "\n");
    replaced = replaced.replace("</p><p>", "\n\n");
    replaced = replaced.replace("<p>", "");
    dissolve::strip_html_tags(&replaced).join("")
}

// Delete old statuses of this account that are older than 90 days.
fn mastodon_delete_older_statuses(mastodon: Mastodon, account: Account) {
    // In order not to fetch old toots every time keep them in a cache file
    // keyed by their dates.
    let cache_file = "mastodon_cache.json";
    let dates = mastodon_load_toot_dates(&mastodon, &account, cache_file);
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
        let json = serde_json::to_string(&new_dates).unwrap();
        let mut file = File::create(cache_file).unwrap();
        file.write_all(json.as_bytes()).unwrap();
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

fn load_dates_from_cache(cache_file: &str) -> Option<BTreeMap<DateTime<Utc>, u64>> {
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

    let json = serde_json::to_string(&dates).unwrap();
    let mut file = File::create(cache_file).unwrap();
    file.write_all(json.as_bytes()).unwrap();

    dates
}

// Delete old statuses of this account that are older than 90 days.
fn twitter_delete_older_statuses(user_id: u64, token: &egg_mode::Token) {
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
        core.run(deletion).unwrap();
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
        let json = serde_json::to_string(&new_dates).unwrap();
        let mut file = File::create(cache_file).unwrap();
        file.write_all(json.as_bytes()).unwrap();
    }
}

fn twitter_load_tweet_dates(user_id: u64, token: &egg_mode::Token, cache_file: &str) -> BTreeMap<DateTime<Utc>, u64> {
    match load_dates_from_cache(cache_file) {
        Some(dates) => dates,
        None => twitter_fetch_tweet_dates(user_id, token, cache_file),
    }
}

fn twitter_fetch_tweet_dates(user_id: u64, token: &egg_mode::Token, cache_file: &str) -> BTreeMap<DateTime<Utc>, u64> {
    let mut core = Core::new().unwrap();
    let handle = core.handle();
    // Try to fetch as many tweets as possible at once, Twitter API docs say
    // that is 200.
    let mut timeline =
        egg_mode::tweet::user_timeline(user_id, true, true, &token, &handle)
            .with_page_size(200);
    let mut dates = BTreeMap::new();
    loop {
        let tweets = core.run(timeline.older(None)).unwrap();
        if tweets.is_empty() {
            break;
        }
        for tweet in tweets {
            dates.insert(tweet.created_at, tweet.id);
        }
    }

    let json = serde_json::to_string(&dates).unwrap();
    let mut file = File::create(cache_file).unwrap();
    file.write_all(json.as_bytes()).unwrap();

    dates
}

fn mastodon_register() -> Mastodon {
    let app = AppBuilder {
        client_name: "mastodon-twitter-sync",
        redirect_uris: "urn:ietf:wg:oauth:2.0:oob",
        scopes: Scope::ReadWrite,
        website: Some("https://github.com/klausi/mastodon-twitter-sync"),
    };

    let instance = console_input(
        "Provide the URL of your Mastodon instance, for example https://mastodon.social ",
    );
    let mut registration = Registration::new(instance);
    registration.register(app).unwrap();
    let url = registration.authorise().unwrap();
    println!("Click this link to authorize on Mastodon: {}", url);

    let code = console_input("Paste the returned authorization code");
    registration.create_access_token(code.to_string()).unwrap()
}

fn config_load(mut file: File) -> Config {
    let mut config = String::new();
    file.read_to_string(&mut config).unwrap();
    toml::from_str(&config).unwrap()
}

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    mastodon: MastodonConfig,
    twitter: TwitterConfig,
}

#[derive(Debug, Serialize, Deserialize)]
struct MastodonConfig {
    app: Data,
    delete_older_statuses: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct TwitterConfig {
    consumer_key: String,
    consumer_secret: String,
    access_token: String,
    access_token_secret: String,
    user_id: u64,
    user_name: String,
    #[serde(default = "twitter_config_delete_default")]
    delete_older_statuses: bool,
}

fn twitter_config_delete_default() -> bool {
    false
}

fn twitter_register() -> TwitterConfig {
    println!("Go to https://apps.twitter.com/app/new to create a new Twitter app.");
    println!("Name: Mastodon Twitter Sync");
    println!("Description: Synchronizes Tweets and Toots");
    println!("Website: https://github.com/klausi/mastodon-twitter-sync");

    let consumer_key = console_input("Paste your consumer key");
    let consumer_secret = console_input("Paste your consumer secret");

    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let con_token = egg_mode::KeyPair::new(consumer_key.clone(), consumer_secret.clone());
    let request_token = core.run(egg_mode::request_token(&con_token, "oob", &handle))
        .unwrap();
    println!(
        "Click this link to authorize on Twitter: {}",
        egg_mode::authorize_url(&request_token)
    );
    let pin = console_input("Paste your PIN");

    let (token, user_id, screen_name) = core.run(egg_mode::access_token(
        con_token,
        &request_token,
        pin,
        &handle,
    )).unwrap();

    match token {
        egg_mode::Token::Access {
            access: ref access_token,
            ..
        } => TwitterConfig {
            consumer_key: consumer_key,
            consumer_secret: consumer_secret,
            access_token: access_token.key.to_string(),
            access_token_secret: access_token.secret.to_string(),
            user_id: user_id,
            user_name: screen_name,
            delete_older_statuses: false,
        },
        _ => unreachable!(),
    }
}

fn console_input(prompt: &str) -> String {
    println!("{}: ", prompt);
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line).unwrap();
    line.trim().to_string()
}


#[cfg(test)]
mod tests {
    use super::*;
    use egg_mode::tweet::{TweetEntities, TweetSource};

    #[test]
    fn tweet_shortening() {
        let toot = "#MASTODON POST PRIVACY - who can see your post?

PUBLIC 🌏 Anyone can see and boost your post everywhere.

UNLISTED 🔓 ✅ Tagged people
✅ Followers
✅ People who look for it
❌ Local and federated timelines
✅ Boostable

FOLLOWERS ONLY 🔐 ✅ Tagged people
✅ Followers
❌ People who look for it
❌ Local and federated timelines
❌ Boostable

DIRECT MESSAGE ✉️
✅ Tagged people
❌ Followers
❌ People who look for it
❌ Local and federated timelines
❌ Boostable

https://cybre.space/media/J-amFmXPvb_Mt7toGgs #tutorial #howto
";
        let shortened_for_twitter =
            tweet_shorten(toot, "https://mastodon.social/@klausi/98999025586548863");
        assert_eq!(
            shortened_for_twitter,
            "#MASTODON POST PRIVACY - who can see your post?

PUBLIC 🌏 Anyone can see and boost your post everywhere.

UNLISTED 🔓 ✅ Tagged people
✅ Followers
✅ People who look for it
❌ Local and federated timelines
✅ Boostable… https://mastodon.social/@klausi/98999025586548863"
        );
    }

    // Test that if a long Mastodon toot already exists as short version on
    // Twitter that it is not posted again.
    #[test]
    fn short_version_on_twitter() {
        let mut status = get_mastodon_status();
        let long_toot = "test test test test test test test test test test test test test
            test test test test test test test test test test test test test
            test test test test test test test test test test test test test
            test test test test test test test test test test test test test
            test test test test";
        status.content = long_toot.to_string();

        let mut tweet = get_twitter_status();
        tweet.text = tweet_shorten(long_toot, &status.url);

        let tweets = vec![tweet];
        let statuses = vec![status];
        let posts = determine_posts(&statuses, &tweets);
        assert!(posts.toots.is_empty());
        assert!(posts.tweets.is_empty());
    }

    // Test an over long post of 280 characters that is the exact same on both
    // Mastodon and Twitter. No sync work necessary.
    #[test]
    fn over_long_status_on_both() {
        let mut status = get_mastodon_status();
        let long_toot = "test test test test test test test test test test test test test
            test test test test test test test test test test test test test
            test test test test test test test test test test test test test
            test test test test test test test test test test test test test
            test test test test";
        status.content = long_toot.to_string();

        let mut tweet = get_twitter_status();
        tweet.text = long_toot.to_string();

        let tweets = vec![tweet];
        let statuses = vec![status];
        let posts = determine_posts(&statuses, &tweets);
        assert!(posts.toots.is_empty());
        assert!(posts.tweets.is_empty());
    }

    // Test that Mastodon status text is posted HTML entity decoded to Twitter.
    // &amp; => &
    #[test]
    fn mastodon_html_decode() {
        let mut status = get_mastodon_status();
        status.content = "<p>You &amp; me!</p>".to_string();
        let posts = determine_posts(&vec![status], &Vec::new());
        assert_eq!(posts.tweets[0], "You & me!");
    }

    // Test that Twitter status text is posted HTML entity decoded to Mastodon.
    // &amp; => &
    #[test]
    fn twitter_html_decode() {
        let mut status = get_twitter_status();
        status.text = "You &amp; me!".to_string();
        let posts = determine_posts(&Vec::new(), &vec![status]);
        assert_eq!(posts.toots[0], "You & me!");
    }

    // Test that a boost on Mastodon is prefixed with "RT @..." when posted to
    // Twitter.
    #[test]
    fn mastodon_boost() {
        let mut reblog = get_mastodon_status();
        reblog.content = "<p>Some example toooot!</p>".to_string();
        let mut status = get_mastodon_status();
        status.reblog = Some(Box::new(reblog));
        status.reblogged = Some(true);

        let posts = determine_posts(&vec![status], &Vec::new());
        assert_eq!(posts.tweets[0], "RT @example: Some example toooot!");
    }

    fn get_mastodon_status() -> Status {
        let json = {
            let mut file = File::open("src/mastodon_status.json").unwrap();
            let mut ret = String::new();
            file.read_to_string(&mut ret).unwrap();
            ret
        };
        let status: Status = serde_json::from_str(&json).unwrap();
        status
    }

    fn get_twitter_status() -> Tweet {
        Tweet {
            coordinates: None,
            created_at: Utc::now(),
            current_user_retweet: None,
            display_text_range: None,
            entities: TweetEntities {
                hashtags: Vec::new(),
                symbols: Vec::new(),
                urls: Vec::new(),
                user_mentions: Vec::new(),
                media: None,
            },
            extended_entities: None,
            favorite_count: 0,
            favorited: None,
            id: 123456,
            in_reply_to_user_id: None,
            in_reply_to_screen_name: None,
            in_reply_to_status_id: None,
            lang: "".to_string(),
            place: None,
            possibly_sensitive: None,
            quoted_status_id: None,
            quoted_status: None,
            retweet_count: 0,
            retweeted: None,
            retweeted_status: None,
            source: TweetSource {
                name: "".to_string(),
                url: "".to_string(),
            },
            text: "".to_string(),
            truncated: false,
            user: None,
            withheld_copyright: false,
            withheld_in_countries: None,
            withheld_scope: None,
        }
    }
}
