# Periodic execution with Github Actions Cron

We can leverage Github Actions to periodically execute Mastodon Twitter Sync for free. This works with a public Github repository using an environment secret that decrypts your API key settings.

## Security and data protection warnings

1. Please be aware that you need to trust Github/Microsoft that they will not read, decrypt and abuse your Mastodon and Twitter API keys.
2. The Github Actions output will contain debug information about your account activity like the posts that get synchronized or the IDs of posts that you favored.

If you run a high-profile account or are at risk of abuse please do not use Github Actions to synchronize your posts.

## Setup steps

1. Run `mastodon-twitter-sync` locally at least one time so that your `mastodon-twitter-sync.toml` configuration file is populated with your API keys.
1. Create a public Github repository with the name `<USERNAME>/mts-cron`. Example: https://github.com/klausi/mts-cron
1. Clone your new repository and copy the contents of the `github-cron` folder in the `mastodon-twitter-sync` repository to your checkout. Example:

```
git clone git@github.com:<USERNAME>/mts-cron.git
cd mts-cron
cp -r /path/to/mastodon-twitter-sync/github-cron/* .
```

4. Generate a secure passphrase that will be used to decrypt your settings in the Github Actions environment. Example:

```
pwgen 30
```

5. Encrypt your `mastodon-twitter-sync.toml` configuration file with GPG using the generated secure passphrase. Example:

```
gpg -c --cipher-algo AES256 /path/to/mastodon-twitter-sync/mastodon-twitter-sync.toml

```
