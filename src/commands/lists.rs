use anyhow::{bail, Result};

use crate::auth::oauth;
use crate::cli::ListsArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;

const LIST_FIELDS: &str =
    "list.fields=id,name,owner_id,private,description,created_at,follower_count,member_count";
const USER_FIELDS: &str = "user.fields=id,username,name,description,public_metrics";

pub async fn run(args: &ListsArgs, config: &Config, client: &XClient) -> Result<()> {
    let client_id = config.require_client_id()?;
    let (access_token, tokens) =
        oauth::get_valid_token(client, &config.tokens_path(), client_id).await?;

    let parts: Vec<String> = args.subcommand.clone().unwrap_or_default();
    let sub = parts.first().map(|s| s.as_str()).unwrap_or("list");

    match sub {
        "list" | "ls" => {
            cmd_list(
                args,
                &tokens.user_id,
                &tokens.username,
                &access_token,
                config,
                client,
            )
            .await
        }
        "create" => cmd_create(args, &parts, &tokens.user_id, &access_token, config, client).await,
        "update" => cmd_update(args, &parts, &access_token, config, client).await,
        "delete" | "remove" | "rm" => cmd_delete(args, &parts, &access_token, config, client).await,
        "members" | "member" => cmd_members(args, &parts, &access_token, config, client).await,
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        _ => {
            eprintln!("Unknown lists subcommand: {sub}");
            print_help();
            Ok(())
        }
    }
}

async fn cmd_list(
    args: &ListsArgs,
    user_id: &str,
    username: &str,
    access_token: &str,
    config: &Config,
    client: &XClient,
) -> Result<()> {
    let lists = fetch_owned_lists(client, user_id, access_token, args.limit.max(1)).await?;
    costs::track_cost(
        &config.costs_path(),
        "lists_list",
        &format!("/2/users/{user_id}/owned_lists"),
        lists.len() as u64,
    );

    if args.json {
        println!("{}", serde_json::to_string_pretty(&lists)?);
        return Ok(());
    }

    if lists.is_empty() {
        println!("No lists found.");
        return Ok(());
    }

    println!("\nLists — @{} ({})\n", username, lists.len());
    for (i, list) in lists.iter().enumerate() {
        let id = list.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        let name = list
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("(unnamed)");
        let is_private = list
            .get("private")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let member_count = list
            .get("member_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let follower_count = list
            .get("follower_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        println!(
            "{}. {} ({})",
            i + 1,
            name,
            if is_private { "private" } else { "public" }
        );
        println!("   id: {id} · {member_count} members · {follower_count} followers");
        if let Some(desc) = list.get("description").and_then(|v| v.as_str()) {
            println!("   {desc}");
        }
    }

    Ok(())
}

async fn cmd_create(
    args: &ListsArgs,
    parts: &[String],
    user_id: &str,
    access_token: &str,
    config: &Config,
    client: &XClient,
) -> Result<()> {
    if args.private && args.public {
        bail!("Use only one of --private or --public");
    }

    let name = compose_name(parts, args.name.as_deref()).ok_or_else(|| {
        anyhow::anyhow!("Usage: xint lists create <name> [--description \"...\"] [--private]")
    })?;

    let mut body = serde_json::json!({ "name": name });
    if let Some(desc) = &args.description {
        body["description"] = serde_json::json!(desc);
    }
    if args.private {
        body["private"] = serde_json::json!(true);
    } else if args.public {
        body["private"] = serde_json::json!(false);
    }

    let result = client
        .oauth_post("lists", access_token, Some(&body))
        .await?;
    costs::track_cost(&config.costs_path(), "lists_create", "/2/lists", 0);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        let id = result
            .pointer("/data/id")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        println!(
            "Created list \"{}\"",
            body["name"].as_str().unwrap_or("list")
        );
        println!("   id: {id}");
        println!("   owner: {user_id}");
    }

    Ok(())
}

async fn cmd_update(
    args: &ListsArgs,
    parts: &[String],
    access_token: &str,
    config: &Config,
    client: &XClient,
) -> Result<()> {
    if args.private && args.public {
        bail!("Use only one of --private or --public");
    }

    let list_id = parts
        .get(1)
        .map(String::as_str)
        .ok_or_else(|| anyhow::anyhow!("Usage: xint lists update <list_id> [--name \"...\"] [--description \"...\"] [--private|--public]"))?;

    let mut body = serde_json::json!({});
    if let Some(name) = &args.name {
        body["name"] = serde_json::json!(name);
    }
    if let Some(desc) = &args.description {
        body["description"] = serde_json::json!(desc);
    }
    if args.private {
        body["private"] = serde_json::json!(true);
    } else if args.public {
        body["private"] = serde_json::json!(false);
    }

    if body.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        bail!("No changes provided. Use --name, --description, --private, or --public.");
    }

    let result = client
        .oauth_put(&format!("lists/{list_id}"), access_token, Some(&body))
        .await?;
    costs::track_cost(
        &config.costs_path(),
        "lists_update",
        &format!("/2/lists/{list_id}"),
        0,
    );

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        let updated = result
            .pointer("/data/updated")
            .and_then(|v| v.as_bool())
            .unwrap_or_else(|| result.get("success").is_some());
        if updated {
            println!("Updated list {list_id}");
        } else {
            println!("No changes applied to list {list_id}");
        }
    }

    Ok(())
}

