use anyhow::Result;
use std::fs;

use crate::api::twitter;
use crate::auth::oauth;
use crate::cli::BookmarksArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::format;

pub async fn run(args: &BookmarksArgs, config: &Config, client: &XClient) -> Result<()> {
    let client_id = config.require_client_id()?;
    let (access_token, tokens) =
        oauth::get_valid_token(client, &config.tokens_path(), client_id).await?;

    let cache_key = format!("bookmarks:{}", tokens.user_id);
    let cache_params = format!("limit={}", args.limit);
    let cache_ttl = 5 * 60 * 1000u64; // 5 minutes

    let cached: Option<Vec<crate::models::Tweet>> = if args.no_cache {
        None
    } else {
        crate::cache::get(&config.cache_dir(), &cache_key, &cache_params, cache_ttl)
    };

    let all_tweets = if let Some(cached) = cached {
        eprintln!("(cached — {} bookmarks)", cached.len());
        cached
    } else {
        let fetch_count = if args.since.is_some() || args.query.is_some() {
            (args.limit * 3).max(100).min(800)
        } else {
            args.limit.min(800)
        };

        eprintln!("Fetching bookmarks for @{}...", tokens.username);

        let mut all = Vec::new();
        let mut next_token: Option<String> = None;
        let per_page = fetch_count.min(100);
        let max_pages = ((fetch_count + per_page - 1) / per_page).min(8);

        for page in 0..max_pages {
            let pagination = match &next_token {
                Some(t) => format!("&pagination_token={}", t),
                None => String::new(),
            };
            let path = format!(
                "users/{}/bookmarks?max_results={}&{}{}",
                tokens.user_id,
                per_page,
                crate::client::FIELDS,
                pagination
            );

            let raw = client.oauth_get(&path, &access_token).await?;
            let tweets = twitter::parse_tweets(&raw);
            all.extend(tweets);

            if all.len() >= fetch_count {
                break;
            }
            next_token = raw.meta.and_then(|m| m.next_token);
            if next_token.is_none() {
                break;
            }
            if page < max_pages - 1 {
                crate::client::rate_delay().await;
            }
        }

        costs::track_cost(
            &config.costs_path(),
            "bookmarks",
            "/2/users/bookmarks",
            all.len() as u64,
        );

        crate::cache::set(&config.cache_dir(), &cache_key, &cache_params, &all);
        all
    };

    // Client-side filtering
    let mut filtered = all_tweets;

    if let Some(ref since) = args.since {
        if let Some(ts) = twitter::parse_since(since) {
            if let Ok(since_dt) = chrono::DateTime::parse_from_rfc3339(&ts) {
                let since_ms = since_dt.timestamp_millis();
                filtered.retain(|t| {
                    chrono::DateTime::parse_from_rfc3339(&t.created_at)
                        .map(|d| d.timestamp_millis() >= since_ms)
                        .unwrap_or(true)
                });
            }
        }
    }

    if let Some(ref q) = args.query {
        let q_lower = q.to_lowercase();
        filtered.retain(|t| {
            t.text.to_lowercase().contains(&q_lower)
                || t.username.to_lowercase().contains(&q_lower)
                || t.hashtags.iter().any(|h| h.to_lowercase().contains(&q_lower))
        });
    }

    filtered = twitter::dedupe(filtered);
    let shown: Vec<_> = filtered.into_iter().take(args.limit).collect();

    if shown.is_empty() {
        println!("No bookmarks match the filters.");
        return Ok(());
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&shown)?);
    } else if args.markdown {
        let md = format::format_research_markdown("Bookmarks", &shown, &["bookmarks"]);
        println!("{}", md);
    } else {
        println!("{}", format::format_results_terminal(&shown, Some("Bookmarks"), args.limit));
    }

    if args.save {
        let exports_dir = config.exports_dir();
        fs::create_dir_all(&exports_dir)?;
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let path = exports_dir.join(format!("x-bookmarks-{}.md", date));
        let md = format::format_research_markdown("Bookmarks", &shown, &["bookmarks"]);
        fs::write(&path, &md)?;
        eprintln!("\nSaved to {}", path.display());
    }

    Ok(())
}
