use anyhow::Result;

use crate::api::twitter;
use crate::cli::RepostsArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::format;

pub async fn run(args: &RepostsArgs, config: &Config, client: &XClient) -> Result<()> {
    let token = config.require_bearer_token()?;

    let spinner =
        crate::spinner::Spinner::new(&format!("Fetching reposts for tweet {}...", args.tweet_id));
    let result = twitter::get_reposts(client, token, &args.tweet_id, args.limit as u32).await;
    match &result {
        Ok(users) => spinner.done(&format!("{} users reposted", users.len())),
        Err(_) => spinner.fail("Failed to fetch reposts"),
    }
    let users = result?;

    costs::track_cost(
        &config.costs_path(),
        "reposts",
        &format!("/2/tweets/{}/retweeted_by", args.tweet_id),
        users.len() as u64,
    );

    let users: Vec<_> = users.into_iter().take(args.limit).collect();

    if args.json {
        format::print_json_pretty_filtered(&users)?;
        return Ok(());
    }

    if users.is_empty() {
        println!("No reposts found for tweet {}", args.tweet_id);
        return Ok(());
    }

    println!(
        "\nReposts of tweet {} ({} users)\n",
        args.tweet_id,
        users.len()
    );

    for (i, u) in users.iter().enumerate() {
        let username = u.get("username").and_then(|v| v.as_str()).unwrap_or("?");
        let name = u.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let followers = u
            .pointer("/public_metrics/followers_count")
            .and_then(|v| v.as_u64());
        let followers_str = match followers {
            Some(n) => format!(" ({n} followers)"),
            None => String::new(),
        };
        println!("{}. @{} — {}{}", i + 1, username, name, followers_str);

        if let Some(desc) = u.get("description").and_then(|v| v.as_str()) {
            if !desc.is_empty() {
                let short = if desc.len() > 200 { &desc[..200] } else { desc };
                println!("   {}", short.replace('\n', " "));
            }
        }
    }

    Ok(())
}
