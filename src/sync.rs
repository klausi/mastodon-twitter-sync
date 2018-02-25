extern crate dissolve;
extern crate serde_json;

use egg_mode_text::character_count;
use egg_mode::tweet::Tweet;
use mammut::entities::status::Status;
use regex::Regex;
use std::collections::HashSet;
use std::fs::File;
use std::io::prelude::*;

// Represents new status updates that should be posted to Twitter (tweets) and
// Mastodon (toots).
#[derive(Debug, Clone)]
pub struct StatusUpdates {
    pub tweets: Vec<String>,
    pub toots: Vec<String>,
}

pub fn determine_posts(mastodon_statuses: &[Status], twitter_statuses: &[Tweet]) -> StatusUpdates {
    let mut updates = StatusUpdates {
        tweets: Vec::new(),
        toots: Vec::new(),
    };
    'tweets: for tweet in twitter_statuses {
        for toot in mastodon_statuses {
            // If the tweet already exists we can stop here and know that we are
            // synced.
            if toot_and_tweet_are_equal(toot, tweet) {
                break 'tweets;
            }
        }
        // The tweet is not on Mastodon yet, let's post it.
        updates.toots.push(tweet_unshorten_decode(tweet));
    }

    'toots: for toot in mastodon_statuses {
        let post = tweet_shorten(&mastodon_toot_get_text(toot), &toot.url);
        // Skip direct toots to other Mastodon users, even if they are public.
        if post.starts_with('@') {
            continue;
        }

        for tweet in twitter_statuses {
            // If the toot already exists we can stop here and know that we are
            // synced.
            if toot_and_tweet_are_equal(toot, tweet) {
                break 'toots;
            }
        }
        // The toot is not on Twitter yet, let's post it.
        updates.tweets.push(post);
    }
    updates
}

// Returns true if a Mastodon toot and a Twitter tweet are considered equal.
fn toot_and_tweet_are_equal(toot: &Status, tweet: &Tweet) -> bool {
    // Strip markup from Mastodon toot.
    let toot_text = mastodon_toot_get_text(toot);
    // Replace those ugly t.co URLs in the tweet text.
    let tweet_text = tweet_unshorten_decode(tweet);
    if toot_text.to_lowercase() == tweet_text.to_lowercase() {
        return true;
    }
    // Mastodon allows up to 500 characters, so we might need to shorten the
    // toot.
    let shortened_toot = tweet_shorten(&toot_text, &toot.url);
    if shortened_toot.to_lowercase() == tweet_text.to_lowercase() {
        return true;
    }
    // Support for old posts that started with "RT @username:", we consider them
    // equal to "RT username:".
    if tweet_text.starts_with("RT @") {
        let old_rt = tweet_text.replacen("RT @", "RT ", 1);
        if old_rt.to_lowercase() == toot_text.to_lowercase()
            || old_rt.to_lowercase() == shortened_toot.to_lowercase()
        {
            return true;
        }
    }
    if toot_text.starts_with("RT @") {
        let old_rt = toot_text.replacen("RT @", "RT ", 1);
        if old_rt.to_lowercase() == tweet_text.to_lowercase() {
            return true;
        }
    }
    if shortened_toot.starts_with("RT @") {
        let old_rt = shortened_toot.replacen("RT @", "RT ", 1);
        if old_rt.to_lowercase() == tweet_text.to_lowercase() {
            return true;
        }
    }

    false
}

// Replace t.co URLs and HTML entity decode &amp;
fn tweet_unshorten_decode(tweet: &Tweet) -> String {
    let (mut tweet_text, urls) = match tweet.retweeted_status {
        None => (tweet.text.clone(), &tweet.entities.urls),
        Some(ref retweet) => (
            format!(
                "RT {}: {}",
                retweet.clone().user.unwrap().screen_name,
                retweet.text
            ),
            &retweet.entities.urls,
        ),
    };
    for url in urls {
        tweet_text = tweet_text.replace(&url.url, &url.expanded_url);
    }
    // Twitterposts have HTML entities such as &amp;, we need to decode them.
    dissolve::strip_html_tags(&tweet_text).join("")
}

