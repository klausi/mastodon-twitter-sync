# Installation of Mastodon Twitter Sync

There are 4 options how to run mastodon-twitter-sync:

1. Recommended: Precompiled executable binaries from the [release page](https://github.com/klausi/mastodon-twitter-sync/releases) (If the binaries do not work on your system you will have to use Docker or compile the program yourself.)
2. Docker
3. Compiling yourself (takes a bit of time with the Rust compiler)
4. Packages for Linux distributions

## Option 1: Precompiled binaries (recommended)

Download the executable archive for your operating system platform from the release page and run it in an directory where the configuration and cache files will be stored.

```
mkdir mastodon-twitter-sync
cd mastodon-twitter-sync
tar xzf /path/to/downloaded/mastodon-twitter-sync-x86_64-unknown-linux-gnu.tar.gz
./mastodon-twitter-sync
```

Follow the text instructions to enter API keys.

## Option 2: Installing with Docker

You need to have Docker installed on your system, then you can use the [published Docker image](https://hub.docker.com/r/klausi/mastodon-twitter-sync).

The following commands create a directory where the settings file and cache files will be stored. Then we use a Docker volume from that directory to store them persistently.

```
mkdir mastodon-twitter-sync
cd mastodon-twitter-sync
docker run -it --rm -v "$(pwd)":/data klausi/mastodon-twitter-sync
```

Follow the text instructions to enter API keys.

Use that Docker command as a replacement for `./mastodon-twitter-sync` in the examples in this README.

## Option 3: Compiling with cargo

This will install Rust and setup API access to Mastodon and Twitter. Follow the text instructions to enter API keys.

```
curl https://sh.rustup.rs -sSf | sh
source ~/.cargo/env
git clone https://github.com/klausi/mastodon-twitter-sync.git
cd mastodon-twitter-sync
cargo run --release
```

Follow the text instructions to enter API keys.

Use the `cargo run --release --` command as a replacement for `./mastodon-twitter-sync` in the examples in this README.

## Option 4: Packages for Linux distributions

<details>
  <summary>Packaging status</summary>

[![Packaging status](https://repology.org/badge/vertical-allrepos/mastodon-twitter-sync.svg)](https://repology.org/project/mastodon-twitter-sync/versions)

</details>

If your distribution has packaged `mastodon-twitter-sync`, you can use that package for the installation.

### Arch Linux

You can use [pacman](https://wiki.archlinux.org/title/Pacman) to install from the [community repository](https://archlinux.org/packages/community/x86_64/mastodon-twitter-sync/):

```
pacman -S mastodon-twitter-sync
```
