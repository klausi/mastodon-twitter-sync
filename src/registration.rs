extern crate egg_mode;
extern crate mammut;
extern crate tokio_core;

use mammut::apps::{AppBuilder, Scopes};
use mammut::{Mastodon, Registration};
use std::io;
use tokio_core::reactor::Core;

use super::*;

pub fn mastodon_register() -> Mastodon {
    let app = AppBuilder {
        client_name: "mastodon-twitter-sync",
        redirect_uris: "urn:ietf:wg:oauth:2.0:oob",
        scopes: Scopes::ReadWrite,
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

pub fn twitter_register() -> TwitterConfig {
    println!("Go to https://apps.twitter.com/app/new to create a new Twitter app.");
    println!("Name: Mastodon Twitter Sync");
    println!("Description: Synchronizes Tweets and Toots");
    println!("Website: https://github.com/klausi/mastodon-twitter-sync");

    let consumer_key = console_input("Paste your consumer key");
    let consumer_secret = console_input("Paste your consumer secret");

    let mut core = Core::new().unwrap();

    let con_token = egg_mode::KeyPair::new(consumer_key.clone(), consumer_secret.clone());
    let request_token = core
        .run(egg_mode::request_token(&con_token, "oob"))
        .unwrap();
    println!(
        "Click this link to authorize on Twitter: {}",
        egg_mode::authorize_url(&request_token)
    );
    let pin = console_input("Paste your PIN");

    let (token, user_id, screen_name) = core
        .run(egg_mode::access_token(con_token, &request_token, pin))
        .unwrap();

    match token {
        egg_mode::Token::Access {
            access: ref access_token,
            ..
        } => TwitterConfig {
            consumer_key,
            consumer_secret,
            access_token: access_token.key.to_string(),
            access_token_secret: access_token.secret.to_string(),
            user_id,
            user_name: screen_name,
            delete_older_statuses: false,
            delete_older_favs: false,
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
