use anyhow::Result;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::auth::oauth;
use crate::cli::DiffArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::models::*;

pub async fn run(args: &DiffArgs, config: &Config, client: &XClient) -> Result<()> {
    let username = match &args.username {
        Some(u) => u.trim_start_matches('@').to_string(),
        None => {
            println!("Usage: xint diff <@username> [options]");
            return Ok(());
        }
    };

    let snap_type = if args.following { "following" } else { "followers" };
    let snapshots_dir = config.snapshots_dir();
    fs::create_dir_all(&snapshots_dir)?;

    // Show history
    if args.history {
        let files = list_snapshots(&snapshots_dir, &username, snap_type);
        if files.is_empty() {
            println!(
                "No {} snapshots for @{}. Run 'xint diff @{}' to create one.",
                snap_type, username, username
            );
            return Ok(());
        }
        println!("\nSnapshots for @{} ({}):\n", username, snap_type);
        for f in &files {
            let path = snapshots_dir.join(f);
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(snap) = serde_json::from_str::<Snapshot>(&content) {
                    println!("  {} — {} {}", &snap.timestamp[..10], snap.count, snap_type);
                } else {
                    println!("  {} (corrupted)", f);
                }
            }
        }
        return Ok(());
    }

    // Get OAuth token
    let client_id = config.require_client_id()?;
    let (access_token, _tokens) =
        oauth::get_valid_token(client, &config.tokens_path(), client_id).await?;

    eprintln!("Fetching {} for @{}...", snap_type, username);

    // Look up user ID
    let lookup_path = format!(
        "users/by/username/{}?user.fields=public_metrics",
        username
    );
    let raw = client.oauth_get(&lookup_path, &access_token).await?;
    let user_id = raw
        .data
        .as_ref()
        .and_then(|d| d.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("User @{} not found", username))?
        .to_string();

    costs::track_cost(
        &config.costs_path(),
        "profile",
        &format!("/2/users/by/username/{}", username),
        1,
    );

    // Fetch current list
    let users = fetch_user_list(client, &access_token, &user_id, snap_type, args.pages).await?;

    costs::track_cost(
        &config.costs_path(),
        snap_type,
        &format!("/2/users/{}/{}", user_id, snap_type),
        users.len() as u64,
    );

    eprintln!("Found {} {}", users.len(), snap_type);

    // Create current snapshot
    let current = Snapshot {
        username: username.clone(),
        snap_type: snap_type.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        count: users.len(),
        users: users.clone(),
    };

    // Load previous snapshot
    let previous = load_latest_snapshot(&snapshots_dir, &username, snap_type);

    // Save current
    let date = &current.timestamp[..10];
    let snap_path = snapshots_dir.join(format!(
        "{}-{}-{}.json",
        username.to_lowercase(),
        snap_type,
        date
    ));
    let json = serde_json::to_string_pretty(&current)?;
    fs::write(&snap_path, &json)?;
    eprintln!("Snapshot saved to {}", snap_path.display());

    // Compute and display diff
    if let Some(prev) = previous {
        let diff = compute_diff(&prev, &current);

        if args.json {
            println!("{}", serde_json::to_string_pretty(&diff)?);
        } else {
            println!("{}", format_diff(&diff, snap_type));
        }
    } else {
        println!(
            "\nFirst snapshot for @{} ({}): {} users",
            username,
            snap_type,
            users.len()
        );
        println!("Run again later to see changes.");

        if args.json {
            println!("{}", serde_json::to_string_pretty(&current)?);
        } else {
            let mut sorted = users;
            sorted.sort_by(|a, b| {
                b.followers_count
                    .unwrap_or(0)
                    .cmp(&a.followers_count.unwrap_or(0))
            });
            let top: Vec<_> = sorted.into_iter().take(20).collect();
            println!("\nTop {} by follower count:\n", snap_type);
            for u in &top {
                let fc = u
                    .followers_count
                    .map(|n| format!("{} followers", n))
                    .unwrap_or_default();
                println!("  @{} — {} ({})", u.username, u.name, fc);
            }
        }
    }

    Ok(())
}

