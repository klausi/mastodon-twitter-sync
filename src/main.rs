extern crate chrono;
extern crate dissolve;
extern crate egg_mode;
extern crate egg_mode_text;
extern crate mammut;
extern crate regex;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate tokio_core;
extern crate toml;

use egg_mode::tweet::DraftTweet;
use mammut::Mastodon;
use mammut::status_builder::StatusBuilder;
use std::fs::File;
use std::io::prelude::*;
use std::str::FromStr;
use tokio_core::reactor::Core;

use config::*;
use registration::mastodon_register;
use registration::twitter_register;
use sync::*;
use delete_statuses::mastodon_delete_older_statuses;
use delete_statuses::twitter_delete_older_statuses;
use delete_favs::*;

mod config;
mod registration;
mod sync;
mod delete_statuses;
mod delete_favs;

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
                    delete_older_favs: false,
                },
                twitter: twitter_config,
            };

            // Save config for using on the next run.
            let toml = toml::to_string(&config).unwrap();
            let mut file = File::create("mastodon-twitter-sync.toml").unwrap();
            file.write_all(toml.as_bytes()).unwrap();

            config
        }
    };

    let mastodon = Mastodon::from_data(config.mastodon.app);

    let account = mastodon.verify().unwrap();
    let mastodon_statuses = mastodon
        .statuses(u64::from_str(&account.id).unwrap(), false, true, None, None)
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
    let timeline =
        egg_mode::tweet::user_timeline(config.twitter.user_id, false, true, &token, &handle)
            .with_page_size(50);

    let (timeline, first_tweets) = core.run(timeline.start()).unwrap();
    let mut tweets = (*first_tweets).to_vec();
    // We might have only one tweet because of filtering out reply tweets. Fetch
    // some more tweets to make sure we have enough for comparing.
    if tweets.len() < 50 {
        let (_, mut next_tweets) = core.run(timeline.older(None)).unwrap();
        tweets.append(&mut (*next_tweets).to_vec());
    }
    let mut posts = determine_posts(&mastodon_statuses, &tweets);

    posts = filter_posted_before(posts);

    for toot in posts.toots {
        println!("Posting to Mastodon: {}", toot);
        mastodon.new_status(StatusBuilder::new(toot)).unwrap();
    }

    for tweet in posts.tweets {
        println!("Posting to Twitter: {}", tweet);
        core.run(DraftTweet::new(tweet).send(&token, &handle))
            .unwrap();
    }

    // Delete old mastodon statuses if that option is enabled.
    if config.mastodon.delete_older_statuses {
        mastodon_delete_older_statuses(&mastodon, &account);
    }
    if config.twitter.delete_older_statuses {
        twitter_delete_older_statuses(config.twitter.user_id, &token);
    }

    // Delete old mastodon favourites if that option is enabled.
    if config.mastodon.delete_older_favs {
        mastodon_delete_older_favs(&mastodon);
    }
    if config.twitter.delete_older_favs {
        twitter_delete_older_favs(config.twitter.user_id, &token);
    }
}
