use anyhow::Result;

use crate::auth::oauth;
use crate::cli::ModerationArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;

const USER_FIELDS: &str = "user.fields=id,username,name,description,public_metrics";

enum Mode {
    Blocks,
    Mutes,
}

impl Mode {
    fn noun(&self) -> &'static str {
        match self {
            Mode::Blocks => "blocks",
            Mode::Mutes => "mutes",
        }
    }

    fn path(&self) -> &'static str {
        match self {
            Mode::Blocks => "blocking",
            Mode::Mutes => "muting",
        }
    }

    fn list_op(&self) -> &'static str {
        match self {
            Mode::Blocks => "blocks_list",
            Mode::Mutes => "mutes_list",
        }
    }

    fn add_op(&self) -> &'static str {
        match self {
            Mode::Blocks => "blocks_add",
            Mode::Mutes => "mutes_add",
        }
    }

    fn remove_op(&self) -> &'static str {
        match self {
            Mode::Blocks => "blocks_remove",
            Mode::Mutes => "mutes_remove",
        }
    }

    fn action_add(&self) -> &'static str {
        match self {
            Mode::Blocks => "block",
            Mode::Mutes => "mute",
        }
    }

    fn action_remove(&self) -> &'static str {
        match self {
            Mode::Blocks => "unblock",
            Mode::Mutes => "unmute",
        }
    }
}

pub async fn run_blocks(args: &ModerationArgs, config: &Config, client: &XClient) -> Result<()> {
    run_mode(Mode::Blocks, args, config, client).await
}

pub async fn run_mutes(args: &ModerationArgs, config: &Config, client: &XClient) -> Result<()> {
    run_mode(Mode::Mutes, args, config, client).await
}

async fn run_mode(
    mode: Mode,
    args: &ModerationArgs,
    config: &Config,
    client: &XClient,
) -> Result<()> {
    let client_id = config.require_client_id()?;
    let (access_token, tokens) =
        oauth::get_valid_token(client, &config.tokens_path(), client_id).await?;

    let parts: Vec<String> = args.subcommand.clone().unwrap_or_default();
    let sub = parts.first().map(String::as_str).unwrap_or("list");

    match sub {
        "list" | "ls" => {
            cmd_list(
                &mode,
                args,
                &tokens.user_id,
                &tokens.username,
                &access_token,
                config,
                client,
            )
            .await
        }
        "add" => {
            cmd_add(
                &mode,
                args,
                &parts,
                &tokens.user_id,
                &access_token,
                config,
                client,
            )
            .await
        }
        "remove" | "rm" | "delete" => {
            cmd_remove(
                &mode,
                args,
                &parts,
                &tokens.user_id,
                &access_token,
                config,
                client,
            )
            .await
        }
        "help" | "--help" | "-h" => {
            print_help(&mode);
            Ok(())
        }
        _ => {
            eprintln!("Unknown {} subcommand: {}", mode.noun(), sub);
            print_help(&mode);
            Ok(())
        }
    }
}

