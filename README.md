# Mastodon Twitter Sync

[![Build Status](https://travis-ci.org/klausi/mastodon-twitter-sync.svg?branch=master)](https://travis-ci.org/klausi/mastodon-twitter-sync)

This tool synchronizes posts from [Mastodon](https://joinmastodon.org/) to [Twitter](https://twitter.com/) and back. It does not matter where you post your stuff - it will get synchronized to the other!

## Synchronization Features

* Your status update on Twitter will be posted automatically to Mastodon
* Your Retweet on Twitter will automatically be posted to Mastodon with a "RT username:" prefix
* Your status update on Mastodon will be posted automatically to Twitter
* Your boost on Mastodon will be posted automatically to Twitter with a "RT username:" prefix

## Old data deletion feature for better privacy

Optionally configuration options can be set to delete posts/favourites from your Mastodon and Twitter accounts that are older than 90 days.

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

Enable automatic status/favourite deletion with config options. Example:

```toml
[mastodon]
# Delete Mastodon status posts that are older than 90 days
delete_older_statuses = true
# Delete Mastodon favourites that are older than 90 days
delete_older_favs = true
# Also sync reblogs (boosts).
sync_reblogs = true
# Restrict sync to a hashtag (leave empty to sync all posts)
sync_hashtag = "#sync"

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
# Delete Twitter status posts that are older than 90 days
delete_older_statuses = true
# Delete Twitter likes that are older than 90 days
delete_older_favs = true
# Also sync retweets.
sync_retweets = true
# Restrict sync to a hashtag (leave empty to sync all posts)
sync_hashtag = "#sync"
```

## Preview what's going to be synced

You can preview what's going to be synced using the `--dry-run` option:

    cargo run --release -- --dry-run

This is running a sync without actually posting or deleting anything.

## Skip existing posts and only sync new posts

If you already have posts in one or both of your accounts and you want to exclude them from being synced you can use `--skip-existing-posts`. This is going to mark all posts as synced without actually posting them.

    cargo run --release -- --skip-existing-posts

Note that combining `--skip-existing-posts --dry-run` will not do anything. You have to run `--skip-existing-posts` alone to mark all posts as synchronized in the post cache.

## Periodic execution

Every run of the program only synchronizes the accounts once. Use Cron to run it periodically, recommended every 10 minutes:

```
*/10 * * * *   cd /home/klausi/workspace/mastodon-twitter-sync && ./target/release/mastodon-twitter-sync
```
