use crate::thread_replies::*;
use anyhow::Result;
use egg_mode::tweet::Tweet;
use egg_mode_text::character_count;
use elefren::entities::status::Status;
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use unicode_segmentation::UnicodeSegmentation;

// Represents new status updates that should be posted to Twitter (tweets) and
// Mastodon (toots).
#[derive(Debug, Clone)]
pub struct StatusUpdates {
    pub tweets: Vec<NewStatus>,
    pub toots: Vec<NewStatus>,
}

impl StatusUpdates {
    /// Reverses the order of statuses in place.
    pub fn reverse_order(&mut self) {
        self.tweets.reverse();
        self.toots.reverse();
    }
}

// A new status for posting. Optionally has links to media (images) that should
// be attached.
#[derive(Debug, Clone)]
pub struct NewStatus {
    pub text: String,
    pub attachments: Vec<NewMedia>,
    // A list of further statuses that are new replies to this new status. Used
    // to sync threads.
    pub replies: Vec<NewStatus>,
    // This new status could be part of a thread, post it in reply to an
    // existing already synced status.
    pub in_reply_to_id: Option<u64>,
    // The original post ID on the source status.
    pub original_id: u64,
}

#[derive(Debug, Clone)]
pub struct NewMedia {
    pub attachment_url: String,
    pub alt_text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SyncOptions {
    pub sync_reblogs: bool,
    pub sync_retweets: bool,
    pub sync_hashtag_twitter: Option<String>,
    pub sync_hashtag_mastodon: Option<String>,
}

/// This is the main synchronization function that can be tested without
/// external API calls.
///
/// The ordering of the statuses in both list parameters is expected to be from
/// newest to oldest. That is also the ordering returned by the Twitter and
/// Mastodon APIs for their timelines, they start with newest posts first.
///
/// The returned data structure contains new posts that are not synchronized yet
/// and should be posted on both Twitter and Mastodon. They are ordered in
/// reverse so that older statuses are posted first if there are multiple
/// statuses to synchronize.
pub fn determine_posts(
    mastodon_statuses: &[Status],
    twitter_statuses: &[Tweet],
    options: &SyncOptions,
) -> StatusUpdates {
    let mut updates = StatusUpdates {
        tweets: Vec::new(),
        toots: Vec::new(),
    };
    'tweets: for tweet in twitter_statuses {
        // Skip replies, they are handled in determine_thread_replies().
        if let Some(_user_id) = &tweet.in_reply_to_user_id {
            continue;
        }

        if tweet.retweeted == Some(true) && !options.sync_retweets {
            // Skip retweets when sync_retweets is disabled
            continue;
        }

        for toot in mastodon_statuses {
            // Skip replies because we don't want to sync them here.
            if let Some(_id) = &toot.in_reply_to_id {
                continue;
            }
            // If the tweet already exists we can stop here and know that we are
            // synced.
            if toot_and_tweet_are_equal(toot, tweet) {
                break 'tweets;
            }
        }

        // The tweet is not on Mastodon yet, check if we should post it.
        // Fetch the tweet text into a String object
        let decoded_tweet = tweet_unshorten_decode(tweet);

        // Check if hashtag filtering is enabled and if the tweet matches.
        if let Some(sync_hashtag) = &options.sync_hashtag_twitter {
            if !sync_hashtag.is_empty() && !decoded_tweet.contains(sync_hashtag) {
                // Skip if a sync hashtag is set and the string doesn't match.
                continue;
            }
        }

        updates.toots.push(NewStatus {
            text: decoded_tweet,
            attachments: tweet_get_attachments(tweet),
            replies: Vec::new(),
            in_reply_to_id: None,
            original_id: tweet.id,
        });
    }

    'toots: for toot in mastodon_statuses {
        // Skip replies, they are handled in determine_thread_replies().
        if let Some(_id) = &toot.in_reply_to_id {
            continue;
        }

        if toot.reblog.is_some() && !options.sync_reblogs {
            // Skip reblogs when sync_reblogs is disabled
            continue;
        }
        let fulltext = mastodon_toot_get_text(toot);
        // If this is a reblog/boost then take the URL to the original toot.
        let post = match &toot.reblog {
            None => tweet_shorten(&fulltext, &toot.url),
            Some(reblog) => tweet_shorten(&fulltext, &reblog.url),
        };
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

        // The toot is not on Twitter yet, check if we should post it.
        // Check if hashtag filtering is enabled and if the tweet matches.
        if let Some(sync_hashtag) = &options.sync_hashtag_mastodon {
            if !sync_hashtag.is_empty() && !fulltext.contains(sync_hashtag) {
                // Skip if a sync hashtag is set and the string doesn't match.
                continue;
            }
        }

        updates.tweets.push(NewStatus {
            text: post,
            attachments: toot_get_attachments(toot),
            replies: Vec::new(),
            in_reply_to_id: None,
            original_id: toot
                .id
                .parse()
                .unwrap_or_else(|_| panic!("Mastodon status ID is not u64: {}", toot.id)),
        });
    }

    determine_thread_replies(mastodon_statuses, twitter_statuses, options, &mut updates);

    // Older posts should come first to preserve the ordering of posts to
    // synchronize.
    updates.reverse_order();
    updates
}

// Returns true if a Mastodon toot and a Twitter tweet are considered equal.
pub fn toot_and_tweet_are_equal(toot: &Status, tweet: &Tweet) -> bool {
    // Make sure the structure is the same: both must be replies or both must
    // not be replies.
    if (toot.in_reply_to_id.is_some() && tweet.in_reply_to_status_id.is_none())
        || (toot.in_reply_to_id.is_none() && tweet.in_reply_to_status_id.is_some())
    {
        return false;
    }

    // Strip markup from Mastodon toot and unify message for comparison.
    let toot_text = unify_post_content(mastodon_toot_get_text(toot));
    // Replace those ugly t.co URLs in the tweet text.
    let tweet_text = unify_post_content(tweet_unshorten_decode(tweet));

    if toot_text == tweet_text {
        return true;
    }
    // Mastodon allows up to 500 characters, so we might need to shorten the
    // toot. If this is a reblog/boost then take the URL to the original toot.
    let shortened_toot = unify_post_content(match &toot.reblog {
        None => tweet_shorten(&toot_text, &toot.url),
        Some(reblog) => tweet_shorten(&toot_text, &reblog.url),
    });

    if shortened_toot == tweet_text {
        return true;
    }

    false
}

