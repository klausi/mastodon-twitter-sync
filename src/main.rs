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

    let mut tweets = Vec::new();
    for tweet in &core.run(timeline.start()).unwrap() {
        // Replace those ugly t.co URLs in the tweet text.
        tweets.push(tweet_unshorten(&tweet));
    }

    'tweets: for tweet in &tweets {
        for toot in &mastodon_statuses {
            let toot_text = mastodon_strip_tags(&toot.content);

            // If the tweet already exists we can stop here and know that we are
            // synced.
            if toot_text == *tweet {
                break 'tweets;
            }
        }
        // The tweet is not on Mastodon yet, let's post it.
        println!("Posting to Mastodon: {}", tweet);
        //mastodon.new_status(StatusBuilder::new(tweet_text)).unwrap();
    }

    'toots: for toot in &mastodon_statuses {
        let toot_text = mastodon_strip_tags(&toot.content);
        for tweet in &tweets {
            // If the toot already exists we can stop here and know that we are
            // synced.
            if toot_text == *tweet {
                break 'toots;
            }
        }
        // Mastodon allows up to 500 characters, so we might need to shorten the
        // toot.
        let shortened = tweet_shorten(toot_text, &toot.url);

        // The tweet is not on Mastodon yet, let's post it.
        println!("Posting to Twitter: {}", shortened);
        /*core.run(DraftTweet::new(&shortened).send(&token, &handle))
            .unwrap();*/
    }
}

fn tweet_unshorten(tweet: &Tweet) -> String {
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
    tweet_text
}

fn tweet_shorten(text: String, toot_url: &str) -> String {
    let (mut char_count, _) = character_count(&text, 23, 23);
    if char_count < 280 {
        return text;
    }
    let re = Regex::new(r"[^\s]+$").unwrap();
    let mut shortened = text.trim().to_string();
    let mut with_link = shortened.clone();
    while char_count > 280 {
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
