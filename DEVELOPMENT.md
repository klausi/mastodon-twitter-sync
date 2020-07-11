# mastodon-twitter-sync development

## Automated Testing

Run `cargo test` to execute the test case to make sure to not break existing functionality.

## Code formatting

Run `cargo fmt` to automatically format all code. You might need to install rustfmt first with `rustup component add rustfmt`.

## Cache files

In order to minimize API calls mastodon-twitter-sync stores post IDs and dates in JSON cache files locally in the same directory where it is executed from.
