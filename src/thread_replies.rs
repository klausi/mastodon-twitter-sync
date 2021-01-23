use crate::sync::*;
use egg_mode::tweet::Tweet;
use elefren::entities::status::Status;

// A reply to a post that has the ID to the parent post.
#[derive(Debug)]
struct Reply {
    pub id: u64,
    pub text: String,
    pub attachments: Vec<NewMedia>,
    pub in_reply_to_id: u64,
}

// Check if there are thread replies that we want to sync.
pub fn determine_thread_replies(
    mastodon_statuses: &[Status],
    twitter_statuses: &[Tweet],
    options: &SyncOptions,
    sync_statuses: &mut StatusUpdates,
) {
    // Collect replies in reverse order to post the oldest first.
    let mut replies = Vec::new();
    'tweets: for tweet in twitter_statuses {
        // Check if this is a reply to a tweet of this user.
        if let Some(user_id) = &tweet.in_reply_to_user_id {
            if user_id != &tweet.user.as_ref().unwrap().id {
                continue;
            }

            for toot in mastodon_statuses {
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

            // Insert this reply in the beginning to reverse order.
            replies.insert(
                0,
                Reply {
                    id: tweet.id,
                    text: decoded_tweet,
                    attachments: tweet_get_attachments(tweet),
                    in_reply_to_id: tweet.in_reply_to_status_id.unwrap(),
                },
            );
        }
    }
    insert_twitter_replies(
        &mut sync_statuses.toots,
        replies,
        twitter_statuses,
        mastodon_statuses,
    );
}

// Insert Twitter replies with the correct Mastodon parent status ID.
// If the status does not exist yet then insert as reply after a new status
// correctly.
fn insert_twitter_replies(
    sync_statuses: &mut Vec<NewStatus>,
    replies: Vec<Reply>,
    twitter_statuses: &[Tweet],
    mastodon_statuses: &[Status],
) {
    'reply_loop: for reply in replies {
        // Check new statuses first if it is a reply to that.
        for sync_status in &mut *sync_statuses {
            if insert_reply_on_status(sync_status, &reply) {
                continue 'reply_loop;
            }
        }
        // Check existing statuses if the parent is there.
        for tweet in twitter_statuses {
            if tweet.id == reply.in_reply_to_id {
                for toot in mastodon_statuses {
                    // If we get a status with the same text then we assume this
                    // must be the corresponding parent.
                    if toot_and_tweet_are_equal(toot, tweet) {
                        sync_statuses.push(NewStatus {
                            text: reply.text.clone(),
                            attachments: reply.attachments.clone(),
                            replies: Vec::new(),
                            in_reply_to_id: Some(toot.id.parse().unwrap()),
                            original_id: reply.id,
                        });
                        continue 'reply_loop;
                    }
                }
            }
        }
    }
}

