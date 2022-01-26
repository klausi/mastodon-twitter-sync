FROM debian:10

RUN true \
    && apt-get update \
    && apt-get install git curl openssh-client build-essential libssl-dev pkg-config -y \
    && mkdir -p -m 0600 ~/.ssh \
    && ssh-keyscan github.com >> ~/.ssh/known_hosts \
    && curl https://sh.rustup.rs -o rustup \
    && chmod +x rustup \
    && ./rustup -y \
    && git clone https://github.com/klausi/mastodon-twitter-sync.git \
    && cd /mastodon-twitter-sync \
    && ~/.cargo/bin/cargo build --release

RUN true \
    && mv /mastodon-twitter-sync/target/release/mastodon-twitter-sync /usr/local/bin/ \
    && rm -rf /mastodon-twitter-sync \
    && rm -rf /root/.rustup \
    && rm -rf /root/.cargo \
    && rm -rf /root/.ssh \
    && rm -rf /root/.bashrc \
    && rm -rf /root/.profile

WORKDIR /data

ENTRYPOINT [ "/usr/local/bin/mastodon-twitter-sync" ]
