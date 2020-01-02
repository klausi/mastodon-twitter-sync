use crate::errors::*;
use crate::sync::NewStatus;
use egg_mode::media::UploadBuilder;
use egg_mode::tweet::DraftTweet;
use egg_mode::tweet::Tweet;
use egg_mode::Token;
use failure::format_err;
use mammut::entities::status::Status;
use mammut::media_builder::MediaBuilder;
use mammut::status_builder::StatusBuilder;
use mammut::Mastodon;
use reqwest::header::CONTENT_TYPE;
use std::fs::remove_file;
use std::io::Read;
use tempfile::NamedTempFile;
use tokio::runtime::current_thread::block_on_all;

pub fn post_to_mastodon(mastodon: &Mastodon, toot: NewStatus) -> Result<Status> {
    let mut status = StatusBuilder::new(toot.text.clone());
    // Post attachments first, if there are any.
    for attachment in toot.attachments {
        let mut response = reqwest::blocking::get(&attachment.attachment_url).context(format!(
            "Failed downloading attachment {}",
            attachment.attachment_url
        ))?;
        let mut tmpfile = NamedTempFile::new()?;
        ::std::io::copy(&mut response, &mut tmpfile)?;

        // Oh boy, this looks really bad. I could not use the path directly because
        // the compiler would not let me. Can this be simpler?
        let path = tmpfile.path().to_str().unwrap().to_string();
        let attachment = match attachment.alt_text {
            None => mastodon.media(path.into())?,
            Some(description) => mastodon.media(MediaBuilder {
                file: path.into(),
                description: Some(description.into()),
                focus: None,
            })?,
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

    match mastodon.new_status(status) {
        Ok(s) => Ok(s),
        Err(e) => Err(e.into()),
    }
}

/// Send a new status update to Twitter, including attachments.
pub fn post_to_twitter(token: &Token, tweet: NewStatus) -> Result<Tweet> {
    let mut draft = DraftTweet::new(tweet.text);
    let mut media_ids = Vec::new();
    for attachment in tweet.attachments {
        let mut response = reqwest::blocking::get(&attachment.attachment_url)?;
        let mut bytes = Vec::new();
        response.read_to_end(&mut bytes)?;
        let media_type = response
            .headers()
            .get(CONTENT_TYPE)
            .ok_or_else(|| format_err!("Missing content-type on response"))?
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
