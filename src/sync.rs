use crate::errors::*;
use egg_mode::tweet::Tweet;
use egg_mode_text::character_count;
use mammut::entities::status::Status;
use regex::Regex;
use std::collections::HashSet;
use std::fs;

// Represents new status updates that should be posted to Twitter (tweets) and
// Mastodon (toots).
#[derive(Debug, Clone)]
pub struct StatusUpdates {
    pub tweets: Vec<NewStatus>,
    pub toots: Vec<NewStatus>,
}

// A new status for posting. Optionally has links to media (images) that should
// be attached.
#[derive(Debug, Clone)]
pub struct NewStatus {
    pub text: String,
    pub attachments: Vec<NewMedia>,
}

#[derive(Debug, Clone)]
pub struct NewMedia {
    pub attachment_url: String,
    pub alt_text: Option<String>,
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
        updates.toots.push(NewStatus {
            text: tweet_unshorten_decode(tweet),
            attachments: tweet_get_attachments(tweet),
        });
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
        updates.tweets.push(NewStatus {
            text: post,
            attachments: toot_get_attachments(toot),
        });
    }
    updates
}

// Returns true if a Mastodon toot and a Twitter tweet are considered equal.
fn toot_and_tweet_are_equal(toot: &Status, tweet: &Tweet) -> bool {
    // Strip markup from Mastodon toot.
    let toot_text = mastodon_toot_get_text(toot);
    let mut toot_compare = toot_text.to_lowercase();
    // Remove http:// and https:// for comparing because Twitter sometimes adds
    // those randomly.
    toot_compare = toot_compare.replace("http://", "");
    toot_compare = toot_compare.replace("https://", "");
    // Replace those ugly t.co URLs in the tweet text.
    let tweet_text = tweet_unshorten_decode(tweet);
    let mut tweet_compare = tweet_text.to_lowercase();
    tweet_compare = tweet_compare.replace("http://", "");
    tweet_compare = tweet_compare.replace("https://", "");

    if toot_compare == tweet_compare {
        return true;
    }
    // Mastodon allows up to 500 characters, so we might need to shorten the
    // toot.
    let shortened_toot = tweet_shorten(&toot_text, &toot.url);
    let mut shortened_toot_compare = shortened_toot.to_lowercase();
    shortened_toot_compare = shortened_toot_compare.replace("http://", "");
    shortened_toot_compare = shortened_toot_compare.replace("https://", "");

    if shortened_toot_compare == tweet_compare {
        return true;
    }

    // Support for old posts that started with "RT @username:", we consider them
    // equal to "RT username:".
    if tweet_compare.starts_with("rt @") {
        let old_rt = tweet_compare.replacen("rt @", "rt ", 1);
        if old_rt == toot_compare || old_rt == shortened_toot_compare {
            return true;
        }
    }
    if toot_compare.starts_with("rt @") {
        let old_rt = toot_compare.replacen("rt @", "rt ", 1);
        if old_rt == tweet_compare {
            return true;
        }
    }
    if shortened_toot_compare.starts_with("rt @") {
        let old_rt = shortened_toot_compare.replacen("rt @", "rt ", 1);
        if old_rt == tweet_compare {
            return true;
        }
    }

    false
}

