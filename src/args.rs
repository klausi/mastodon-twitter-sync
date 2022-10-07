use clap::Parser;

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Config file
    #[arg(
        short = 'c',
        long = "config",
        default_value = "mastodon-twitter-sync.toml"
    )]
    pub config: String,
    /// Dry run
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,
    /// Skip all existing posts, use this if you only want to sync future posts
    #[arg(long = "skip-existing-posts")]
    pub skip_existing_posts: bool,
}
