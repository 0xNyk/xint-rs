use anyhow::Result;

use crate::api::twitter;
use crate::cli::TweetArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::format;
use crate::output_meta;

pub async fn run(args: &TweetArgs, config: &Config, client: &XClient) -> Result<()> {
    let started_at = std::time::Instant::now();
    let token = config.require_bearer_token()?;

    let tweet = twitter::get_tweet(client, token, &args.tweet_id).await?;

    costs::track_cost(&config.costs_path(), "tweet", "/2/tweets", 1);

    match tweet {
        Some(t) => {
            if args.json {
                let meta = output_meta::build_meta(
                    "x_api_v2",
                    started_at,
                    false,
                    1.0,
                    &format!("/2/tweets/{}", args.tweet_id),
                    0.005,
                    &config.costs_path(),
                );
                output_meta::print_json_with_meta(&meta, &t)?;
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