async fn fetch_user_list(
    client: &XClient,
    access_token: &str,
    user_id: &str,
    snap_type: &str,
    max_pages: u32,
) -> Result<Vec<UserSnapshot>> {
    let mut users = Vec::new();
    let mut next_token: Option<String> = None;
    let fields = "user.fields=public_metrics,username,name";

    for page in 0..max_pages {
        let pagination = match &next_token {
            Some(t) => format!("&pagination_token={}", t),
            None => String::new(),
        };
        let path = format!(
            "users/{}/{}?max_results=1000&{}{}",
            user_id, snap_type, fields, pagination
        );

        let raw = client.oauth_get(&path, access_token).await?;

        if let Some(data) = &raw.data {
            if let Some(arr) = data.as_array() {
                for u in arr {
                    users.push(UserSnapshot {
                        id: u.get("id").and_then(|v| v.as_str()).unwrap_or("?").to_string(),
                        username: u
                            .get("username")
                            .and_then(|v| v.as_str())
                            .unwrap_or("?")
                            .to_string(),
                        name: u
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("?")
                            .to_string(),
                        followers_count: u
                            .pointer("/public_metrics/followers_count")
                            .and_then(|v| v.as_u64()),
                        following_count: u
                            .pointer("/public_metrics/following_count")
                            .and_then(|v| v.as_u64()),
                    });
                }
            }
        }

        next_token = raw.meta.and_then(|m| m.next_token);
        if next_token.is_none() {
            break;
        }
        if page < max_pages - 1 {
            crate::client::rate_delay().await;
        }
    }

    Ok(users)
}

fn list_snapshots(dir: &Path, username: &str, snap_type: &str) -> Vec<String> {
    let prefix = format!("{}-{}-", username.to_lowercase(), snap_type);
    let mut files: Vec<String> = fs::read_dir(dir)
        .ok()
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.starts_with(&prefix) && name.ends_with(".json") {
                        Some(name)
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files.reverse();
    files
}

fn load_latest_snapshot(dir: &Path, username: &str, snap_type: &str) -> Option<Snapshot> {
    let files = list_snapshots(dir, username, snap_type);
    let first = files.first()?;
    let content = fs::read_to_string(dir.join(first)).ok()?;
    serde_json::from_str(&content).ok()
}

fn compute_diff(previous: &Snapshot, current: &Snapshot) -> FollowerDiff {
    let prev_ids: HashSet<&str> = previous.users.iter().map(|u| u.id.as_str()).collect();
    let curr_ids: HashSet<&str> = current.users.iter().map(|u| u.id.as_str()).collect();

    let added: Vec<UserSnapshot> = current
        .users
        .iter()
        .filter(|u| !prev_ids.contains(u.id.as_str()))
        .cloned()
        .collect();
    let removed: Vec<UserSnapshot> = previous
        .users
        .iter()
        .filter(|u| !curr_ids.contains(u.id.as_str()))
        .cloned()
        .collect();
    let unchanged = current
        .users
        .iter()
        .filter(|u| prev_ids.contains(u.id.as_str()))
        .count();

    FollowerDiff {
        added,
        removed,
        unchanged,
        previous: DiffSnapshot {
            timestamp: previous.timestamp.clone(),
            count: previous.count,
        },
        current: DiffSnapshot {
            timestamp: current.timestamp.clone(),
            count: current.count,
        },
    }
}

fn format_diff(diff: &FollowerDiff, snap_type: &str) -> String {
    let net_change = diff.added.len() as i64 - diff.removed.len() as i64;
    let sign = if net_change >= 0 { "+" } else { "" };
    let prev_date = &diff.previous.timestamp[..10];
    let curr_date = &diff.current.timestamp[..10];
    let label = if snap_type == "followers" {
        "Follower"
    } else {
        "Following"
    };

    let mut out = format!(
        "\n{} Diff: {} -> {}\n",
        label, prev_date, curr_date
    );
    out.push_str(&format!(
        "{} -> {} ({}{} net)\n",
        diff.previous.count, diff.current.count, sign, net_change
    ));

    if !diff.added.is_empty() {
        out.push_str(&format!("\n+ New ({}):\n", diff.added.len()));
        for u in diff.added.iter().take(50) {
            let fc = u
                .followers_count
                .map(|n| format!(" ({} followers)", n))
                .unwrap_or_default();
            out.push_str(&format!("  + @{}{}\n", u.username, fc));
        }
        if diff.added.len() > 50 {
            out.push_str(&format!("  ... +{} more\n", diff.added.len() - 50));
        }
    }

    if !diff.removed.is_empty() {
        out.push_str(&format!("\n- Lost ({}):\n", diff.removed.len()));
        for u in diff.removed.iter().take(50) {
            out.push_str(&format!("  - @{}\n", u.username));
        }
        if diff.removed.len() > 50 {
            out.push_str(&format!("  ... +{} more\n", diff.removed.len() - 50));
        }
    }

    if diff.added.is_empty() && diff.removed.is_empty() {
        out.push_str("\nNo changes detected.\n");
    }

    out
}
