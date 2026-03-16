use anyhow::{bail, Result};

use crate::api::twitter;
use crate::cli::UsersArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::format;

pub async fn run(args: &UsersArgs, config: &Config, client: &XClient) -> Result<()> {
    let token = config.require_bearer_token()?;
    let query = args.query.join(" ");
    if query.is_empty() {
        bail!("Search query required. Usage: xint users <query>");
    }

    let spinner = crate::spinner::Spinner::new(&format!("Searching users for \"{query}\"..."));
    let result = twitter::search_users(client, token, &query, args.limit as u32).await;
    match &result {
        Ok(users) => spinner.done(&format!("{} users found", users.len())),
        Err(_) => spinner.fail("User search failed"),
    }
    let users = result?;

    costs::track_cost(
        &config.costs_path(),
        "users_search",
        "/2/users/search",
        users.len() as u64,
    );

    let users: Vec<_> = users.into_iter().take(args.limit).collect();

    if args.json {
        format::print_json_pretty_filtered(&users)?;
        return Ok(());
    }

    if users.is_empty() {
        println!("No users found for \"{}\"", query);
        return Ok(());
    }

    println!("\nUser search: \"{}\" ({} results)\n", query, users.len());

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
