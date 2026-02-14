use anyhow::Result;

use crate::api::twitter;
use crate::auth::oauth;
use crate::cli::*;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::format;

// ---------------------------------------------------------------------------
// Likes (list)
// ---------------------------------------------------------------------------

pub async fn run_likes(args: &LikesArgs, config: &Config, client: &XClient) -> Result<()> {
    let client_id = config.require_client_id()?;
    let (access_token, tokens) =
        oauth::get_valid_token(client, &config.tokens_path(), client_id).await?;

    let cache_key = format!("likes:{}", tokens.user_id);
    let cache_ttl = 5 * 60 * 1000u64;

    let cached: Option<Vec<crate::models::Tweet>> = if args.no_cache {
        None
    } else {
        crate::cache::get(&config.cache_dir(), &cache_key, "", cache_ttl)
    };

    let mut tweets = if let Some(cached) = cached {
        cached
    } else {
        let fetch_count = args.limit.max(100);
        let mut all = Vec::new();
        let mut next_token: Option<String> = None;

        while all.len() < fetch_count {
            let per_page = (fetch_count - all.len()).min(100);
            let pagination = match &next_token {
                Some(t) => format!("&pagination_token={}", t),
                None => String::new(),
            };
            let path = format!(
                "users/{}/liked_tweets?max_results={}&{}{}",
                tokens.user_id,
                per_page,
                crate::client::FIELDS,
                pagination
            );

            let raw = client.oauth_get(&path, &access_token).await?;
            let batch = twitter::parse_tweets(&raw);
            if batch.is_empty() {
                break;
            }
            all.extend(batch);
            next_token = raw.meta.and_then(|m| m.next_token);
            if next_token.is_none() {
                break;
            }
        }

        costs::track_cost(
            &config.costs_path(),
            "likes",
            "/2/users/liked_tweets",
            all.len() as u64,
        );
        crate::cache::set(&config.cache_dir(), &cache_key, "", &all);
        all
    };

    // Filters
    if let Some(ref since) = args.since {
        if let Some(ts) = twitter::parse_since(since) {
            if let Ok(since_dt) = chrono::DateTime::parse_from_rfc3339(&ts) {
                let since_ms = since_dt.timestamp_millis();
                tweets.retain(|t| {
                    chrono::DateTime::parse_from_rfc3339(&t.created_at)
                        .map(|d| d.timestamp_millis() >= since_ms)
                        .unwrap_or(true)
                });
            }
        }
    }

    if let Some(ref q) = args.query {
        let q_lower = q.to_lowercase();
        tweets.retain(|t| {
            t.text.to_lowercase().contains(&q_lower)
                || t.username.to_lowercase().contains(&q_lower)
        });
    }

    tweets.truncate(args.limit);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&tweets)?);
    } else if args.markdown {
        println!(
            "{}",
            format::format_results_terminal(&tweets, Some("Liked Tweets"), args.limit)
        );
    } else {
        println!("\nLiked Tweets — @{}\n", tokens.username);
        if tweets.is_empty() {
            println!("No liked tweets found.");
        } else {
            for (i, t) in tweets.iter().enumerate() {
                if i > 0 {
                    println!();
                }
                println!("{}", format::format_tweet_terminal(t, Some(i), false));
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Like
// ---------------------------------------------------------------------------

pub async fn run_like(args: &LikeArgs, config: &Config, client: &XClient) -> Result<()> {
    let client_id = config.require_client_id()?;
    let (access_token, tokens) =
        oauth::get_valid_token(client, &config.tokens_path(), client_id).await?;

    let body = serde_json::json!({ "tweet_id": args.tweet_id });
    let path = format!("users/{}/likes", tokens.user_id);
    let result = client.oauth_post(&path, &access_token, Some(&body)).await?;

    costs::track_cost(&config.costs_path(), "like", "/2/users/likes", 0);

    if result.pointer("/data/liked") == Some(&serde_json::Value::Bool(true)) {
        println!("Liked tweet {}", args.tweet_id);
        println!("   https://x.com/i/status/{}", args.tweet_id);
    } else {
        eprintln!("Failed to like tweet {}", args.tweet_id);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Unlike
// ---------------------------------------------------------------------------

pub async fn run_unlike(args: &UnlikeArgs, config: &Config, client: &XClient) -> Result<()> {
    let client_id = config.require_client_id()?;
    let (access_token, tokens) =
        oauth::get_valid_token(client, &config.tokens_path(), client_id).await?;

    let path = format!("users/{}/likes/{}", tokens.user_id, args.tweet_id);
    let result = client.oauth_delete(&path, &access_token).await?;

    costs::track_cost(&config.costs_path(), "unlike", "/2/users/likes", 0);

    let success = result.pointer("/data/liked") == Some(&serde_json::Value::Bool(false))
        || result.get("success").is_some();

    if success {
        println!("Unliked tweet {}", args.tweet_id);
    } else {
        eprintln!("Failed to unlike tweet {}", args.tweet_id);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Bookmark save
// ---------------------------------------------------------------------------

pub async fn run_bookmark(args: &BookmarkArgs, config: &Config, client: &XClient) -> Result<()> {
    let client_id = config.require_client_id()?;
    let (access_token, tokens) =
        oauth::get_valid_token(client, &config.tokens_path(), client_id).await?;

    let body = serde_json::json!({ "tweet_id": args.tweet_id });
    let path = format!("users/{}/bookmarks", tokens.user_id);
    let result = client.oauth_post(&path, &access_token, Some(&body)).await?;

    costs::track_cost(&config.costs_path(), "bookmark_save", "/2/users/bookmarks", 0);

    if result.pointer("/data/bookmarked") == Some(&serde_json::Value::Bool(true)) {
        println!("Bookmarked tweet {}", args.tweet_id);
        println!("   https://x.com/i/status/{}", args.tweet_id);
    } else {
        eprintln!("Failed to bookmark tweet {}", args.tweet_id);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Unbookmark
// ---------------------------------------------------------------------------

pub async fn run_unbookmark(
    args: &UnbookmarkArgs,
    config: &Config,
    client: &XClient,
) -> Result<()> {
    let client_id = config.require_client_id()?;
    let (access_token, tokens) =
        oauth::get_valid_token(client, &config.tokens_path(), client_id).await?;

    let path = format!("users/{}/bookmarks/{}", tokens.user_id, args.tweet_id);
    let result = client.oauth_delete(&path, &access_token).await?;

    costs::track_cost(
        &config.costs_path(),
        "bookmark_remove",
        "/2/users/bookmarks",
        0,
    );

    let success = result.pointer("/data/bookmarked") == Some(&serde_json::Value::Bool(false))
        || result.get("success").is_some();

    if success {
        println!("Removed bookmark for tweet {}", args.tweet_id);
    } else {
        eprintln!("Failed to remove bookmark for tweet {}", args.tweet_id);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Following (list)
// ---------------------------------------------------------------------------

pub async fn run_following(args: &FollowingArgs, config: &Config, client: &XClient) -> Result<()> {
    let client_id = config.require_client_id()?;
    let (access_token, tokens) =
        oauth::get_valid_token(client, &config.tokens_path(), client_id).await?;

    let (user_id, display_username) = if let Some(ref username) = args.username {
        let username = username.trim_start_matches('@');
        if username.eq_ignore_ascii_case(&tokens.username) {
            (tokens.user_id.clone(), tokens.username.clone())
        } else {
            let path = format!(
                "users/by/username/{}?user.fields=public_metrics",
                username
            );
            let raw = client.oauth_get(&path, &access_token).await?;
            let id = raw
                .data
                .as_ref()
                .and_then(|d| d.get("id"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("User @{} not found", username))?
                .to_string();
            (id, username.to_string())
        }
    } else {
        (tokens.user_id.clone(), tokens.username.clone())
    };

    let mut all_users: Vec<serde_json::Value> = Vec::new();
    let mut next_token: Option<String> = None;
    let fields = "user.fields=username,name,public_metrics,description,created_at";

    while all_users.len() < args.limit {
        let per_page = (args.limit - all_users.len()).min(1000);
        let pagination = match &next_token {
            Some(t) => format!("&pagination_token={}", t),
            None => String::new(),
        };
        let path = format!(
            "users/{}/following?max_results={}&{}{}",
            user_id, per_page, fields, pagination
        );

        let raw = client.oauth_get(&path, &access_token).await?;

        if let Some(data) = &raw.data {
            if let Some(arr) = data.as_array() {
                all_users.extend(arr.iter().cloned());
            }
        }

        next_token = raw.meta.and_then(|m| m.next_token);
        if next_token.is_none() {
            break;
        }
    }

    costs::track_cost(
        &config.costs_path(),
        "following",
        "/2/users/following",
        0,
    );

    all_users.truncate(args.limit);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&all_users)?);
        return Ok(());
    }

    println!(
        "\nFollowing — @{} ({} accounts)\n",
        display_username,
        all_users.len()
    );

    for (i, u) in all_users.iter().enumerate() {
        let username = u.get("username").and_then(|v| v.as_str()).unwrap_or("?");
        let name = u.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let followers = u
            .pointer("/public_metrics/followers_count")
            .and_then(|v| v.as_u64());
        let followers_str = match followers {
            Some(n) => format!(" ({} followers)", n),
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
