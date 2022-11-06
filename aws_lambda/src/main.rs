use lambda_runtime::{service_fn, Error, LambdaEvent};
use mastodon_twitter_sync::args::Args;
use serde_json::Value;

async fn function_handler(_event: LambdaEvent<Value>) -> Result<(), Error> {
    let paths = std::fs::read_dir("/tmp").unwrap();

    println!("Files in /tmp:");
    for path in paths {
        println!("Name: {}", path.unwrap().path().display())
    }
    println!("===== End files in /tmp.");

    let args = Args {
        config: format!(
            "{}/mastodon-twitter-sync.toml",
            std::env::var("LAMBDA_TASK_ROOT").unwrap()
        ),
        dry_run: false,
        skip_existing_posts: false,
    };

    mastodon_twitter_sync::run(args)?;

    Ok(())
}

fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        // disable printing the name of the module in every log line.
        .with_target(false)
        // disabling time is handy because CloudWatch will add the ingestion time.
        .without_time()
        .init();

    let paths = std::fs::read_dir("/tmp").unwrap();

    println!("Files in /tmp:");
    for path in paths {
        println!("Name: {}", path.unwrap().path().display())
    }
    println!("===== End files in /tmp.");

    let args = Args {
        config: format!(
            "{}/mastodon-twitter-sync.toml",
            std::env::var("LAMBDA_TASK_ROOT").unwrap()
        ),
        dry_run: false,
        skip_existing_posts: false,
    };

    mastodon_twitter_sync::run(args)?;

    Ok(())
}