// Unifies tweet text or toot text to a common format.
fn unify_post_content(content: String) -> String {
    let mut result = content.to_lowercase();
    // Remove http:// and https:// for comparing because Twitter sometimes adds
    // those randomly.
    result = result.replace("http://", "");
    result = result.replace("https://", "");

    // Support for old posts that started with "RT @\username:", we consider
    // them equal to "RT username:".
    if result.starts_with("rt @\\") {
        result = result.replacen("rt @\\", "rt ", 1);
    }
    // Support for old posts that started with "RT @username:", we consider them
    // equal to "RT username:".
    if result.starts_with("rt @") {
        result = result.replacen("rt @", "rt ", 1);
    }
    if result.starts_with("rt \\@") {
        result = result.replacen("rt \\@", "rt ", 1);
    }
    // Escape direct user mentions with \@.
    result = result.replace(" \\@", " @");
    result.replace(" @\\", " @")
}

// Replace t.co URLs and HTML entity decode &amp;.
// Directly include quote tweets in the text.
pub fn tweet_unshorten_decode(tweet: &Tweet) -> String {
    // We need to cleanup the tweet text while passing the tweet around.
    let mut tweet = tweet.clone();

    if let Some(retweet) = &tweet.retweeted_status {
        tweet.text = format!(
            "RT {}: {}",
            retweet
                .clone()
                .user
                .unwrap_or_else(|| panic!("Twitter user missing on retweet {}", retweet.id))
                .screen_name,
            tweet_get_text_with_quote(retweet)
        );
        tweet.entities.urls = retweet.entities.urls.clone();
        tweet.extended_entities = retweet.extended_entities.clone();
    }

    // Remove the last media link if there is one, we will upload attachments
    // directly to Mastodon.
    if let Some(media) = &tweet.extended_entities {
        for attachment in &media.media {
            tweet.text = tweet.text.replace(&attachment.url, "");
        }
    }
    tweet.text = tweet.text.trim().to_string();
    tweet.text = tweet_get_text_with_quote(&tweet);

    // Replace t.co URLs with the real links in tweets.
    for url in tweet.entities.urls {
        if let Some(expanded_url) = &url.expanded_url {
            tweet.text = tweet.text.replace(&url.url, expanded_url);
        }
    }

    // Escape direct user mentions with @\.
    tweet.text = tweet.text.replace(" @", " @\\").replace(" @\\\\", " @\\");

    // Twitterposts have HTML entities such as &amp;, we need to decode them.
    let decoded = html_escape::decode_html_entities(&tweet.text);

    toot_shorten(&decoded, tweet.id)
}

// If this is a quote tweet then include the original text.
fn tweet_get_text_with_quote(tweet: &Tweet) -> String {
    match tweet.quoted_status {
        None => tweet.text.clone(),
        Some(ref quoted_tweet) => {
            // Prevent infinite quote tweets. We only want to include
            // the first level, so make sure that the original has any
            // quote tweet removed.
            let mut original = quoted_tweet.clone();
            original.quoted_status = None;
            let original_text = tweet_unshorten_decode(&original);
            let screen_name = &original
                .user
                .as_ref()
                .unwrap_or_else(|| panic!("Twitter user missing on tweet {}", original.id))
                .screen_name;
            let mut tweet_text = tweet.text.clone();

            // Remove quote link at the end of the tweet text.
            for url in &tweet.entities.urls {
                if let Some(expanded_url) = &url.expanded_url {
                    if expanded_url
                        == &format!(
                            "https://twitter.com/{}/status/{}",
                            screen_name, quoted_tweet.id
                        )
                        || expanded_url
                            == &format!(
                                "https://mobile.twitter.com/{}/status/{}",
                                screen_name, quoted_tweet.id
                            )
                    {
                        tweet_text = tweet_text.replace(&url.url, "").trim().to_string();
                    }
                }
            }

            format!(
                "{tweet_text}

QT {screen_name}: {original_text}"
            )
        }
    }
}

pub fn tweet_shorten(text: &str, toot_url: &Option<String>) -> String {
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
    with_link
}

// Mastodon has a 500 character post limit. With embedded quote tweets and long
// links the content could get too long, shorten it to 500 characters.
fn toot_shorten(text: &str, tweet_id: u64) -> String {
    let mut char_count = text.graphemes(true).count();
    let re = Regex::new(r"[^\s]+$").unwrap();
    let mut shortened = text.trim().to_string();
    let mut with_link = shortened.clone();

    // Hard-coding a limit of 500 here for now, could be configurable.
    while char_count > 500 {
        // Remove the last word.
        shortened = re.replace_all(&shortened, "").trim().to_string();
        // Add a link to the full length tweet.
        with_link = format!("{shortened}… https://twitter.com/twitter/status/{tweet_id}");
        char_count = with_link.graphemes(true).count();
    }
    with_link
}

// Prefix boost toots with the author and strip HTML tags.
pub fn mastodon_toot_get_text(toot: &Status) -> String {
    let mut replaced = match toot.reblog {
        None => toot.content.clone(),
        Some(ref reblog) => format!("RT {}: {}", reblog.account.acct, reblog.content),
    };
    replaced = replaced.replace("<br />", "\n");
    replaced = replaced.replace("<br>", "\n");
    replaced = replaced.replace("</p><p>", "\n\n");
    replaced = replaced.replace("<p>", "");
    replaced = replaced.replace("</p>", "");

    replaced = voca_rs::strip::strip_tags(&replaced);

    // Escape direct user mentions with @\.
    replaced = replaced.replace(" @", " @\\").replace(" @\\\\", " @\\");

    html_escape::decode_html_entities(&replaced).to_string()
}

// Ensure that sync posts have not been made before to prevent syncing loops.
// Use a cache file to temporarily store posts and compare them on the next
// invocation.
pub fn filter_posted_before(
    posts: StatusUpdates,
    post_cache: &HashSet<String>,
) -> Result<StatusUpdates> {
    // If there are no status updates then we don't need to check anything.
    if posts.toots.is_empty() && posts.tweets.is_empty() {
        return Ok(posts);
    }

    let mut filtered_posts = StatusUpdates {
        tweets: Vec::new(),
        toots: Vec::new(),
    };
    for tweet in posts.tweets {
        if post_cache.contains(&tweet.text) {
            eprintln!(
                "Error: preventing double posting to Twitter: {}",
                tweet.text
            );
        } else {
            filtered_posts.tweets.push(tweet.clone());
        }
    }
    for toot in posts.toots {
        if post_cache.contains(&toot.text) {
            eprintln!(
                "Error: preventing double posting to Mastodon: {}",
                toot.text
            );
        } else {
            filtered_posts.toots.push(toot.clone());
        }
    }

    Ok(filtered_posts)
}

