# Mastodon Twitter Sync

This tool synchronizes posts from [Mastodon](https://joinmastodon.org/) to [Twitter](https://twitter.com/) and back. It does not matter where you post your stuff - it will get synchronized to the other!

## Synchronization Features

* Your status update on Twitter will be posted automatically to Mastodon
* Your Retweet on Twitter will automatically be posted to Mastodon with a "RT @username" prefix
* Your status update on Mastodon will be posted automatically to Twitter
* Your boost on Mastodon will be posted automatically to Mastodon with a "RT @username" prefix

## Old status deletion feature

Optionally a configuration option can be set to delete posts from your Mastodon account that are older than 90 days.

## Installation and execution

This will install Rust and setup API access to Mastodon and Twitter. Follow the text instructions to enter API keys.

```
curl https://sh.rustup.rs -sSf | sh
source ~/.cargo/env
git clone https://github.com/klausi/mastodon-twitter-sync.git
cd mastodon-twitter-sync
cargo run --release
```

## Configuration

All configuration options are created in a `mastodon-twitter-sync.toml` file in the directory where you executed the program.

Enable automatic post deletion after 90 days by setting `delete_older_statuses = true` in the mastodon section. Example:

```toml
[mastodon]
delete_older_statuses = true
[mastodon.app]
base = "https://mastodon.social"
client_id = "XXXXXXXXXXX"
client_secret = "XXXXXXXXXXX"
redirect = "urn:ietf:wg:oauth:2.0:oob"
token = "XXXXXXXXXXX"
[twitter]
consumer_key = "XXXXXXXXXXX"
consumer_secret = "XXXXXXXXXXX"
access_token = "XXXXXXXXXXX"
access_token_secret = "XXXXXXXXXXX"
user_id = 1234567890
user_name = "example"
```

## Periodic execution

Every run of the program only synchronizes the accounts once. Use Cron to run it periodically, recommended every 10 minutes:

```
*/10 * * * *   cd /home/klausi/workspace/mastodon-twitter-sync && ./target/release/mastodon-twitter-sync
```
