FROM rust:alpine

RUN mkdir -p /mastodon-twitter-sync/src

ADD Cargo.lock /mastodon-twitter-sync
ADD Cargo.toml /mastodon-twitter-sync
ADD src/* /mastodon-twitter-sync/src/

RUN true \
    && apk add --no-cache openssh openssh-client protoc musl-dev libressl-dev openssl-dev \
    && mkdir -p -m 0600 ~/.ssh \
    && ssh-keyscan github.com >> ~/.ssh/known_hosts \
    && cd /mastodon-twitter-sync \
    && /usr/local/cargo/bin/cargo build --release \
    && mv /mastodon-twitter-sync/target/release/mastodon-twitter-sync /usr/local/bin/ \
    && rm -rf /mastodon-twitter-sync \
    && echo "Done"

WORKDIR /data

ENTRYPOINT [ "/usr/local/bin/mastodon-twitter-sync" ]
