use mammut::{Mastodon, StatusesRequest};
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::process;
use structopt::StructOpt;

use log::debug;

use crate::args::*;
use crate::config::*;
use crate::delete_favs::*;
use crate::delete_statuses::mastodon_delete_older_statuses;
use crate::delete_statuses::twitter_delete_older_statuses;
use crate::errors::*;
use crate::post::*;
use crate::registration::mastodon_register;
use crate::registration::twitter_register;
use crate::sync::*;

mod args;
mod config;
mod delete_favs;
mod delete_statuses;
mod errors;
mod post;
mod registration;
mod sync;

async fn run() -> Result<()> {
    env_logger::init();

    let args = Args::from_args();
    debug!("running with args {:?}", args);

    let config = match fs::read_to_string(&args.config) {
        Ok(config) => config_load(&config)?,
        Err(_) => {
            let mastodon = mastodon_register().context("Failed to setup mastodon account")?;
            let twitter_config = twitter_register()
                .await
                .context("Failed to setup twitter account")?;
            let config = Config {
                mastodon: MastodonConfig {
                    app: (*mastodon).clone(),
                    // Do not delete older status per default, users should
                    // enable this explicitly.
                    delete_older_statuses: false,
                    delete_older_favs: false,
                    sync_reblogs: true,
                },
                twitter: twitter_config,
            };

            // Save config for using on the next run.
            let toml = toml::to_string(&config)?;
            let mut file = File::create(&args.config).context("Failed to create config file")?;
            file.write_all(toml.as_bytes())?;

            config
        }
    };

    let mastodon = Mastodon::from_data(config.mastodon.app);

    let account = match mastodon.verify_credentials() {
        Ok(account) => account,
        Err(e) => {
            println!("Error connecting to Mastodon: {:#?}", e);
            process::exit(1);
        }
    };
    // Get most recent toots but without replies.
    let mastodon_statuses =
        match mastodon.statuses(&account.id, StatusesRequest::new().exclude_replies()) {
            Ok(statuses) => statuses.initial_items,
            Err(e) => {
                println!("Error fetching toots from Mastodon: {:#?}", e);
                process::exit(2);
            }
        };

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

    let timeline = egg_mode::tweet::user_timeline(config.twitter.user_id, false, true, &token)
        .with_page_size(50);

    let (timeline, first_tweets) = match timeline.start().await {
        Ok(tweets) => tweets,
        Err(e) => {
            println!("Error fetching tweets from Twitter: {:#?}", e);
            process::exit(3);
        }
    };
    let mut tweets = (*first_tweets).to_vec();
    // We might have only one tweet because of filtering out reply tweets. Fetch
    // some more tweets to make sure we have enough for comparing.
    if tweets.len() < 50 {
        let (_, mut next_tweets) = match timeline.older(None).await {
            Ok(tweets) => tweets,
            Err(e) => {
                println!("Error fetching older tweets from Twitter: {:#?}", e);
                process::exit(4);
            }
        };
        tweets.append(&mut (*next_tweets).to_vec());
    }

    let options = SyncOptions {
        sync_reblogs: config.mastodon.sync_reblogs,
        sync_retweets: config.twitter.sync_retweets,
    };

    let mut posts = determine_posts(&mastodon_statuses, &tweets, &options);

    // Prevent double posting with a post cache that records each new status
    // message.
    let post_cache_file = "post_cache.json";
    let mut post_cache = read_post_cache(post_cache_file);
    let mut cache_changed = false;
    posts = filter_posted_before(posts, &post_cache)?;

    for toot in posts.toots {
        if !args.skip_existing_posts {
            println!("Posting to Mastodon: {}", toot.text);
            if !args.dry_run {
                if let Err(e) = post_to_mastodon(&mastodon, &toot).await {
                    println!("Error posting toot to Mastodon: {:#?}", e);
                    process::exit(5);
                }
            }
        }
        // Posting API call was successful: store text in cache to prevent any
        // double posting next time.
        post_cache.insert(toot.text);
        cache_changed = true;
    }

    for tweet in posts.tweets {
        if !args.skip_existing_posts {
            println!("Posting to Twitter: {}", tweet.text);
            if !args.dry_run {
                if let Err(e) = post_to_twitter(&token, &tweet).await {
                    println!("Error posting tweet to Twitter: {:#?}", e);
                    process::exit(6);
                }
            }
        }
        // Posting API call was successful: store text in cache to prevent any
        // double posting next time.
        post_cache.insert(tweet.text);
        cache_changed = true;
    }

    // Write out the cache file if necessary.
    if !args.dry_run && cache_changed {
        let json = serde_json::to_string_pretty(&post_cache)?;
        fs::write(post_cache_file, json.as_bytes())?;
    }

    // Delete old mastodon statuses if that option is enabled.
    if config.mastodon.delete_older_statuses {
        mastodon_delete_older_statuses(&mastodon, &account, args.dry_run)
            .context("Failed to delete old mastodon statuses")?;
    }
    if config.twitter.delete_older_statuses {
        twitter_delete_older_statuses(config.twitter.user_id, &token, args.dry_run)
            .await
            .context("Failed to delete old twitter statuses")?;
    }

    // Delete old mastodon favourites if that option is enabled.
    if config.mastodon.delete_older_favs {
        mastodon_delete_older_favs(&mastodon, args.dry_run)
            .context("Failed to delete old mastodon favs")?;
    }
    if config.twitter.delete_older_favs {
        twitter_delete_older_favs(config.twitter.user_id, &token, args.dry_run)
            .await
            .context("Failed to delete old twitter favs")?;
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("Error: {}", err);
        for cause in err.iter_chain().skip(1) {
            eprintln!("Because: {}", cause);
        }
        std::process::exit(1);
    }
}
