# We get segmentation faults with the Alpine Rust image, so we use the bigger
# default one.
FROM rust:latest

# Only if the Rust source files change do we need to recompile and invalidate
# the Docker cache.
WORKDIR /usr/src/mastodon-twitter-sync
COPY src src
COPY Cargo* ./

RUN cargo install --path .

# Use a separate workdir so that users can have a Docker volume with their
# settings file. Cache files will also be written here.
WORKDIR /data

ENTRYPOINT ["mastodon-twitter-sync"]
