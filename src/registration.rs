use anyhow::Result;
use elefren::helpers::cli;
use elefren::scopes::Scopes;
use elefren::{Mastodon, Registration};
use std::io;

use super::*;

pub fn mastodon_register() -> Result<Mastodon> {
    let instance = console_input(
        "Provide the URL of your Mastodon instance, for example https://mastodon.social ",
    )?;
    let registration = Registration::new(instance)
        .client_name("mastodon-twitter-sync")
        .website("https://github.com/klausi/mastodon-twitter-sync")
        .redirect_uris("urn:ietf:wg:oauth:2.0:oob")
        .scopes(Scopes::read_all() | Scopes::write_all())
        .build()?;

    Ok(cli::authenticate(registration)?)
}

pub async fn twitter_register() -> Result<TwitterConfig> {
    println!("Go to https://developer.twitter.com/en/apps/create to create a new Twitter app.");
    println!("Name: Mastodon Twitter Sync (plus something unique, like your name)");
    println!("Description: Synchronizes Tweets and Toots");
    println!("Website: https://github.com/klausi/mastodon-twitter-sync");
    println!("App Usage: This app synchronizes Tweets and Toots between Twitter and my Mastodon instance, for the purpose of keeping the two in sync.");

    let consumer_key = console_input("Paste your consumer key")?;
    let consumer_secret = console_input("Paste your consumer secret")?;

    let con_token = egg_mode::KeyPair::new(consumer_key.clone(), consumer_secret.clone());
    let request_token = egg_mode::auth::request_token(&con_token, "oob").await?;
    println!(
        "Click this link to authorize on Twitter: {}",
        egg_mode::auth::authorize_url(&request_token)
    );
    let pin = console_input("Paste your PIN")?;

    let (token, user_id, screen_name) =
        egg_mode::auth::access_token(con_token, &request_token, pin).await?;

    match token {
        egg_mode::Token::Access {
            access: ref access_token,
            ..
        } => Ok(TwitterConfig {
            consumer_key,
            consumer_secret,
            access_token: access_token.key.to_string(),
            access_token_secret: access_token.secret.to_string(),
            user_id,
            user_name: screen_name,
            delete_older_statuses: false,
            delete_older_favs: false,
            sync_retweets: true,
            sync_hashtag: None,
            sync_prefix: Default::default(),
        }),
        _ => unreachable!(),
    }
}

fn console_input(prompt: &str) -> Result<String> {
    println!("{}: ", prompt);
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}
