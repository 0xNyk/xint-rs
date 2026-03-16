use anyhow::Result;

use crate::api::twitter;
use crate::cli::ProfileArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::format;
use crate::output_meta;

pub async fn run(args: &ProfileArgs, config: &Config, client: &XClient) -> Result<()> {
    let started_at = std::time::Instant::now();
    let token = config.require_bearer_token()?;
    let username = args.username.trim_start_matches('@');

    let spinner = crate::spinner::Spinner::new(&format!("Fetching profile for @{username}..."));
    let result = twitter::get_profile(client, token, username, args.count, args.replies).await;
    match &result {
        Ok((_, tweets)) => spinner.done(&format!("@{username}: {} tweets", tweets.len())),
        Err(_) => spinner.fail(&format!("Failed to fetch @{username}")),
    }
    let (user, tweets) = result?;

    costs::track_cost(
        &config.costs_path(),
        "profile",
        &format!("/2/users/by/username/{username}"),
        tweets.len() as u64 + 1,
    );

    if args.json {
        let est_cost = (tweets.len() as f64 + 1.0) * 0.005;
        let output = serde_json::json!({
            "user": user,
            "tweets": tweets,
        });
        let meta = output_meta::build_meta(
            "x_api_v2",
            started_at,
            false,
            1.0,
            &format!("/2/users/by/username/{username}"),
            est_cost,
            &config.costs_path(),
        );
        output_meta::print_json_with_meta(&meta, &output)?;
    } else {
        println!("{}", format::format_profile_terminal(&user, &tweets));
    }

    Ok(())
}
