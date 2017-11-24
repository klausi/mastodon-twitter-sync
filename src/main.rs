extern crate chrono;
extern crate egg_mode;
extern crate mammut;
extern crate regex;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate tokio_core;
extern crate toml;

use egg_mode::tweet::DraftTweet;
use mammut::{Data, Mastodon, Registration};
use mammut::apps::{AppBuilder, Scope};
use mammut::status_builder::StatusBuilder;
use std::io;
use std::fs::File;
use std::io::prelude::*;
use tokio_core::reactor::Core;

use sync::determine_posts;
use delete_statuses::mastodon_delete_older_statuses;
use delete_statuses::twitter_delete_older_statuses;

mod sync;
mod delete_statuses;

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
        mastodon_delete_older_statuses(&mastodon, &account);
    }
    if config.twitter.delete_older_statuses {
        twitter_delete_older_statuses(config.twitter.user_id, &token);
    }
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
    #[serde(default = "twitter_config_delete_default")] delete_older_statuses: bool,
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
