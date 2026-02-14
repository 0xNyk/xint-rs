use anyhow::Result;

use crate::api::twitter;
use crate::cli::TweetArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::format;

pub async fn run(args: &TweetArgs, config: &Config, client: &XClient) -> Result<()> {
    let token = config.require_bearer_token()?;

    let tweet = twitter::get_tweet(client, token, &args.tweet_id).await?;

    costs::track_cost(&config.costs_path(), "tweet", "/2/tweets", 1);

    match tweet {
        Some(t) => {
            if args.json {
                println!("{}", serde_json::to_string_pretty(&t)?);
            } else {
                println!("{}", format::format_tweet_terminal(&t, None, true));
            }
        }
        None => {
            println!("Tweet {} not found.", args.tweet_id);
        }
    }

    Ok(())
}
