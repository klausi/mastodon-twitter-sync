use crate::sync::NewStatus;
use egg_mode::media::UploadBuilder;
use egg_mode::tweet::DraftTweet;
use egg_mode::tweet::Tweet;
use egg_mode::Token;
use mammut::entities::status::Status;
use mammut::status_builder::StatusBuilder;
use mammut::Mastodon;
use reqwest::header::CONTENT_TYPE;
use std::fs::remove_file;
use std::io::Read;
use tempfile::NamedTempFile;
use tokio::runtime::current_thread::block_on_all;

pub fn post_to_mastodon(mastodon: &Mastodon, toot: NewStatus) -> mammut::Result<Status> {
    let mut status = StatusBuilder::new(toot.text.clone());
    // Post attachments first, if there are any.
    for attachment in toot.attachments {
        let mut response = reqwest::get(&attachment.attachment_url)?;
        let mut tmpfile = NamedTempFile::new()?;
        ::std::io::copy(&mut response, &mut tmpfile)?;

        // Oh boy, this looks really bad. I could not use the path directly because
        // the compiler would not let me. Can this be simpler?
        let path = tmpfile.path().to_str().unwrap().to_string();
        let attachment = match attachment.alt_text {
            None => mastodon.media(path.into())?,
            Some(description) => mastodon.media_description(path.into(), description.into())?,
        };

        match status.media_ids.as_mut() {
            Some(ids) => {
                ids.push(attachment.id);
            }
            None => {
                status.media_ids = Some(vec![attachment.id]);
            }
        }
        remove_file(tmpfile)?;
    }

    mastodon.new_status(status)
}

/// Send a new status update to Twitter, including attachments.
pub fn post_to_twitter(token: &Token, tweet: NewStatus) -> Result<Tweet, failure::Error> {
    let mut draft = DraftTweet::new(tweet.text);
    let mut media_ids = Vec::new();
    for attachment in tweet.attachments {
        let mut response = reqwest::get(&attachment.attachment_url)?;
        let mut bytes = Vec::new();
        response.read_to_end(&mut bytes)?;
        let media_type = response
            .headers()
            .get(CONTENT_TYPE)
            // The ? operator does not work with Option :-(
            // All HTTP responses should have a content type, so we can just
            // panic here, YOLO!
            .unwrap()
            .to_str()?
            .parse::<mime::Mime>()?;
        let mut builder = UploadBuilder::new(bytes, media_type);
        if let Some(alt_text) = attachment.alt_text {
            builder = builder.alt_text(alt_text);
        }
        media_ids.push(block_on_all(builder.call(&token))?.id);
    }
    if !media_ids.is_empty() {
        draft = draft.media_ids(&media_ids);
    }
    let created_tweet = block_on_all(draft.send(&token))?;
    Ok((*created_tweet).clone())
}