async fn cmd_delete(
    args: &ListsArgs,
    parts: &[String],
    access_token: &str,
    config: &Config,
    client: &XClient,
) -> Result<()> {
    let list_id = parts
        .get(1)
        .map(String::as_str)
        .ok_or_else(|| anyhow::anyhow!("Usage: xint lists delete <list_id>"))?;

    let result = client
        .oauth_delete(&format!("lists/{list_id}"), access_token)
        .await?;
    costs::track_cost(
        &config.costs_path(),
        "lists_delete",
        &format!("/2/lists/{list_id}"),
        0,
    );

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        let deleted = result
            .pointer("/data/deleted")
            .and_then(|v| v.as_bool())
            .unwrap_or_else(|| result.get("success").is_some());
        if deleted {
            println!("Deleted list {list_id}");
        } else {
            println!("Failed to delete list {list_id}");
        }
    }

    Ok(())
}

async fn cmd_members(
    args: &ListsArgs,
    parts: &[String],
    access_token: &str,
    config: &Config,
    client: &XClient,
) -> Result<()> {
    let action = parts.get(1).map(String::as_str).unwrap_or("list");
    match action {
        "list" | "ls" => {
            let list_id = parts.get(2).map(String::as_str).ok_or_else(|| {
                anyhow::anyhow!("Usage: xint lists members list <list_id> [--limit N] [--json]")
            })?;

            let members =
                fetch_list_members(client, list_id, access_token, args.limit.max(1)).await?;
            costs::track_cost(
                &config.costs_path(),
                "list_members_list",
                &format!("/2/lists/{list_id}/members"),
                members.len() as u64,
            );

            if args.json {
                println!("{}", serde_json::to_string_pretty(&members)?);
                return Ok(());
            }

            if members.is_empty() {
                println!("No members found in list {list_id}.");
                return Ok(());
            }

            println!("\nList Members ({}) — {}\n", members.len(), list_id);
            for (i, member) in members.iter().enumerate() {
                let handle = member
                    .get("username")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let name = member.get("name").and_then(|v| v.as_str()).unwrap_or("");
                println!(
                    "{}. @{}{}",
                    i + 1,
                    handle,
                    if name.is_empty() {
                        String::new()
                    } else {
                        format!(" — {name}")
                    }
                );
            }
            Ok(())
        }
        "add" => {
            let list_id = parts.get(2).map(String::as_str).ok_or_else(|| {
                anyhow::anyhow!("Usage: xint lists members add <list_id> <@username|user_id>")
            })?;
            let target = parts.get(3).map(String::as_str).ok_or_else(|| {
                anyhow::anyhow!("Usage: xint lists members add <list_id> <@username|user_id>")
            })?;

            let (target_user_id, target_username) =
                resolve_user_id(client, access_token, target).await?;
            let body = serde_json::json!({ "user_id": target_user_id });
            let result = client
                .oauth_post(
                    &format!("lists/{list_id}/members"),
                    access_token,
                    Some(&body),
                )
                .await?;

            costs::track_cost(
                &config.costs_path(),
                "list_members_add",
                &format!("/2/lists/{list_id}/members"),
                0,
            );

            if args.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                let is_member = result
                    .pointer("/data/is_member")
                    .and_then(|v| v.as_bool())
                    .unwrap_or_else(|| result.get("success").is_some());
                if is_member {
                    println!("Added @{target_username} to list {list_id}");
                } else {
                    println!("Failed to add @{target_username} to list {list_id}");
                }
            }
            Ok(())
        }
        "remove" | "rm" | "delete" => {
            let list_id = parts.get(2).map(String::as_str).ok_or_else(|| {
                anyhow::anyhow!("Usage: xint lists members remove <list_id> <@username|user_id>")
            })?;
            let target = parts.get(3).map(String::as_str).ok_or_else(|| {
                anyhow::anyhow!("Usage: xint lists members remove <list_id> <@username|user_id>")
            })?;

            let (target_user_id, target_username) =
                resolve_user_id(client, access_token, target).await?;
            let result = client
                .oauth_delete(
                    &format!("lists/{list_id}/members/{target_user_id}"),
                    access_token,
                )
                .await?;

            costs::track_cost(
                &config.costs_path(),
                "list_members_remove",
                &format!("/2/lists/{list_id}/members/{target_user_id}"),
                0,
            );

            if args.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                let is_member = result
                    .pointer("/data/is_member")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if !is_member || result.get("success").is_some() {
                    println!("Removed @{target_username} from list {list_id}");
                } else {
                    println!("Failed to remove @{target_username} from list {list_id}");
                }
            }
            Ok(())
        }
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        _ => {
            bail!("Unknown lists members subcommand: {action}");
        }
    }
}