fn tweet_shorten(text: &str, toot_url: &Option<String>) -> String {
    let mut char_count = character_count(text, 23, 23);
    let re = Regex::new(r"[^\s]+$").unwrap();
    let mut shortened = text.trim().to_string();
    let mut with_link = shortened.clone();

    // Twitter should allow 280 characters, but their counting is unpredictable.
    // Use 40 characters less and hope it works ¯\_(ツ)_/¯
    while char_count > 240 {
        // Remove the last word.
        shortened = re.replace_all(&shortened, "").trim().to_string();
        if let Some(ref toot_url) = *toot_url {
            // Add a link to the toot that has the full text.
            with_link = shortened.clone() + "… " + toot_url;
        } else {
            with_link = shortened.clone();
        }
        let new_count = character_count(&with_link, 23, 23);
        char_count = new_count;
    }
    with_link.to_string()
}

// Prefix boost toots with the author and strip HTML tags.
fn mastodon_toot_get_text(toot: &Status) -> String {
    let mut replaced = match toot.reblog {
        None => toot.content.clone(),
        Some(ref reblog) => format!("RT {}: {}", reblog.account.username, reblog.content),
    };
    replaced = replaced.replace("<br />", "\n");
    replaced = replaced.replace("<br>", "\n");
    replaced = replaced.replace("</p><p>", "\n\n");
    replaced = replaced.replace("<p>", "");
    dissolve::strip_html_tags(&replaced).join("")
}

// Ensure that sync posts have not been made before to prevent syncing loops.
// Use a cache file to temporarily store posts and compare them on the next
// invocation.
pub fn filter_posted_before(posts: StatusUpdates) -> StatusUpdates {
    // If there are not status updates then we don't need to check anything.
    if posts.toots.is_empty() && posts.tweets.is_empty() {
        return posts;
    }

    let cache_file = "post_cache.json";
    let cache: HashSet<String> = match File::open(cache_file) {
        Ok(mut file) => {
            let mut json = String::new();
            file.read_to_string(&mut json).unwrap();
            serde_json::from_str(&json).unwrap()
        }
        Err(_) => HashSet::new(),
    };
    let mut filtered_posts = StatusUpdates {
        tweets: Vec::new(),
        toots: Vec::new(),
    };
    let mut write_cache = cache.clone();
    for tweet in posts.tweets {
        if cache.contains(&tweet) {
            println!("Error: preventing double posting to Twitter: {}", tweet);
        } else {
            filtered_posts.tweets.push(tweet.clone());
            write_cache.insert(tweet);
        }
    }
    for toot in posts.toots {
        if cache.contains(&toot) {
            println!("Error: preventing double posting to Mastodon: {}", toot);
        } else {
            filtered_posts.toots.push(toot.clone());
            write_cache.insert(toot);
        }
    }

    let json = serde_json::to_string(&write_cache).unwrap();
    let mut file = File::create(cache_file).unwrap();
    file.write_all(json.as_bytes()).unwrap();

    filtered_posts
}

#[cfg(test)]
mod tests {

    use super::*;
    use chrono::Utc;
    use egg_mode::tweet::{TweetEntities, TweetSource};
    use std::fs::File;

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
        let shortened_for_twitter = tweet_shorten(
            toot,
            &Some("https://mastodon.social/@klausi/98999025586548863".to_string()),
        );
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

    // Test that the tweet/toot comparison is not case sensitive.
    #[test]
    fn case_insensitive() {
        let mut status = get_mastodon_status();
        status.content = "Casing different @Yes".to_string();
        let mut tweet = get_twitter_status();
        tweet.text = "casing Different @yes".to_string();
        assert!(toot_and_tweet_are_equal(&status, &tweet));

        let long_toot = "Test test test test test test test test test test test test test
        test test test test test test test test test test test test test
        test test test test test test test test test test test test test
        test test test test test test test test test test test test test
        test test test test";
        status.content = long_toot.to_string();
        tweet.text = tweet_shorten(long_toot, &status.url).to_lowercase();
        assert!(toot_and_tweet_are_equal(&status, &tweet));
    }

    // Test that direct toots starting with "@" are not copied to twitter.
    #[test]
    fn direct_toot() {
        let mut status = get_mastodon_status();
        status.content = "@Test Hello! http://example.com".to_string();
        let tweets = Vec::new();
        let statuses = vec![status];
        let posts = determine_posts(&statuses, &tweets);
        assert!(posts.toots.is_empty());
        assert!(posts.tweets.is_empty());
    }

    // Test that toots starting with umlauts like Ö do not panic.
    #[test]
    fn umlaut_toot() {
        let mut status = get_mastodon_status();
        status.content = "Österreich".to_string();
        let tweets = Vec::new();
        let statuses = vec![status];
        let posts = determine_posts(&statuses, &tweets);
        assert!(posts.toots.is_empty());
        assert_eq!(posts.tweets[0], "Österreich");
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

}