// Check if the status is the parent of the reply or any of its already set
// replies.
fn insert_reply_on_status(status: &mut NewStatus, reply: &Reply) -> bool {
    if reply.in_reply_to_id == status.original_id {
        status.replies.push(NewStatus {
            text: reply.text.clone(),
            attachments: reply.attachments.clone(),
            replies: Vec::new(),
            in_reply_to_id: None,
            original_id: reply.id,
        });
        return true;
    }
    for existing_reply in &mut status.replies {
        if insert_reply_on_status(existing_reply, reply) == true {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::sync::tests::*;

    static DEFAULT_SYNC_OPTIONS: SyncOptions = SyncOptions {
        sync_reblogs: true,
        sync_retweets: true,
        sync_hashtag_twitter: None,
        sync_hashtag_mastodon: None,
    };

    // Tests that a reply to your own tweet is synced as thread reply to
    // Mastodon.
    #[test]
    fn sync_twitter_thread_reply() {
        let mut original_tweet = get_twitter_status();
        original_tweet.user = Some(Box::new(get_twitter_user()));
        original_tweet.text = "Original".to_string();
        let mut reply_tweet = get_twitter_status();
        reply_tweet.user = Some(Box::new(get_twitter_user()));
        reply_tweet.text = "Reply".to_string();
        reply_tweet.in_reply_to_user_id = Some(original_tweet.user.clone().unwrap().id);
        reply_tweet.in_reply_to_status_id = Some(original_tweet.id.clone());

        let tweets = vec![reply_tweet, original_tweet];
        let toots = Vec::new();
        let posts = determine_posts(&toots, &tweets, &DEFAULT_SYNC_OPTIONS);

        assert_eq!(posts.toots.len(), 1);
        let sync_toot = &posts.toots[0];
        assert_eq!(sync_toot.text, "Original");
        assert_eq!(sync_toot.replies[0].text, "Reply");
    }

    // Tests that a reply for a tweet that has already been synced is also
    // synced on a subsequent run.
    #[test]
    fn sync_twitter_reply_to_older_post() {
        let mut original_tweet = get_twitter_status();
        original_tweet.user = Some(Box::new(get_twitter_user()));
        original_tweet.text = "Original".to_string();
        let mut reply_tweet = get_twitter_status();
        reply_tweet.user = Some(Box::new(get_twitter_user()));
        reply_tweet.text = "Reply".to_string();
        reply_tweet.in_reply_to_user_id = Some(original_tweet.user.clone().unwrap().id);
        reply_tweet.in_reply_to_status_id = Some(original_tweet.id.clone());

        let mut status = get_mastodon_status();
        status.content = "Original".to_string();

        let tweets = vec![reply_tweet, original_tweet];
        let toots = vec![status];
        let posts = determine_posts(&toots, &tweets, &DEFAULT_SYNC_OPTIONS);

        assert_eq!(posts.toots.len(), 1);
        let sync_toot = &posts.toots[0];
        assert_eq!(sync_toot.text, "Reply");
        assert!(sync_toot.in_reply_to_id.is_some());
        assert_eq!(
            sync_toot.in_reply_to_id.unwrap(),
            toots[0].id.parse::<u64>().unwrap()
        );
        assert!(sync_toot.replies.is_empty());
    }

    // Tests that multiple new replies for a tweet are synced in the right order
    // to Mastodon
    #[test]
    fn sync_multiple_new_replies() {
        let mut original_tweet = get_twitter_status();
        original_tweet.id = 1;
        original_tweet.user = Some(Box::new(get_twitter_user()));
        original_tweet.text = "Original".to_string();
        let mut reply1_tweet = get_twitter_status();
        reply1_tweet.id = 2;
        reply1_tweet.user = Some(Box::new(get_twitter_user()));
        reply1_tweet.text = "Reply1".to_string();
        reply1_tweet.in_reply_to_user_id = Some(original_tweet.user.clone().unwrap().id);
        reply1_tweet.in_reply_to_status_id = Some(original_tweet.id.clone());
        let mut reply2_tweet = get_twitter_status();
        reply2_tweet.id = 3;
        reply2_tweet.user = Some(Box::new(get_twitter_user()));
        reply2_tweet.text = "Reply2".to_string();
        reply2_tweet.in_reply_to_user_id = Some(original_tweet.user.clone().unwrap().id);
        reply2_tweet.in_reply_to_status_id = Some(reply1_tweet.id.clone());
        let mut reply3_tweet = get_twitter_status();
        reply3_tweet.id = 4;
        reply3_tweet.user = Some(Box::new(get_twitter_user()));
        reply3_tweet.text = "Reply3".to_string();
        reply3_tweet.in_reply_to_user_id = Some(original_tweet.user.clone().unwrap().id);
        reply3_tweet.in_reply_to_status_id = Some(reply2_tweet.id.clone());

        let mut status = get_mastodon_status();
        status.content = "Original".to_string();

        let tweets = vec![reply3_tweet, reply2_tweet, reply1_tweet, original_tweet];
        let toots = vec![status];
        let posts = determine_posts(&toots, &tweets, &DEFAULT_SYNC_OPTIONS);

        assert_eq!(posts.toots.len(), 1);
        let reply1_toot = &posts.toots[0];
        assert_eq!(reply1_toot.text, "Reply1");
        assert!(reply1_toot.in_reply_to_id.is_some());
        assert_eq!(
            reply1_toot.in_reply_to_id.unwrap(),
            toots[0].id.parse::<u64>().unwrap()
        );
        assert_eq!(reply1_toot.replies.len(), 1);

        let reply2_toot = &reply1_toot.replies[0];
        assert_eq!(reply2_toot.text, "Reply2");
        assert!(reply2_toot.in_reply_to_id.is_none());
        assert_eq!(reply2_toot.replies.len(), 1);

        let reply3_toot = &reply2_toot.replies[0];
        assert_eq!(reply3_toot.text, "Reply3");
        assert!(reply3_toot.in_reply_to_id.is_none());
        assert!(reply3_toot.replies.is_empty());
    }
}