// Read the JSON encoded cache file from disk or provide an empty default cache.
pub fn read_post_cache(cache_file: &str) -> HashSet<String> {
    match fs::read_to_string(cache_file) {
        Ok(json) => {
            match serde_json::from_str::<HashSet<String>>(&json) {
                Ok(cache) => {
                    // If the cache has more than 150 items already then empty it to not
                    // accumulate too many items and allow posting the same text at a
                    // later date.
                    if cache.len() > 150 {
                        HashSet::new()
                    } else {
                        cache
                    }
                }
                Err(_) => HashSet::new(),
            }
        }
        Err(_) => HashSet::new(),
    }
}

// Returns a list of direct links to attachments for download.
pub fn tweet_get_attachments(tweet: &Tweet) -> Vec<NewMedia> {
    let mut links = Vec::new();
    // Check if there are attachments directly on the tweet, otherwise try to
    // use attachments from retweets and quote tweets.
    let media = match &tweet.extended_entities {
        Some(media) => Some(media),
        None => {
            let mut retweet_media = None;
            if let Some(retweet) = &tweet.retweeted_status {
                if let Some(media) = &retweet.extended_entities {
                    retweet_media = Some(media);
                }
            } else if let Some(quote_tweet) = &tweet.quoted_status {
                if let Some(media) = &quote_tweet.extended_entities {
                    retweet_media = Some(media);
                }
            }
            retweet_media
        }
    };

    if let Some(media) = media {
        for attachment in &media.media {
            match &attachment.video_info {
                Some(video_info) => {
                    let mut bitrate = 0;
                    let mut media_url = "".to_string();
                    // Use the video variant with the highest bitrate.
                    for variant in &video_info.variants {
                        if let Some(video_bitrate) = variant.bitrate {
                            if video_bitrate > bitrate {
                                bitrate = video_bitrate;
                                media_url = variant.url.clone();
                            }
                        }
                    }
                    links.push(NewMedia {
                        attachment_url: media_url,
                        alt_text: attachment.ext_alt_text.clone(),
                    });
                }
                None => {
                    links.push(NewMedia {
                        attachment_url: attachment.media_url_https.clone(),
                        alt_text: attachment.ext_alt_text.clone(),
                    });
                }
            }
        }
    }
    links
}

// Returns a list of direct links to attachments for download.
pub fn toot_get_attachments(toot: &Status) -> Vec<NewMedia> {
    let mut links = Vec::new();
    let mut attachments = &toot.media_attachments;
    // If there are no attachments check if this is a boost and if there might
    // be some attachments there.
    if attachments.is_empty() {
        if let Some(boost) = &toot.reblog {
            attachments = &boost.media_attachments;
        }
    }
    for attachment in attachments {
        links.push(NewMedia {
            attachment_url: attachment.url.clone(),
            // Twitter only allows a max length of 1,000 characters for alt
            // text, so we need to cut it off here.
            alt_text: truncate_option_string(attachment.description.clone(), 1_000),
        });
    }
    links
}

/// Truncates a given string to a maximum number of characters.
///
/// I could not find a Rust core function that does this? We don't care about
/// graphemes, please just cut off characters at a certain length. Copied from
/// https://stackoverflow.com/a/38461750/2000435
///
/// No, I will not install the substring crate just to get a substring, are you
/// kidding me????
fn truncate_option_string(stringy: Option<String>, max_chars: usize) -> Option<String> {
    match stringy {
        Some(string) => match string.char_indices().nth(max_chars) {
            None => Some(string),
            Some((idx, _)) => Some(string[..idx].to_string()),
        },
        None => None,
    }
}

#[cfg(test)]
pub mod tests {

    use super::*;
    use chrono::Utc;
    use egg_mode::entities::ResizeMode::{Crop, Fit};
    use egg_mode::entities::VideoInfo;
    use egg_mode::entities::VideoVariant;
    use egg_mode::entities::{
        HashtagEntity, MediaEntity, MediaSize, MediaSizes, MediaType, UrlEntity,
    };
    use egg_mode::tweet::{ExtendedTweetEntities, TweetEntities, TweetSource};
    use egg_mode::user::{TwitterUser, UserEntities, UserEntityDetail};

