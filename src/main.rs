use clap::Parser;
use mastodon_twitter_sync::{args::Args, run};

fn main() {
    env_logger::init();

    let args = Args::parse();

    if let Err(err) = run(args) {
        eprintln!("Error: {err}");
        for cause in err.chain().skip(1) {
            eprintln!("Because: {cause}");
        }
        std::process::exit(1);
    }
}
