use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct Args {
    /// Config file
    #[structopt(short = "c", long = "config", default_value = "mastodon-twitter-sync.toml")]
    pub config: String,
    /// Dry run
    #[structopt(short = "n", long = "dry-run")]
    pub dry_run: bool,
}
