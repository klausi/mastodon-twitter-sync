#!/bin/bash

set -x

cargo lambda build --package mastodon-twitter-sync-aws --release --arm64
cp target/lambda/mastodon-twitter-sync-aws/bootstrap .
zip lambda.zip bootstrap mastodon-twitter-sync.toml
