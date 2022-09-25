use clap::StructOpt;

#[derive(Debug, StructOpt)]
pub struct Args {
    /// Config file
    #[structopt(
        short = 'c',
        long = "config",
        default_value = "mastodon-twitter-sync.toml"
    )]
    pub config: String,
    /// Dry run
    #[structopt(short = 'n', long = "dry-run")]
    pub dry_run: bool,
    /// Skip all existing posts, use this if you only want to sync future posts
    #[structopt(long = "skip-existing-posts")]
    pub skip_existing_posts: bool,
}