async fn cmd_list(
    mode: &Mode,
    args: &ModerationArgs,
    user_id: &str,
    username: &str,
    access_token: &str,
    config: &Config,
    client: &XClient,
) -> Result<()> {
    let users = fetch_users(
        client,
        user_id,
        mode.path(),
        access_token,
        args.limit.max(1),
    )
    .await?;
    costs::track_cost(
        &config.costs_path(),
        mode.list_op(),
        &format!("/2/users/{}/{}", user_id, mode.path()),
        users.len() as u64,
    );

    if args.json {
        println!("{}", serde_json::to_string_pretty(&users)?);
        return Ok(());
    }

    if users.is_empty() {
        println!("No {} found.", mode.noun());
        return Ok(());
    }

    println!(
        "\n{} — @{} ({})\n",
        title_case(mode.noun()),
        username,
        users.len()
    );
    for (i, user) in users.iter().enumerate() {
        let handle = user.get("username").and_then(|v| v.as_str()).unwrap_or("?");
        let name = user.get("name").and_then(|v| v.as_str()).unwrap_or("");
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

async fn cmd_add(
    mode: &Mode,
    args: &ModerationArgs,
    parts: &[String],
    source_user_id: &str,
    access_token: &str,
    config: &Config,
    client: &XClient,
) -> Result<()> {
    let target = parts
        .get(1)
        .map(String::as_str)
        .ok_or_else(|| anyhow::anyhow!("Usage: xint {} add <@username|user_id>", mode.noun()))?;

    let (target_user_id, target_username) = resolve_user_id(client, access_token, target).await?;
    let body = serde_json::json!({ "target_user_id": target_user_id });
    let result = client
        .oauth_post(
            &format!("users/{}/{}", source_user_id, mode.path()),
            access_token,
            Some(&body),
        )
        .await?;

    costs::track_cost(
        &config.costs_path(),
        mode.add_op(),
        &format!("/2/users/{}/{}", source_user_id, mode.path()),
        0,
    );

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        let success = is_added(&result, mode) || result.get("success").is_some();
        if success {
            println!("{} @{}", past_tense(mode.action_add()), target_username);
        } else {
            println!("Failed to {} @{}", mode.action_add(), target_username);
        }
    }

    Ok(())
}

async fn cmd_remove(
    mode: &Mode,
    args: &ModerationArgs,
    parts: &[String],
    source_user_id: &str,
    access_token: &str,
    config: &Config,
    client: &XClient,
) -> Result<()> {
    let target = parts
        .get(1)
        .map(String::as_str)
        .ok_or_else(|| anyhow::anyhow!("Usage: xint {} remove <@username|user_id>", mode.noun()))?;

    let (target_user_id, target_username) = resolve_user_id(client, access_token, target).await?;
    let result = client
        .oauth_delete(
            &format!(
                "users/{}/{}/{}",
                source_user_id,
                mode.path(),
                target_user_id
            ),
            access_token,
        )
        .await?;

    costs::track_cost(
        &config.costs_path(),
        mode.remove_op(),
        &format!(
            "/2/users/{}/{}/{}",
            source_user_id,
            mode.path(),
            target_user_id
        ),
        0,
    );

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        let success = is_removed(&result, mode) || result.get("success").is_some();
        if success {
            println!("{} @{}", past_tense(mode.action_remove()), target_username);
        } else {
            println!("Failed to {} @{}", mode.action_remove(), target_username);
        }
    }

    Ok(())
}

async fn fetch_users(
    client: &XClient,
    user_id: &str,
    path_segment: &str,
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
        let path = format!(
            "users/{user_id}/{path_segment}?max_results={per_page}&{USER_FIELDS}{pagination}"
        );

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
    let value = input.trim_start_matches('@');
    if is_likely_user_id(value) {
        return Ok((value.to_string(), value.to_string()));
    }

    let path = format!("users/by/username/{value}?{USER_FIELDS}");
    let raw = client.oauth_get(&path, access_token).await?;
    let user = raw
        .data
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("User @{value} not found"))?;
    let id = user
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("User @{value} not found"))?;
    let username = user
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or(value);
    Ok((id.to_string(), username.to_string()))
}

fn is_added(result: &serde_json::Value, mode: &Mode) -> bool {
    match mode {
        Mode::Blocks => result.pointer("/data/blocking") == Some(&serde_json::Value::Bool(true)),
        Mode::Mutes => result.pointer("/data/muting") == Some(&serde_json::Value::Bool(true)),
    }
}

fn is_removed(result: &serde_json::Value, mode: &Mode) -> bool {
    match mode {
        Mode::Blocks => result.pointer("/data/blocking") == Some(&serde_json::Value::Bool(false)),
        Mode::Mutes => result.pointer("/data/muting") == Some(&serde_json::Value::Bool(false)),
    }
}

fn is_likely_user_id(input: &str) -> bool {
    !input.is_empty() && input.chars().all(|c| c.is_ascii_digit())
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

fn title_case(s: &str) -> String {
    capitalize(s)
}

fn past_tense(action: &str) -> &'static str {
    match action {
        "block" => "Blocked",
        "mute" => "Muted",
        "unblock" => "Unblocked",
        "unmute" => "Unmuted",
        _ => "Updated",
    }
}

fn print_help(mode: &Mode) {
    println!("Usage: xint {} <subcommand> [options]", mode.noun());
    println!();
    println!("Subcommands:");
    println!(
        "  list [--limit N] [--json]                 List your {} (default)",
        mode.noun()
    );
    println!(
        "  add <@username|user_id> [--json]          {} a user",
        capitalize(mode.action_add())
    );
    println!(
        "  remove <@username|user_id> [--json]       {} a user",
        capitalize(mode.action_remove())
    );
}

#[cfg(test)]
mod tests {
    use super::{capitalize, is_likely_user_id, past_tense};

    #[test]
    fn test_is_likely_user_id() {
        assert!(is_likely_user_id("2244994945"));
        assert!(!is_likely_user_id("jack"));
        assert!(!is_likely_user_id("@jack"));
    }

    #[test]
    fn test_capitalize() {
        assert_eq!(capitalize("block"), "Block");
        assert_eq!(capitalize(""), "");
    }

    #[test]
    fn test_past_tense() {
        assert_eq!(past_tense("mute"), "Muted");
        assert_eq!(past_tense("unmute"), "Unmuted");
    }
}
