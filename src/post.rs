use crate::errors::*;
use crate::sync::NewStatus;
use egg_mode::media::{set_metadata, upload_media};
use egg_mode::tweet::DraftTweet;
use egg_mode::tweet::Tweet;
use egg_mode::Token;
use elefren::entities::status::Status;
use elefren::media_builder::MediaBuilder;
use elefren::status_builder::StatusBuilder;
use elefren::Mastodon;
use elefren::MastodonClient;
use failure::format_err;
use reqwest::header::CONTENT_TYPE;
use std::fs::remove_file;
use tempfile::NamedTempFile;
use tokio::fs::File;
use tokio::prelude::*;

/// Send new status with any given replies to Mastodon.
pub async fn post_to_mastodon(mastodon: &Mastodon, toot: &NewStatus, dry_run: bool) -> Result<()> {
    if let Some(reply_to) = toot.in_reply_to_id {
        println!(
            "Posting thread reply for {} to Mastodon: {}",
            reply_to, toot.text
        );
    } else {
        println!("Posting to Mastodon: {}", toot.text);
    }
    let mut status_id = "0".to_string();
    if !dry_run {
        let status = send_single_post_to_mastodon(mastodon, toot).await?;
        status_id = status.id;
    }

    // Recursion does not work well with async functions, so we use iteration
    // here instead.
    let mut replies = Vec::new();
    for reply in &toot.replies {
        replies.push((status_id.clone(), reply));
    }

    while !replies.is_empty() {
        let (parent_id, reply) = replies.remove(0);
        let mut new_reply = reply.clone();
        // Set the new ID of the parent status to reply to.
        new_reply.in_reply_to_id = Some(parent_id.parse().unwrap());

        println!("Posting thread reply to Mastodon: {}", reply.text);
        let mut parent_status_id = "0".to_string();
        if !dry_run {
            let parent_status = send_single_post_to_mastodon(mastodon, &new_reply).await?;
            parent_status_id = parent_status.id;
        }
        for remaining_reply in &reply.replies {
            replies.push((parent_status_id.clone(), remaining_reply));
        }
    }

    Ok(())
}

/// Sends the given new status to Mastodon.
pub async fn send_single_post_to_mastodon(mastodon: &Mastodon, toot: &NewStatus) -> Result<Status> {
    let mut media_ids = Vec::new();
    // Post attachments first, if there are any.
    for attachment in &toot.attachments {
        // Because we use async for egg-mode we also need to use reqwest in
        // async mode. Otherwise we get double async executor errors.
        let response = reqwest::get(&attachment.attachment_url)
            .await
            .context(format!(
                "Failed downloading attachment {}",
                attachment.attachment_url
            ))?;
        let tmpfile = NamedTempFile::new()?;

        // Oh boy, this looks really bad. I could not use the path directly because
        // the compiler would not let me. Can this be simpler?
        let path = tmpfile.path().to_str().unwrap().to_string();

        let mut file = File::create(&path).await?;
        file.write_all(&response.bytes().await?).await?;

        let attachment = match &attachment.alt_text {
            None => wrap_elefren_error(mastodon.media(path.into()))?,
            Some(description) => wrap_elefren_error(mastodon.media(MediaBuilder {
                file: path.into(),
                description: Some(description.clone().into()),
                focus: None,
            }))?,
        };

        media_ids.push(attachment.id);
        remove_file(tmpfile)?;
    }

    let mut status_builder = StatusBuilder::new();
    status_builder.status(&toot.text);
    status_builder.media_ids(media_ids);
    if let Some(parent_id) = toot.in_reply_to_id {
        status_builder.in_reply_to(parent_id.to_string());
    }
    let status = wrap_elefren_error(status_builder.build())?;

    wrap_elefren_error(mastodon.new_status(status))
}

/// Send a new status update to Twitter, including attachments.
pub async fn post_to_twitter(token: &Token, tweet: &NewStatus) -> Result<Tweet> {
    let mut draft = DraftTweet::new(tweet.text.clone());
    for attachment in &tweet.attachments {
        let response = reqwest::get(&attachment.attachment_url).await?;
        let media_type = response
            .headers()
            .get(CONTENT_TYPE)
            .ok_or_else(|| format_err!("Missing content-type on response"))?
            .to_str()?
            .parse::<mime::Mime>()?;
        let bytes = response.bytes().await?;
        let media_handle = upload_media(&bytes, &media_type, &token).await?;
        draft.add_media(media_handle.id.clone());
        if let Some(alt_text) = &attachment.alt_text {
            set_metadata(&media_handle.id, &alt_text, &token).await?;
        }
    }
    let created_tweet = draft.send(&token).await?;
    Ok((*created_tweet).clone())
}
