use crate::sync::NewStatus;
use mammut::entities::status::Status;
use mammut::status_builder::StatusBuilder;
use mammut::Mastodon;
use std::fs::remove_file;
use tempfile::NamedTempFile;

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
