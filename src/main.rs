extern crate mammut;
extern crate toml;
use mammut::{Data, Mastodon, Registration};
use mammut::apps::{AppBuilder, Scope};
use std::io;
use std::fs::File;
use std::io::prelude::*;

fn main() {
    let mastodon = match File::open("mastodon-twitter-sync.toml") {
        Ok(f) => load_from_config(f),
        Err(_) => register(),
    };

    let account = mastodon.verify();
    println!("{:?}", account);
}

fn register() -> Mastodon {
    let app = AppBuilder {
        client_name: "mastodon-twitter-sync",
        redirect_uris: "urn:ietf:wg:oauth:2.0:oob",
        scopes: Scope::Read,
        website: None,
    };

    let mut registration = Registration::new("https://mastodon.social");
    registration.register(app).unwrap();;
    let url = registration.authorise().unwrap();
    println!("Click this link to authorize on Mastodon: {}", url);
    println!("Paste the returned authorization code: ");
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();

    let code = input.trim();
    let mastodon = registration.create_access_token(code.to_string()).unwrap();

    // Save app data for using on the next run.
    let toml = toml::to_string(&*mastodon).unwrap();
    let mut file = File::create("mastodon-twitter-sync.toml").unwrap();
    file.write_all(toml.as_bytes()).unwrap();
    mastodon
}

fn load_from_config(mut file: File) -> Mastodon {
    let mut config = String::new();
    file.read_to_string(&mut config).unwrap();
    let data: Data = toml::from_str(&config).unwrap();
    Mastodon::from_data(data)
}
