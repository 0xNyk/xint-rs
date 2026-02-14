use anyhow::Result;

use crate::api::twitter;
use crate::cli::ProfileArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::format;

pub async fn run(args: &ProfileArgs, config: &Config, client: &XClient) -> Result<()> {
    let token = config.require_bearer_token()?;
    let username = args.username.trim_start_matches('@');

    eprintln!("Fetching profile for @{}...", username);

    let (user, tweets) =
        twitter::get_profile(client, token, username, args.count, args.replies).await?;

    costs::track_cost(
        &config.costs_path(),
        "profile",
        &format!("/2/users/by/username/{}", username),
        tweets.len() as u64 + 1,
    );

    if args.json {
        let output = serde_json::json!({
            "user": user,
            "tweets": tweets,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("{}", format::format_profile_terminal(&user, &tweets));
    }

    Ok(())
}