// Replace t.co URLs and HTML entity decode &amp;
fn tweet_unshorten_decode(tweet: &Tweet) -> String {
    let (mut tweet_text, urls, media) = match tweet.retweeted_status {
        None => (
            tweet.text.clone(),
            &tweet.entities.urls,
            &tweet.extended_entities,
        ),
        Some(ref retweet) => (
            format!(
                "RT {}: {}",
                retweet.clone().user.unwrap().screen_name,
                retweet.text
            ),
            &retweet.entities.urls,
            &retweet.extended_entities,
        ),
    };
    // Replace t.co URLs with the real links in tweets.
    for url in urls {
        if let Some(expanded_url) = &url.expanded_url {
            tweet_text = tweet_text.replace(&url.url, &expanded_url);
        }
    }
    // Remove the last media link if there is one, we will upload attachments
    // directly to Mastodon.
    if let Some(media) = media {
        for attachment in &media.media {
            tweet_text = tweet_text.replace(&attachment.url, "");
        }
    }
    tweet_text = tweet_text.trim().to_string();

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
pub fn filter_posted_before(posts: StatusUpdates, dry_run: bool) -> Result<StatusUpdates> {
    // If there are not status updates then we don't need to check anything.
    if posts.toots.is_empty() && posts.tweets.is_empty() {
        return Ok(posts);
    }

    let cache_file = "post_cache.json";
    let mut cache = read_post_cache(cache_file);
    let mut filtered_posts = StatusUpdates {
        tweets: Vec::new(),
        toots: Vec::new(),
    };
    for tweet in posts.tweets {
        if cache.contains(&tweet.text) {
            println!(
                "Error: preventing double posting to Twitter: {}",
                tweet.text
            );
        } else {
            filtered_posts.tweets.push(tweet.clone());
            cache.insert(tweet.text);
        }
    }
    for toot in posts.toots {
        if cache.contains(&toot.text) {
            println!(
                "Error: preventing double posting to Mastodon: {}",
                toot.text
            );
        } else {
            filtered_posts.toots.push(toot.clone());
            cache.insert(toot.text);
        }
    }

    if !dry_run {
        let json = serde_json::to_string(&cache)?;
        fs::write(cache_file, json.as_bytes())?;
    }

    Ok(filtered_posts)
}

// Read the JSON encoded cache file from disk or provide en empty default cache.
fn read_post_cache(cache_file: &str) -> HashSet<String> {
    match fs::read_to_string(cache_file) {
        Ok(json) => {
            match serde_json::from_str::<HashSet<String>>(&json) {
                Ok(cache) => {
                    // If the cache has more than 50 items already then empty it to not
                    // accumulate too many items and allow posting the same text at a
                    // later date.
                    if cache.len() > 50 {
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
fn tweet_get_attachments(tweet: &Tweet) -> Vec<NewMedia> {
    let mut links = Vec::new();
    // Check if there are attachments directly on the tweet, otherwise try to
    // use attachments from retweets.
    let media = match &tweet.extended_entities {
        Some(media) => Some(media),
        None => {
            let mut retweet_media = None;
            if let Some(retweet) = &tweet.retweeted_status {
                if let Some(media) = &retweet.extended_entities {
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
fn toot_get_attachments(toot: &Status) -> Vec<NewMedia> {
    let mut links = Vec::new();
    for attachment in &toot.media_attachments {
        // Only images are supported for now, no videos.
        if let mammut::entities::attachment::MediaType::Image = attachment.media_type {
            links.push(NewMedia {
                attachment_url: attachment.url.clone(),
                alt_text: attachment.description.clone(),
            });
        }
    }
    links
}

#[cfg(test)]
mod tests {

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
        assert_eq!(posts.tweets[0].text, "You & me!");
    }

    // Test that Twitter status text is posted HTML entity decoded to Mastodon.
    // &amp; => &
    #[test]
    fn twitter_html_decode() {
        let mut status = get_twitter_status();
        status.text = "You &amp; me!".to_string();
        let posts = determine_posts(&Vec::new(), &vec![status]);
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

        let posts = determine_posts(&vec![status], &Vec::new());
        assert_eq!(posts.tweets[0].text, "RT example: Some example toooot!");
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
        let posts = determine_posts(&statuses, &tweets);

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
        let posts = determine_posts(&statuses, &tweets);

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
        let posts = determine_posts(&statuses, &tweets);

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
        let posts = determine_posts(&toots, &tweets);

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

    fn get_mastodon_status() -> Status {
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
            source: TweetSource {
                name: "Twitter Web Client".to_string(),
                url: "http://twitter.com".to_string(),
            },
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

    fn get_twitter_user() -> TwitterUser {
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