    static DEFAULT_SYNC_OPTIONS: SyncOptions = SyncOptions {
        sync_reblogs: true,
        sync_retweets: true,
        sync_hashtag_twitter: None,
        sync_hashtag_mastodon: None,
    };

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
        let posts = determine_posts(&statuses, &tweets, &DEFAULT_SYNC_OPTIONS);
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
        let posts = determine_posts(&statuses, &tweets, &DEFAULT_SYNC_OPTIONS);
        assert!(posts.toots.is_empty());
        assert!(posts.tweets.is_empty());
    }

    // Test that Mastodon status text is posted HTML entity decoded to Twitter.
    // &amp; => &
    #[test]
    fn mastodon_html_decode() {
        let mut status = get_mastodon_status();
        status.content = "<p>You &amp; me!</p>".to_string();
        let posts = determine_posts(&vec![status], &Vec::new(), &DEFAULT_SYNC_OPTIONS);
        assert_eq!(posts.tweets[0].text, "You & me!");
    }

    // Test that Twitter status text is posted HTML entity decoded to Mastodon.
    // &amp; => &
    #[test]
    fn twitter_html_decode() {
        let mut status = get_twitter_status();
        status.text = "You &amp; me!".to_string();
        let posts = determine_posts(&Vec::new(), &vec![status], &DEFAULT_SYNC_OPTIONS);
        assert_eq!(posts.toots[0].text, "You & me!");
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

        let posts = determine_posts(&vec![status], &Vec::new(), &DEFAULT_SYNC_OPTIONS);
        assert_eq!(posts.tweets[0].text, "RT example: Some example toooot!");
    }

    // Test that the URL from the original toot is used in a long boost.
    #[test]
    fn mastodon_boost_url() {
        let mut reblog = get_mastodon_status();
        reblog.content = "longer than 280 characters longer than 280 characters longer than 280 characters longer than 280 characters longer than 280 characters longer than 280 characters longer than 280 characters longer than 280 characters longer than 280 characters".to_string();
        reblog.url = Some("https://example.com/a/b/c/5".to_string());
        let mut status = get_mastodon_status();
        status.reblog = Some(Box::new(reblog));
        status.reblogged = Some(true);

        let posts = determine_posts(&vec![status], &Vec::new(), &DEFAULT_SYNC_OPTIONS);
        assert_eq!(posts.tweets[0].text, "RT example: longer than 280 characters longer than 280 characters longer than 280 characters longer than 280 characters longer than 280 characters longer than 280 characters longer than 280 characters longer than… https://example.com/a/b/c/5");
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
        let posts = determine_posts(&statuses, &tweets, &DEFAULT_SYNC_OPTIONS);
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

    // Test that @username mentions are escaped, because we don't want to mention completely unrelated users on the other network.
    #[test]
    fn mention_escaped() {
        let mut status = get_mastodon_status();
        status.content = "I will mention <span class=\"h-card\"><a href=\"https://example.com/@klausi\" class=\"u-url mention\">@<span>klausi</span></a></span> here".to_string();
        let mut tweet = get_twitter_status();
        tweet.text = "I will mention @\\klausi here".to_string();
        assert!(toot_and_tweet_are_equal(&status, &tweet));

        let tweets = Vec::new();
        let statuses = vec![status];
        let posts = determine_posts(&statuses, &tweets, &DEFAULT_SYNC_OPTIONS);
        assert!(posts.toots.is_empty());
        assert_eq!(posts.tweets[0].text, "I will mention @\\klausi here");

        tweet.text = "I will mention @klausi here".to_string();
        let tweets = vec![tweet];
        let statuses = Vec::new();
        let posts = determine_posts(&statuses, &tweets, &DEFAULT_SYNC_OPTIONS);
        assert!(posts.tweets.is_empty());
        assert_eq!(posts.toots[0].text, "I will mention @\\klausi here");
    }

    // Test that the old way of escaping with \@username is considered the same
    // as @\username
    #[test]
    fn mention_old_escaped() {
        let mut status = get_mastodon_status();
        status.content = "I will mention <span class=\"h-card\"><a href=\"https://example.com/@klausi\" class=\"u-url mention\">@<span>klausi</span></a></span> here".to_string();
        let mut tweet = get_twitter_status();
        tweet.text = "I will mention \\@klausi here".to_string();
        assert!(toot_and_tweet_are_equal(&status, &tweet));

        let tweets = vec![tweet.clone()];
        let statuses = vec![status.clone()];
        let posts = determine_posts(&statuses, &tweets, &DEFAULT_SYNC_OPTIONS);
        assert!(posts.toots.is_empty());
        assert!(posts.tweets.is_empty());

        tweet.text = "I will mention @klausi here".to_string();
        status.content = "I will mention \\@klausi here".to_string();
        assert!(toot_and_tweet_are_equal(&status, &tweet));
        let tweets = vec![tweet];
        let statuses = vec![status];
        let posts = determine_posts(&statuses, &tweets, &DEFAULT_SYNC_OPTIONS);
        assert!(posts.toots.is_empty());
        assert!(posts.tweets.is_empty());
    }

    // Test that direct toots starting with "@" are not copied to twitter.
    #[test]
    fn direct_toot() {
        let mut status = get_mastodon_status();
        status.content = "@Test Hello! http://example.com".to_string();
        let tweets = Vec::new();
        let statuses = vec![status];
        let posts = determine_posts(&statuses, &tweets, &DEFAULT_SYNC_OPTIONS);
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
        let posts = determine_posts(&statuses, &tweets, &DEFAULT_SYNC_OPTIONS);
        assert!(posts.toots.is_empty());
        assert_eq!(posts.tweets[0].text, "Österreich");
    }

    // Test that posting something looking like a URL/domain is considered
    // equal coming back from Twitter.
    #[test]
    fn urls_in_posts() {
        let mut status = get_mastodon_status();
        status.content = "<p>What happened to the bofa.lol instance? <a href=\"https://mastodon.social/tags/mastodon\" class=\"mention hashtag\" rel=\"tag\">#<span>mastodon</span></a></p>".to_string();
        let mut tweet = get_twitter_status();
        tweet.text = "What happened to the https://t.co/OxEvHBajwd instance? #mastodon".to_string();
        tweet.entities = TweetEntities {
            hashtags: vec![HashtagEntity {
                range: (55, 64),
                text: "mastodon".to_string(),
            }],
            symbols: Vec::new(),
            urls: vec![UrlEntity {
                display_url: "bofa.lol".to_string(),
                expanded_url: Some("http://bofa.lol".to_string()),
                range: (21, 44),
                url: "https://t.co/OxEvHBajwd".to_string(),
            }],
            user_mentions: Vec::new(),
            media: None,
        };

        assert!(toot_and_tweet_are_equal(&status, &tweet));
    }

    // Test that if there are pictures in a tweet that they are attached as
    // media files to the Mastodon toot.
    #[test]
    fn pictures_in_tweet() {
        let tweets = vec![get_twitter_status_media()];
        let statuses = Vec::new();
        let posts = determine_posts(&statuses, &tweets, &DEFAULT_SYNC_OPTIONS);

        let status = &posts.toots[0];
        assert_eq!(status.text, "Verhalten bei #Hausdurchsuchung");
        assert_eq!(
            status.attachments[0].attachment_url,
            "https://pbs.twimg.com/media/Du70iGVUcAMgBp6.jpg"
        );
        assert_eq!(
            status.attachments[0].alt_text,
            Some("Accessibility text".to_string())
        );
    }

    // Test that attached videos are posted directly to Mastodon.
    #[test]
    fn video_in_tweet() {
        let tweet = get_twitter_status_video();
        let tweets = vec![tweet];
        let statuses = Vec::new();
        let posts = determine_posts(&statuses, &tweets, &DEFAULT_SYNC_OPTIONS);

        let status = &posts.toots[0];
        assert_eq!(status.text, "Verhalten bei #Hausdurchsuchung");
        assert_eq!(
            status.attachments[0].attachment_url,
            "https://video.twimg.com/ext_tw_video/869317980307415040/pu/vid/720x1280/octt5pFbISkef8RB.mp4"
        );
        assert_eq!(
            status.attachments[0].alt_text,
            Some("Accessibility text".to_string())
        );
    }

    // Test that if there are pictures in a toot that they are attached as
    // media files to the tweet.
    #[test]
    fn pictures_in_toot() {
        let statuses = vec![get_mastodon_status_media()];
        let tweets = Vec::new();
        let posts = determine_posts(&statuses, &tweets, &DEFAULT_SYNC_OPTIONS);

        let tweet = &posts.tweets[0];
        assert_eq!(tweet.text, "test image");
        assert_eq!(
            tweet.attachments[0].attachment_url,
            "https://files.mastodon.social/media_attachments/files/011/514/042/original/e046a3fb6a71a07b.jpg"
        );
        assert_eq!(
            tweet.attachments[0].alt_text,
            Some("Test image from a TV screen".to_string())
        );
    }

    // Test retweets that have attachments.
    #[test]
    fn picture_in_retweet() {
        let mut retweet = get_twitter_status();
        retweet.retweeted = Some(true);
        let mut original_tweet = get_twitter_status_media();
        original_tweet.user = Some(Box::new(get_twitter_user()));
        retweet.retweeted_status = Some(Box::new(original_tweet));

        let tweets = vec![retweet];
        let toots = Vec::new();
        let posts = determine_posts(&toots, &tweets, &DEFAULT_SYNC_OPTIONS);

        let sync_toot = &posts.toots[0];
        assert_eq!(
            sync_toot.text,
            "RT test123: Verhalten bei #Hausdurchsuchung"
        );
        assert_eq!(
            sync_toot.attachments[0].attachment_url,
            "https://pbs.twimg.com/media/Du70iGVUcAMgBp6.jpg"
        );
    }

    // Test boosts that have attachments.
    #[test]
    fn picture_in_boost() {
        let original_toot = get_mastodon_status_media();
        let mut boost = get_mastodon_status();
        boost.reblog = Some(Box::new(original_toot));

        let tweets = Vec::new();
        let toots = vec![boost];
        let posts = determine_posts(&toots, &tweets, &DEFAULT_SYNC_OPTIONS);

        let sync_tweet = &posts.tweets[0];
        assert_eq!(sync_tweet.text, "RT example: test image");
        assert_eq!(sync_tweet.attachments[0].attachment_url, "https://files.mastodon.social/media_attachments/files/011/514/042/original/e046a3fb6a71a07b.jpg");
    }

    // Test that a quote tweet is directly embedded for posting to Mastodon.
    #[test]
    fn quote_tweet() {
        let mut quote_tweet = get_twitter_status();
        quote_tweet.text = "Quote tweet test https://t.co/MqIukRm3dG".to_string();
        quote_tweet.entities = TweetEntities {
            hashtags: Vec::new(),
            symbols: Vec::new(),
            urls: vec![UrlEntity {
                display_url: "twitter.com/test123/statu…".to_string(),
                expanded_url: Some(
                    "https://twitter.com/test123/status/1230906460160380928".to_string(),
                ),
                range: (21, 44),
                url: "https://t.co/MqIukRm3dG".to_string(),
            }],
            user_mentions: Vec::new(),
            media: None,
        };

        let mut original_tweet = get_twitter_status();
        original_tweet.text = "Original text".to_string();
        original_tweet.user = Some(Box::new(get_twitter_user()));
        original_tweet.id = 1230906460160380928;
        quote_tweet.quoted_status = Some(Box::new(original_tweet));

        let tweets = vec![quote_tweet];
        let toots = Vec::new();
        let posts = determine_posts(&toots, &tweets, &DEFAULT_SYNC_OPTIONS);

        let sync_toot = &posts.toots[0];
        assert_eq!(
            sync_toot.text,
            "Quote tweet test

QT test123: Original text"
        );
    }

    // Test that attachments on a quote tweet get synchronized.
    #[test]
    fn quote_tweet_attachments() {
        let mut quote_tweet = get_twitter_status_media();
        quote_tweet.text =
            "Quote tweet test https://t.co/MqIukRm3dG https://t.co/AhiyYybK1m".to_string();
        quote_tweet.entities = TweetEntities {
            hashtags: Vec::new(),
            symbols: Vec::new(),
            urls: vec![UrlEntity {
                display_url: "twitter.com/test123/statu…".to_string(),
                expanded_url: Some(
                    "https://twitter.com/test123/status/1230906460160380928".to_string(),
                ),
                range: (21, 44),
                url: "https://t.co/MqIukRm3dG".to_string(),
            }],
            user_mentions: Vec::new(),
            media: None,
        };

        let mut original_tweet = get_twitter_status();
        original_tweet.text = "Original text".to_string();
        original_tweet.user = Some(Box::new(get_twitter_user()));
        original_tweet.id = 1230906460160380928;
        quote_tweet.quoted_status = Some(Box::new(original_tweet));

        let tweets = vec![quote_tweet];
        let toots = Vec::new();
        let posts = determine_posts(&toots, &tweets, &DEFAULT_SYNC_OPTIONS);

        let sync_toot = &posts.toots[0];
        assert_eq!(
            sync_toot.text,
            "Quote tweet test

QT test123: Original text"
        );
        assert_eq!(
            sync_toot.attachments[0].attachment_url,
            "https://pbs.twimg.com/media/Du70iGVUcAMgBp6.jpg"
        );
    }

    // Test that attachments on the quote tweet original get synchronized.
    #[test]
    fn quote_tweet_attachments_original() {
        let mut quote_tweet = get_twitter_status();
        quote_tweet.text = "Quote tweet test https://t.co/MqIukRm3dG".to_string();
        quote_tweet.entities = TweetEntities {
            hashtags: Vec::new(),
            symbols: Vec::new(),
            urls: vec![UrlEntity {
                display_url: "twitter.com/test123/statu…".to_string(),
                expanded_url: Some(
                    "https://twitter.com/test123/status/1230906460160380928".to_string(),
                ),
                range: (21, 44),
                url: "https://t.co/MqIukRm3dG".to_string(),
            }],
            user_mentions: Vec::new(),
            media: None,
        };

        let mut original_tweet = get_twitter_status_media();
        original_tweet.user = Some(Box::new(get_twitter_user()));
        original_tweet.id = 1230906460160380928;
        quote_tweet.quoted_status = Some(Box::new(original_tweet));

        let tweets = vec![quote_tweet];
        let toots = Vec::new();
        let posts = determine_posts(&toots, &tweets, &DEFAULT_SYNC_OPTIONS);

        let sync_toot = &posts.toots[0];
        assert_eq!(
            sync_toot.text,
            "Quote tweet test

QT test123: Verhalten bei #Hausdurchsuchung"
        );
        assert_eq!(
            sync_toot.attachments[0].attachment_url,
            "https://pbs.twimg.com/media/Du70iGVUcAMgBp6.jpg"
        );
    }

    // Test that a quote tweet with a mobile.twitter.com link is synced
    // correctly.
    #[test]
    fn mobile_quote_tweet() {
        let mut quote_tweet = get_twitter_status();
        quote_tweet.text = "Quote tweet test https://t.co/MqIukRm3dG".to_string();
        quote_tweet.entities = TweetEntities {
            hashtags: Vec::new(),
            symbols: Vec::new(),
            urls: vec![UrlEntity {
                display_url: "mobile.twitter.com/test123/statu…".to_string(),
                expanded_url: Some(
                    "https://mobile.twitter.com/test123/status/1230906460160380928".to_string(),
                ),
                range: (21, 44),
                url: "https://t.co/MqIukRm3dG".to_string(),
            }],
            user_mentions: Vec::new(),
            media: None,
        };

        let mut original_tweet = get_twitter_status();
        original_tweet.text = "Original text".to_string();
        original_tweet.user = Some(Box::new(get_twitter_user()));
        original_tweet.id = 1230906460160380928;
        quote_tweet.quoted_status = Some(Box::new(original_tweet));

        let tweets = vec![quote_tweet];
        let toots = Vec::new();
        let posts = determine_posts(&toots, &tweets, &DEFAULT_SYNC_OPTIONS);

        let sync_toot = &posts.toots[0];
        assert_eq!(
            sync_toot.text,
            "Quote tweet test

QT test123: Original text"
        );
    }

    // Test that a long tweet and a long quote tweet are shortened to pass the
    // 500 character limit of Mastodon.
    #[test]
    fn long_quote_tweet() {
        let mut quote_tweet = get_twitter_status();
        quote_tweet.id = 1515580612391936001;
        quote_tweet.text = "SQLite is an absolute fascinating open source project by 3 old white men. They reject contributions, have a troll code of conduct and even built their own version control system insted of using git! https://t.co/5mE9PjjAsR".to_string();
        quote_tweet.entities = TweetEntities {
            hashtags: Vec::new(),
            symbols: Vec::new(),
            urls: vec![UrlEntity {
                display_url: "twitter.com/test123/statu…".to_string(),
                expanded_url: Some(
                    "https://twitter.com/test123/status/1515372417081745410".to_string(),
                ),
                range: (199, 222),
                url: "https://t.co/5mE9PjjAsR".to_string(),
            }],
            user_mentions: Vec::new(),
            media: None,
        };

        let mut original_tweet = get_twitter_status();
        original_tweet.text = "Reminder that there's a *very* small group of maintainers on SQLite and they have some odd practices when it comes to building software. They went as far as building their own VCS so no one else could contribute and have this as their \"Code Of Ethics\" https://t.co/2KL9b2BENN https://t.co/NdfoMUScX2".to_string();
        original_tweet.entities = TweetEntities {
            hashtags: Vec::new(),
            symbols: Vec::new(),
            urls: vec![
                UrlEntity {
                    display_url: "sqlite.org/codeofethics.h…".to_string(),
                    expanded_url: Some("https://sqlite.org/codeofethics.html".to_string()),
                    range: (252, 275),
                    url: "https://t.co/2KL9b2BENN".to_string(),
                },
                UrlEntity {
                    display_url: "twitter.com/SebastianSztur…".to_string(),
                    expanded_url: Some(
                        "https://twitter.com/SebastianSzturo/status/1515297367335247877"
                            .to_string(),
                    ),
                    range: (276, 299),
                    url: "https://t.co/NdfoMUScX2".to_string(),
                },
            ],
            user_mentions: Vec::new(),
            media: None,
        };
        original_tweet.user = Some(Box::new(get_twitter_user()));
        original_tweet.id = 1515372417081745410;
        quote_tweet.quoted_status = Some(Box::new(original_tweet));

        let tweets = vec![quote_tweet];
        let toots = Vec::new();
        let posts = determine_posts(&toots, &tweets, &DEFAULT_SYNC_OPTIONS);

        let sync_toot = &posts.toots[0];
        assert_eq!(
            sync_toot.text,
            "SQLite is an absolute fascinating open source project by 3 old white men. They reject contributions, have a troll code of conduct and even built their own version control system insted of using git!

QT test123: Reminder that there's a *very* small group of maintainers on SQLite and they have some odd practices when it comes to building software. They went as far as building their own VCS so no one else could contribute and have this as… https://twitter.com/twitter/status/1515580612391936001"
        );

        // Also test that a shortened toot is detected as equal.
        let mut status = get_mastodon_status();
        status.content = sync_toot.text.clone();
        let posts = determine_posts(&vec![status], &tweets, &DEFAULT_SYNC_OPTIONS);
        assert!(posts.toots.is_empty());
        assert!(posts.tweets.is_empty());
    }

    // Test that retweets are ignored when `sync_retweets` is `false`
    #[test]
    fn ignore_retweets() {
        let mut original_tweet = get_twitter_status();
        original_tweet.user = Some(Box::new(get_twitter_user()));
        original_tweet.id = 1230906460160380928;

        let mut retweet = get_twitter_status();
        retweet.user = Some(Box::new(get_twitter_user()));
        retweet.retweeted = Some(true);
        retweet.retweeted_status = Some(Box::new(original_tweet));

        let tweets = vec![retweet];
        let toots = Vec::new();
        let mut options = DEFAULT_SYNC_OPTIONS.clone();
        options.sync_retweets = false;

        let posts = determine_posts(&toots, &tweets, &options);
        assert!(posts.toots.is_empty());
        assert!(posts.tweets.is_empty());
    }

    // Test that quote tweets are synced when `sync_retweets=false`
    #[test]
    fn quote_tweets_are_synced_when_ignoring_retweets() {
        let mut original_tweet = get_twitter_status();
        original_tweet.text = "Original text".to_string();
        original_tweet.user = Some(Box::new(get_twitter_user()));
        original_tweet.id = 1230906460160380928;

        let mut quote_tweet = get_twitter_status();
        quote_tweet.text = "Quote tweet test".to_string();
        quote_tweet.quoted_status = Some(Box::new(original_tweet));

        let tweets = vec![quote_tweet];
        let toots = Vec::new();
        let mut options = DEFAULT_SYNC_OPTIONS.clone();
        options.sync_retweets = false;

        let posts = determine_posts(&toots, &tweets, &options);

        let sync_toot = &posts.toots[0];

        assert_eq!(
            sync_toot.text,
            "Quote tweet test

QT test123: Original text"
        );
    }

    // Test that reblogs are ignored when `sync_reblogs` is `false`
    #[test]
    fn ignore_reblogs() {
        let original_toot = get_mastodon_status();
        let mut boost = get_mastodon_status();
        boost.reblog = Some(Box::new(original_toot));

        let tweets = Vec::new();
        let toots = vec![boost];
        let mut options = DEFAULT_SYNC_OPTIONS.clone();
        options.sync_reblogs = false;

        let posts = determine_posts(&toots, &tweets, &options);
        assert!(posts.toots.is_empty());
        assert!(posts.tweets.is_empty());
    }

    // Test tagged posts are sent when hashtag is set
    #[test]
    fn tagged_posts_sent() {
        let mut status = get_mastodon_status();
        status.content = "Let's #tweet!".to_string();
        let mut tweet = get_twitter_status();
        tweet.text = "Let's #toot!".to_string();

        let mut options = DEFAULT_SYNC_OPTIONS.clone();
        options.sync_hashtag_twitter = Some("#toot".to_string());
        options.sync_hashtag_mastodon = Some("#tweet".to_string());

        let tweets = vec![tweet];
        let toots = vec![status];

        let posts = determine_posts(&toots, &tweets, &options);
        assert!(!posts.toots.is_empty());
        assert!(!posts.tweets.is_empty());
    }

    // Test posts without a tag are not sent
    #[test]
    fn ignore_untagged_posts() {
        let mut status = get_mastodon_status();
        status.content = "Let's NOT tweet!".to_string();
        let mut tweet = get_twitter_status();
        tweet.text = "Let's NOT toot!".to_string();

        let mut options = DEFAULT_SYNC_OPTIONS.clone();
        options.sync_hashtag_twitter = Some("#toot".to_string());
        options.sync_hashtag_mastodon = Some("#tweet".to_string());

        let tweets = vec![tweet];
        let toots = vec![status];

        let posts = determine_posts(&toots, &tweets, &options);
        assert!(posts.toots.is_empty());
        assert!(posts.tweets.is_empty());
    }

    // Test that a retweet of a quote tweet also includes the quoted text.
    #[test]
    fn retweet_quote_tweet() {
        let mut quote_tweet = get_twitter_status();
        quote_tweet.text = "Quote tweet test https://t.co/MqIukRm3dG".to_string();
        quote_tweet.entities = TweetEntities {
            hashtags: Vec::new(),
            symbols: Vec::new(),
            urls: vec![UrlEntity {
                display_url: "twitter.com/test123/statu…".to_string(),
                expanded_url: Some(
                    "https://twitter.com/test123/status/1230906460160380928".to_string(),
                ),
                range: (21, 44),
                url: "https://t.co/MqIukRm3dG".to_string(),
            }],
            user_mentions: Vec::new(),
            media: None,
        };
        quote_tweet.user = Some(Box::new(get_twitter_user()));

        let mut original_tweet = get_twitter_status();
        original_tweet.text = "Original text".to_string();
        original_tweet.user = Some(Box::new(get_twitter_user()));
        original_tweet.id = 1230906460160380928;
        quote_tweet.quoted_status = Some(Box::new(original_tweet));

        let mut retweet = get_twitter_status();
        retweet.user = Some(Box::new(get_twitter_user()));
        retweet.retweeted = Some(true);
        retweet.retweeted_status = Some(Box::new(quote_tweet));

        let tweets = vec![retweet];
        let toots = Vec::new();
        let posts = determine_posts(&toots, &tweets, &DEFAULT_SYNC_OPTIONS);

        let sync_toot = &posts.toots[0];
        assert_eq!(
            sync_toot.text,
            "RT test123: Quote tweet test

QT test123: Original text"
        );
    }

    // Test that a Mastodon thread reply is not synced if there is no parent.
    #[test]
    fn mastodon_thread_reply() {
        let mut status = get_mastodon_status();
        status.in_reply_to_id = Some("1234".to_string());
        let toots = vec![status];

        let posts = determine_posts(&toots, &Vec::new(), &DEFAULT_SYNC_OPTIONS);
        assert!(posts.toots.is_empty());
        assert!(posts.tweets.is_empty());
    }

    // Test that the post order is correct.
    #[test]
    fn post_order() {
        let mut toot1 = get_mastodon_status();
        toot1.content = "toot #1".to_string();
        let mut toot2 = get_mastodon_status();
        toot2.content = "toot #2".to_string();

        let mut tweet1 = get_twitter_status();
        tweet1.text = "tweet #1".to_string();
        let mut tweet2 = get_twitter_status();
        tweet2.text = "tweet #2".to_string();

        let posts = determine_posts(
            &vec![toot1, toot2],
            &vec![tweet1, tweet2],
            &DEFAULT_SYNC_OPTIONS,
        );
        assert_eq!(
            vec!["tweet #2", "tweet #1"],
            posts
                .toots
                .iter()
                .map(|v| v.text.as_str())
                .collect::<Vec<&str>>()
        );
        assert_eq!(
            vec!["toot #2", "toot #1"],
            posts
                .tweets
                .iter()
                .map(|v| v.text.as_str())
                .collect::<Vec<&str>>()
        );
    }

    // Test that long image alt text on Mastodon is shortened to the Twitter
    // 1000 character limit.
    #[test]
    fn tweet_alt_text_length() {
        let mut toot = get_mastodon_status_media();
        toot.media_attachments[0].description = Some("a".repeat(1_001));
        let posts = determine_posts(&vec![toot], &Vec::new(), &DEFAULT_SYNC_OPTIONS);

        let tweet = &posts.tweets[0];
        assert_eq!(tweet.attachments[0].alt_text, Some("a".repeat(1_000)));
    }

    pub fn get_mastodon_status() -> Status {
        read_mastodon_status("src/mastodon_status.json")
    }

    fn get_mastodon_status_media() -> Status {
        read_mastodon_status("src/mastodon_attach.json")
    }

    fn read_mastodon_status(file_name: &str) -> Status {
        let json = fs::read_to_string(file_name).unwrap();
        let status: Status = serde_json::from_str(&json).unwrap();
        status
    }

    pub fn get_twitter_status() -> Tweet {
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
            filter_level: None,
            id: 123456,
            in_reply_to_user_id: None,
            in_reply_to_screen_name: None,
            in_reply_to_status_id: None,
            lang: None,
            place: None,
            possibly_sensitive: None,
            quoted_status_id: None,
            quoted_status: None,
            retweet_count: 0,
            retweeted: None,
            retweeted_status: None,
            source: None,
            text: "".to_string(),
            truncated: false,
            user: None,
            withheld_copyright: false,
            withheld_in_countries: None,
            withheld_scope: None,
        }
    }

    fn get_twitter_status_media() -> Tweet {
        Tweet {
            coordinates: None,
            created_at: Utc::now(),
            current_user_retweet: None,
            display_text_range: Some((0, 31)),
            entities: TweetEntities {
                hashtags: vec![HashtagEntity {
                    range: (14, 31),
                    text: "Hausdurchsuchung".to_string(),
                }],
                symbols: Vec::new(),
                urls: Vec::new(),
                user_mentions: Vec::new(),
                media: Some(vec![MediaEntity {
                    display_url: "pic.twitter.com/AhiyYybK1m".to_string(),
                    expanded_url: "https://twitter.com/_example_/status/1234567890/photo/1"
                        .to_string(),
                    id: 1076066227640889347,
                    range: (32, 55),
                    media_url: "http://pbs.twimg.com/media/Du70iGVUcAMgBp6.jpg".to_string(),
                    media_url_https: "https://pbs.twimg.com/media/Du70iGVUcAMgBp6.jpg".to_string(),
                    sizes: MediaSizes {
                        thumb: MediaSize {
                            w: 150,
                            h: 150,
                            resize: Crop,
                        },
                        small: MediaSize {
                            w: 612,
                            h: 680,
                            resize: Fit,
                        },
                        medium: MediaSize {
                            w: 716,
                            h: 795,
                            resize: Fit,
                        },
                        large: MediaSize {
                            w: 716,
                            h: 795,
                            resize: Fit,
                        },
                    },
                    source_status_id: None,
                    media_type: MediaType::Photo,
                    url: "https://t.co/AhiyYybK1m".to_string(),
                    video_info: None,
                    ext_alt_text: Some("Accessibility text".to_string()),
                }]),
            },
            extended_entities: Some(ExtendedTweetEntities {
                media: vec![MediaEntity {
                    display_url: "pic.twitter.com/AhiyYybK1m".to_string(),
                    expanded_url: "https://twitter.com/_example_/status/1234567890/photo/1"
                        .to_string(),
                    id: 1076066227640889347,
                    range: (32, 55),
                    media_url: "http://pbs.twimg.com/media/Du70iGVUcAMgBp6.jpg".to_string(),
                    media_url_https: "https://pbs.twimg.com/media/Du70iGVUcAMgBp6.jpg".to_string(),
                    sizes: MediaSizes {
                        thumb: MediaSize {
                            w: 150,
                            h: 150,
                            resize: Crop,
                        },
                        small: MediaSize {
                            w: 612,
                            h: 680,
                            resize: Fit,
                        },
                        medium: MediaSize {
                            w: 716,
                            h: 795,
                            resize: Fit,
                        },
                        large: MediaSize {
                            w: 716,
                            h: 795,
                            resize: Fit,
                        },
                    },
                    source_status_id: None,
                    media_type: MediaType::Photo,
                    url: "https://t.co/AhiyYybK1m".to_string(),
                    video_info: None,
                    ext_alt_text: Some("Accessibility text".to_string()),
                }],
            }),
            favorite_count: 0,
            favorited: Some(false),
            filter_level: None,
            id: 1234567890,
            in_reply_to_user_id: None,
            in_reply_to_screen_name: None,
            in_reply_to_status_id: None,
            lang: Some("de".to_string()),
            place: None,
            possibly_sensitive: Some(false),
            quoted_status_id: None,
            quoted_status: None,
            retweet_count: 0,
            retweeted: Some(false),
            retweeted_status: None,
            source: Some(TweetSource {
                name: "Twitter Web Client".to_string(),
                url: "http://twitter.com".to_string(),
            }),
            text: "Verhalten bei #Hausdurchsuchung https://t.co/AhiyYybK1m".to_string(),
            truncated: false,
            user: None,
            withheld_copyright: false,
            withheld_in_countries: None,
            withheld_scope: None,
        }
    }

    fn get_twitter_status_video() -> Tweet {
        // Reuse the media tweet and change it to video content.
        let mut tweet = get_twitter_status_media();
        // Set the attachment type to video.
        let media = tweet.entities.media.as_mut().unwrap();
        media[0].media_type = MediaType::Video;
        let extended_media = tweet.extended_entities.as_mut().unwrap();
        extended_media.media[0].media_type = MediaType::Video;

        extended_media.media[0].video_info = Some(VideoInfo {
            aspect_ratio: (9, 16),
            duration_millis: Some(10704),
            variants: vec![VideoVariant {
                bitrate: Some(320000),
                content_type: "video/mp4".parse().unwrap(),
                url: "https://video.twimg.com/ext_tw_video/869317980307415040/pu/vid/180x320/FMei8yCw7yc_Z7e-.mp4".to_string(),
            },
            VideoVariant {
                bitrate: Some(2176000),
                content_type: "video/mp4".parse().unwrap(),
                url: "https://video.twimg.com/ext_tw_video/869317980307415040/pu/vid/720x1280/octt5pFbISkef8RB.mp4".to_string(),
            },
            VideoVariant {
                bitrate: Some(832000),
                content_type: "video/mp4".parse().unwrap(),
                url: "https://video.twimg.com/ext_tw_video/869317980307415040/pu/vid/360x640/2OmqK74SQ9jNX8mZ.mp4".to_string(),
            },
            VideoVariant {
                bitrate: None,
                content_type: "application/x-mpegURL".parse().unwrap(),
                url: "https://video.twimg.com/ext_tw_video/869317980307415040/pu/pl/wcJQJ2nxiFU4ZZng.m3u8".to_string(),
            }],
        });
        tweet
    }

    pub fn get_twitter_user() -> TwitterUser {
        TwitterUser {
            contributors_enabled: false,
            created_at: Utc::now(),
            default_profile: false,
            default_profile_image: false,
            description: Some("test".to_string()),
            entities: UserEntities {
                description: UserEntityDetail { urls: Vec::new() },
                url: None,
            },
            favourites_count: 770,
            follow_request_sent: Some(false),
            followers_count: 1484,
            friends_count: 853,
            geo_enabled: false,
            id: 1,
            is_translator: false,
            lang: None,
            listed_count: 11,
            location: Some("Rustland".to_string()),
            name: "test user".to_string(),
            profile_background_color: "C0DEED".to_string(),
            profile_background_image_url: None,
            profile_background_image_url_https: None,
            profile_background_tile: Some(false),
            profile_banner_url: None,
            profile_image_url: "https://example.com".to_string(),
            profile_image_url_https: "https://example.com".to_string(),
            profile_link_color: "142DCF".to_string(),
            profile_sidebar_border_color: "C0DEED".to_string(),
            profile_sidebar_fill_color: "DDEEF6".to_string(),
            profile_text_color: "333333".to_string(),
            profile_use_background_image: true,
            protected: false,
            screen_name: "test123".to_string(),
            show_all_inline_media: None,
            status: None,
            statuses_count: 157,
            time_zone: None,
            url: None,
            utc_offset: None,
            verified: false,
            withheld_in_countries: None,
            withheld_scope: None,
        }
    }
}
