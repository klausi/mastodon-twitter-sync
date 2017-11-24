#![cfg(test)]

use super::*;
use egg_mode::tweet::{TweetEntities, TweetSource};

#[test]
fn tweet_shortening() {
    let toot = "#MASTODON POST PRIVACY - who can see your post?

PUBLIC 🌏 Anyone can see and boost your post everywhere.

UNLISTED 🔓 ✅ Tagged people
✅ Followers
✅ People who look for it
❌ Local and federated timelines
✅ Boostable

FOLLOWERS ONLY 🔐 ✅ Tagged people
✅ Followers
❌ People who look for it
❌ Local and federated timelines
❌ Boostable

DIRECT MESSAGE ✉️
✅ Tagged people
❌ Followers
❌ People who look for it
❌ Local and federated timelines
❌ Boostable

https://cybre.space/media/J-amFmXPvb_Mt7toGgs #tutorial #howto
";
    let shortened_for_twitter =
        tweet_shorten(toot, "https://mastodon.social/@klausi/98999025586548863");
    assert_eq!(
        shortened_for_twitter,
        "#MASTODON POST PRIVACY - who can see your post?

PUBLIC 🌏 Anyone can see and boost your post everywhere.

UNLISTED 🔓 ✅ Tagged people
✅ Followers
✅ People who look for it
❌ Local and federated timelines
✅ Boostable… https://mastodon.social/@klausi/98999025586548863"
    );
}

// Test that if a long Mastodon toot already exists as short version on
// Twitter that it is not posted again.
#[test]
fn short_version_on_twitter() {
    let mut status = get_mastodon_status();
    let long_toot = "test test test test test test test test test test test test test
        test test test test test test test test test test test test test
        test test test test test test test test test test test test test
        test test test test test test test test test test test test test
        test test test test";
    status.content = long_toot.to_string();

    let mut tweet = get_twitter_status();
    tweet.text = tweet_shorten(long_toot, &status.url);

    let tweets = vec![tweet];
    let statuses = vec![status];
    let posts = determine_posts(&statuses, &tweets);
    assert!(posts.toots.is_empty());
    assert!(posts.tweets.is_empty());
}

// Test an over long post of 280 characters that is the exact same on both
// Mastodon and Twitter. No sync work necessary.
#[test]
fn over_long_status_on_both() {
    let mut status = get_mastodon_status();
    let long_toot = "test test test test test test test test test test test test test
        test test test test test test test test test test test test test
        test test test test test test test test test test test test test
        test test test test test test test test test test test test test
        test test test test";
    status.content = long_toot.to_string();

    let mut tweet = get_twitter_status();
    tweet.text = long_toot.to_string();

    let tweets = vec![tweet];
    let statuses = vec![status];
    let posts = determine_posts(&statuses, &tweets);
    assert!(posts.toots.is_empty());
    assert!(posts.tweets.is_empty());
}

// Test that Mastodon status text is posted HTML entity decoded to Twitter.
// &amp; => &
#[test]
fn mastodon_html_decode() {
    let mut status = get_mastodon_status();
    status.content = "<p>You &amp; me!</p>".to_string();
    let posts = determine_posts(&vec![status], &Vec::new());
    assert_eq!(posts.tweets[0], "You & me!");
}

// Test that Twitter status text is posted HTML entity decoded to Mastodon.
// &amp; => &
#[test]
fn twitter_html_decode() {
    let mut status = get_twitter_status();
    status.text = "You &amp; me!".to_string();
    let posts = determine_posts(&Vec::new(), &vec![status]);
    assert_eq!(posts.toots[0], "You & me!");
}

// Test that a boost on Mastodon is prefixed with "RT username:" when posted
// to Twitter.
#[test]
fn mastodon_boost() {
    let mut reblog = get_mastodon_status();
    reblog.content = "<p>Some example toooot!</p>".to_string();
    let mut status = get_mastodon_status();
    status.reblog = Some(Box::new(reblog));
    status.reblogged = Some(true);

    let posts = determine_posts(&vec![status], &Vec::new());
    assert_eq!(posts.tweets[0], "RT example: Some example toooot!");
}

// Test that the old "RT @username" prefix is considered equal to "RT
// username:".
#[test]
fn old_rt_prefix() {
    let mut reblog = get_mastodon_status();
    reblog.content = "<p>Some example toooot!</p>".to_string();
    let mut status = get_mastodon_status();
    status.reblog = Some(Box::new(reblog));
    status.reblogged = Some(true);

    let mut tweet = get_twitter_status();
    tweet.text = "RT @example: Some example toooot!".to_string();

    let tweets = vec![tweet];
    let statuses = vec![status];
    let posts = determine_posts(&statuses, &tweets);
    assert!(posts.toots.is_empty());
    assert!(posts.tweets.is_empty());
}

fn get_mastodon_status() -> Status {
    let json = {
        let mut file = File::open("src/mastodon_status.json").unwrap();
        let mut ret = String::new();
        file.read_to_string(&mut ret).unwrap();
        ret
    };
    let status: Status = serde_json::from_str(&json).unwrap();
    status
}

fn get_twitter_status() -> Tweet {
    Tweet {
        coordinates: None,
        created_at: Utc::now(),
        current_user_retweet: None,
        display_text_range: None,
        entities: TweetEntities {
            hashtags: Vec::new(),
            symbols: Vec::new(),
            urls: Vec::new(),
            user_mentions: Vec::new(),
            media: None,
        },
        extended_entities: None,
        favorite_count: 0,
        favorited: None,
        id: 123456,
        in_reply_to_user_id: None,
        in_reply_to_screen_name: None,
        in_reply_to_status_id: None,
        lang: "".to_string(),
        place: None,
        possibly_sensitive: None,
        quoted_status_id: None,
        quoted_status: None,
        retweet_count: 0,
        retweeted: None,
        retweeted_status: None,
        source: TweetSource {
            name: "".to_string(),
            url: "".to_string(),
        },
        text: "".to_string(),
        truncated: false,
        user: None,
        withheld_copyright: false,
        withheld_in_countries: None,
        withheld_scope: None,
    }
}
