# This is for an image based on alpine.

FROM --platform=$BUILDPLATFORM rust:1-alpine AS vendor

ENV USER=root
WORKDIR /code
RUN cargo init

# Fetch all the dependencies without loading the code to have an independant layer
COPY Cargo.lock Cargo.toml /code/
RUN mkdir -p /code/.cargo
RUN cargo vendor >> /code/.cargo/config.toml

FROM rust:1-alpine AS builder

RUN apk add --no-cache musl-dev openssl-dev

ENV USER=root
WORKDIR /code

COPY Cargo.toml /code/Cargo.toml
COPY Cargo.lock /code/Cargo.lock
COPY src /code/src
COPY --from=vendor /code/.cargo /code/.cargo
COPY --from=vendor /code/vendor /code/vendor

RUN cargo build --release --offline

FROM alpine:latest

RUN apk add --no-cache musl-dev

COPY --from=builder /code/target/release/mastodon-twitter-sync /usr/bin/mastodon-twitter-sync

# Use a separate workdir so that users can have a Docker volume with their
# settings file. Cache files will also be written here.
WORKDIR /data

ENTRYPOINT ["/usr/bin/mastodon-twitter-sync"]
