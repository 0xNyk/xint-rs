use anyhow::Result;

use crate::api::twitter;
use crate::cli::ThreadArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::format;

pub async fn run(args: &ThreadArgs, config: &Config, client: &XClient) -> Result<()> {
    let token = config.require_bearer_token()?;

    eprintln!("Fetching thread {}...", args.tweet_id);

    let tweets = twitter::get_thread(client, token, &args.tweet_id, args.pages).await?;

    costs::track_cost(
        &config.costs_path(),
        "thread",
        "/2/tweets/search/recent",
        tweets.len() as u64,
    );

    if tweets.is_empty() {
        println!("No tweets found in thread.");
        return Ok(());
    }

    println!("\nThread ({} tweets):\n", tweets.len());
    for (i, t) in tweets.iter().enumerate() {
        if i > 0 {
            println!();
        }
        println!("{}", format::format_tweet_terminal(t, Some(i), true));
    }

    Ok(())
}
