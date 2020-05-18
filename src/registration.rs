use crate::errors::*;
use mammut::apps::{AppBuilder, Scopes};
use mammut::{Mastodon, Registration};
use std::io;

use super::*;

pub fn mastodon_register() -> Result<Mastodon> {
    let app = AppBuilder {
        client_name: "mastodon-twitter-sync",
        redirect_uris: "urn:ietf:wg:oauth:2.0:oob",
        scopes: Scopes::ReadWrite,
        website: Some("https://github.com/klausi/mastodon-twitter-sync"),
    };

    let instance = console_input(
        "Provide the URL of your Mastodon instance, for example https://mastodon.social ",
    )?;
    let mut registration = Registration::new(instance);
    registration.register(app)?;
    let url = registration.authorise()?;
    println!("Click this link to authorize on Mastodon: {}", url);

    let code = console_input("Paste the returned authorization code")?;
    let access_token = registration.create_access_token(code)?;
    Ok(access_token)
}

pub async fn twitter_register() -> Result<TwitterConfig> {
    println!("Go to https://developer.twitter.com/en/apps/create to create a new Twitter app.");
    println!("Name: Mastodon Twitter Sync");
    println!("Description: Synchronizes Tweets and Toots");
    println!("Website: https://github.com/klausi/mastodon-twitter-sync");

    let consumer_key = console_input("Paste your consumer key")?;
    let consumer_secret = console_input("Paste your consumer secret")?;

    let con_token = egg_mode::KeyPair::new(consumer_key.clone(), consumer_secret.clone());
    let request_token = egg_mode::request_token(&con_token, "oob").await?;
    println!(
        "Click this link to authorize on Twitter: {}",
        egg_mode::authorize_url(&request_token)
    );
    let pin = console_input("Paste your PIN")?;

    let (token, user_id, screen_name) =
        egg_mode::access_token(con_token, &request_token, pin).await?;

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
            sync_hashtag: "".to_string(),
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