async fn fetch_owned_lists(
    client: &XClient,
    user_id: &str,
    access_token: &str,
    max_total: usize,
) -> Result<Vec<serde_json::Value>> {
    let mut all = Vec::new();
    let mut next_token: Option<String> = None;

    while all.len() < max_total {
        let per_page = (max_total - all.len()).min(100);
        let pagination = match &next_token {
            Some(token) => format!("&pagination_token={token}"),
            None => String::new(),
        };
        let path =
            format!("users/{user_id}/owned_lists?max_results={per_page}&{LIST_FIELDS}{pagination}");

        let raw = client.oauth_get(&path, access_token).await?;
        let batch = raw
            .data
            .as_ref()
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();
        if batch.is_empty() {
            break;
        }
        all.extend(batch);
        next_token = raw.meta.and_then(|m| m.next_token);
        if next_token.is_none() {
            break;
        }
    }

    all.truncate(max_total);
    Ok(all)
}

async fn fetch_list_members(
    client: &XClient,
    list_id: &str,
    access_token: &str,
    max_total: usize,
) -> Result<Vec<serde_json::Value>> {
    let mut all = Vec::new();
    let mut next_token: Option<String> = None;

    while all.len() < max_total {
        let per_page = (max_total - all.len()).min(100);
        let pagination = match &next_token {
            Some(token) => format!("&pagination_token={token}"),
            None => String::new(),
        };
        let path =
            format!("lists/{list_id}/members?max_results={per_page}&{USER_FIELDS}{pagination}");
        let raw = client.oauth_get(&path, access_token).await?;
        let batch = raw
            .data
            .as_ref()
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();
        if batch.is_empty() {
            break;
        }
        all.extend(batch);
        next_token = raw.meta.and_then(|m| m.next_token);
        if next_token.is_none() {
            break;
        }
    }

    all.truncate(max_total);
    Ok(all)
}

async fn resolve_user_id(
    client: &XClient,
    access_token: &str,
    input: &str,
) -> Result<(String, String)> {
    let candidate = input.trim_start_matches('@');
    if is_likely_user_id(candidate) {
        return Ok((candidate.to_string(), candidate.to_string()));
    }

    let path = format!("users/by/username/{candidate}?{USER_FIELDS}");
    let raw = client.oauth_get(&path, access_token).await?;
    let user = raw
        .data
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("User @{candidate} not found"))?;
    let id = user
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("User @{candidate} not found"))?;
    let username = user
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or(candidate);
    Ok((id.to_string(), username.to_string()))
}

fn is_likely_user_id(input: &str) -> bool {
    !input.is_empty() && input.chars().all(|c| c.is_ascii_digit())
}

fn compose_name(parts: &[String], fallback: Option<&str>) -> Option<String> {
    if parts.len() > 1 {
        let joined = parts[1..].join(" ").trim().to_string();
        if !joined.is_empty() {
            return Some(joined);
        }
    }
    fallback
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn print_help() {
    println!("Usage: xint lists <subcommand> [options]");
    println!();
    println!("Subcommands:");
    println!(
        "  list [--limit N] [--json]                              List your owned lists (default)"
    );
    println!("  create <name> [--description \"...\"] [--private] [--json]");
    println!(
        "  update <list_id> [--name \"...\"] [--description \"...\"] [--private|--public] [--json]"
    );
    println!("  delete <list_id> [--json]");
    println!("  members list <list_id> [--limit N] [--json]");
    println!("  members add <list_id> <@username|user_id> [--json]");
    println!("  members remove <list_id> <@username|user_id> [--json]");
}

#[cfg(test)]
mod tests {
    use super::{compose_name, is_likely_user_id};

    #[test]
    fn test_is_likely_user_id() {
        assert!(is_likely_user_id("2244994945"));
        assert!(!is_likely_user_id("jack"));
        assert!(!is_likely_user_id("@jack"));
    }

    #[test]
    fn test_compose_name_from_parts() {
        let parts = vec![
            "create".to_string(),
            "AI".to_string(),
            "Researchers".to_string(),
        ];
        assert_eq!(
            compose_name(&parts, None).as_deref(),
            Some("AI Researchers")
        );
    }

    #[test]
    fn test_compose_name_from_fallback() {
        let parts = vec!["create".to_string()];
        assert_eq!(
            compose_name(&parts, Some("Fallback Name")).as_deref(),
            Some("Fallback Name")
        );
    }
}
