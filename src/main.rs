extern crate dissolve;
extern crate egg_mode;
extern crate mammut;
extern crate regex;
#[macro_use]
extern crate serde_derive;
extern crate tokio_core;
extern crate toml;

use egg_mode::text::character_count;
use egg_mode::tweet::DraftTweet;
use egg_mode::tweet::Tweet;
use mammut::{Data, Mastodon, Registration};
use mammut::apps::{AppBuilder, Scope};
use mammut::entities::status::Status;
use mammut::status_builder::StatusBuilder;
use regex::Regex;
use std::io;
use std::fs::File;
use std::io::prelude::*;
use tokio_core::reactor::Core;

fn main() {
    let mastodon = match File::open("mastodon.toml") {
        Ok(f) => mastodon_load_from_config(f),
        Err(_) => mastodon_register(),
    };

    let account = mastodon.verify().unwrap();
    let mastodon_statuses = mastodon.statuses(account.id, false, true).unwrap();

    let twitter_config = match File::open("twitter.toml") {
        Ok(f) => twitter_load_from_config(f),
        Err(_) => twitter_register(),
    };

    let con_token =
        egg_mode::KeyPair::new(twitter_config.consumer_key, twitter_config.consumer_secret);
    let access_token = egg_mode::KeyPair::new(
        twitter_config.access_token,
        twitter_config.access_token_secret,
    );
    let token = egg_mode::Token::Access {
        consumer: con_token,
        access: access_token,
    };

    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let mut timeline =
        egg_mode::tweet::user_timeline(twitter_config.user_id, false, true, &token, &handle)
            .with_page_size(50);

    let tweets = core.run(timeline.start()).unwrap();
    let posts = determine_posts(&mastodon_statuses, &*tweets);

    for toot in posts.toots {
        println!("Posting to Mastodon: {}", toot);
        mastodon
            .new_status(StatusBuilder::new(toot))
            .unwrap();
    }

    for tweet in posts.tweets {
        println!("Posting to Twitter: {}", tweet);
        core.run(DraftTweet::new(&tweet).send(&token, &handle))
            .unwrap();
    }
}

// Represents new status updates that should be posted to Twitter (tweets) and
// Mastodon (toots).
#[derive(Debug)]
struct StatusUpdates {
    tweets: Vec<String>,
    toots: Vec<String>,
}

fn determine_posts(mastodon_statuses: &Vec<Status>, twitter_statuses: &Vec<Tweet>) -> StatusUpdates {
    let mut updates = StatusUpdates {
        tweets: Vec::new(),
        toots: Vec::new(),
    };
    let mut tweets = Vec::new();
    for tweet in twitter_statuses {
        // Replace those ugly t.co URLs in the tweet text.
        tweets.push(tweet_unshorten(&tweet));
    }

    let compare_toots = prepare_compare_toots(&mastodon_statuses);

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
        let toot_text = mastodon_strip_tags(&toot.content);
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
fn prepare_compare_toots(mastodon_statuses: &Vec<Status>) -> Vec<String> {
    let mut toots = Vec::new();
    for toot in mastodon_statuses {
        // Prepare the toots to be comparable with tweets.
        let toot_text = mastodon_strip_tags(&toot.content);
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
    let (mut char_count, _) = character_count(&text, 23, 23);
    let re = Regex::new(r"[^\s]+$").unwrap();
    let mut shortened = text.trim().to_string();
    let mut with_link = shortened.clone();

    // Twitter should allow 280 characters, but their counting is unpredictable.
    // Use 40 characters less and hope it works ¯\_(ツ)_/¯
    while char_count > 240 {
        // Remove the last word.
        shortened = re.replace_all(&shortened, "").trim().to_string();
        // Add a link to the toot that has the full text.
        with_link = shortened.clone() + "… " + &toot_url;
        let (new_count, _) = character_count(&with_link, 23, 23);
        char_count = new_count;
    }
    with_link.to_string()
}

fn mastodon_strip_tags(toot_html: &str) -> String {
    let mut replaced = toot_html.to_string();
    replaced = replaced.replace("<br />", "\n");
    replaced = replaced.replace("<br>", "\n");
    replaced = replaced.replace("</p><p>", "\n\n");
    replaced = replaced.replace("<p>", "");
    dissolve::strip_html_tags(&replaced).join("")
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
    registration.register(app).unwrap();;
    let url = registration.authorise().unwrap();
    println!("Click this link to authorize on Mastodon: {}", url);

    let code = console_input("Paste the returned authorization code");
    let mastodon = registration.create_access_token(code.to_string()).unwrap();

    // Save app data for using on the next run.
    let toml = toml::to_string(&*mastodon).unwrap();
    let mut file = File::create("mastodon.toml").unwrap();
    file.write_all(toml.as_bytes()).unwrap();
    mastodon
}

fn mastodon_load_from_config(mut file: File) -> Mastodon {
    let mut config = String::new();
    file.read_to_string(&mut config).unwrap();
    let data: Data = toml::from_str(&config).unwrap();
    Mastodon::from_data(data)
}

#[derive(Debug, Serialize, Deserialize)]
struct TwitterConfig {
    consumer_key: String,
    consumer_secret: String,
    access_token: String,
    access_token_secret: String,
    user_id: u64,
    user_name: String,
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
        } => {
            let twitter_config = TwitterConfig {
                consumer_key: consumer_key,
                consumer_secret: consumer_secret,
                access_token: access_token.key.to_string(),
                access_token_secret: access_token.secret.to_string(),
                user_id: user_id,
                user_name: screen_name,
            };
            // Save app data for using on the next run.
            let toml = toml::to_string(&twitter_config).unwrap();
            let mut file = File::create("twitter.toml").unwrap();
            file.write_all(toml.as_bytes()).unwrap();

            return twitter_config;
        }
        _ => unreachable!(),
    }
}

fn twitter_load_from_config(mut file: File) -> TwitterConfig {
    let mut config = String::new();
    file.read_to_string(&mut config).unwrap();
    toml::from_str(&config).unwrap()
}

fn console_input(prompt: &str) -> String {
    println!("{}: ", prompt);
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line).unwrap();
    line.trim().to_string()
}


#[cfg(test)]
mod tests {
    extern crate chrono;
    extern crate serde_json;

    use super::*;
    use self::chrono::prelude::*;
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
        let long_toot = "test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test";
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
        let long_toot = "test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test test";
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
