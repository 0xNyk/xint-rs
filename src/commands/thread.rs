use anyhow::Result;

use crate::api::twitter;
use crate::cli::ThreadArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::format;

pub async fn run(args: &ThreadArgs, config: &Config, client: &XClient) -> Result<()> {
    let token = config.require_bearer_token()?;

    let spinner = crate::spinner::Spinner::new(&format!("Fetching thread {}...", args.tweet_id));
    let result = twitter::get_thread(client, token, &args.tweet_id, args.pages).await;
    match &result {
        Ok(tweets) => spinner.done(&format!("Thread: {} tweets", tweets.len())),
        Err(_) => spinner.fail("Failed to fetch thread"),
    }
    let tweets = result?;

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
