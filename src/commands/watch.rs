use anyhow::{bail, Result};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::api::twitter;
use crate::cli::WatchArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::format;

fn parse_duration(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let last = s.chars().last()?;
    let num: u64 = s[..s.len() - 1].parse().ok()?;
    match last {
        's' => Some(num * 1000),
        'm' => Some(num * 60_000),
        'h' => Some(num * 3_600_000),
        _ => None,
    }
}

fn now_display() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

pub async fn run(args: &WatchArgs, config: &Config, client: &XClient) -> Result<()> {
    let token = config.require_bearer_token()?;
    let mut query = args.query.join(" ");

    if query.is_empty() {
        println!("Usage: xint watch <query> [options]");
        return Ok(());
    }

    // Auto-expand @username to from:username
    if query.starts_with('@') && !query.contains(' ') {
        query = format!("from:{}", &query[1..]);
    }

    // Auto-add noise filters
    if !query.contains("is:retweet") {
        query.push_str(" -is:retweet");
    }

    let interval_ms = parse_duration(&args.interval)
        .ok_or_else(|| anyhow::anyhow!("Invalid interval: {}. Use format: 30s, 5m, 1h", args.interval))?;

    if interval_ms < 10_000 {
        bail!("Minimum interval is 10s");
    }

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })?;

    let interval_str = if interval_ms >= 3_600_000 {
        format!("{}h", interval_ms / 3_600_000)
    } else if interval_ms >= 60_000 {
        format!("{}m", interval_ms / 60_000)
    } else {
        format!("{}s", interval_ms / 1000)
    };

    eprintln!("\nWatching: \"{}\" every {}", query, interval_str);
    if let Some(ref wh) = args.webhook {
        eprintln!("Webhook: {}", wh);
    }
    eprintln!("Press Ctrl+C to stop\n");

    let mut seen_ids = HashSet::new();
    let mut poll_count = 0u64;
    let mut total_new = 0u64;
    let mut total_cost = 0.0f64;
    let start_time = std::time::Instant::now();

    while running.load(Ordering::SeqCst) {
        match twitter::search(
            client,
            token,
            &query,
            1,
            "recency",
            Some(&args.since),
            None,
            false,
        )
        .await
        {
            Ok(tweets) => {
                let cost = tweets.len() as f64 * 0.005;
                total_cost += cost;
                costs::track_cost(
                    &config.costs_path(),
                    "search",
                    "/2/tweets/search/recent",
                    tweets.len() as u64,
                );
                poll_count += 1;

                let new_tweets: Vec<_> =
                    tweets.iter().filter(|t| !seen_ids.contains(&t.id)).cloned().collect();

                for t in &tweets {
                    seen_ids.insert(t.id.clone());
                }

                if !new_tweets.is_empty() {
                    total_new += new_tweets.len() as u64;
                    let limited: Vec<_> = new_tweets.into_iter().take(args.limit).collect();

                    if !args.quiet {
                        eprintln!("\n[{}] +{} new", now_display(), limited.len());
                    }

                    if args.jsonl {
                        for t in &limited {
                            if let Ok(json) = serde_json::to_string(t) {
                                println!("{}", json);
                            }
                        }
                    } else {
                        for t in &limited {
                            println!("{}", format::format_tweet_terminal(t, None, false));
                            println!();
                        }
                    }

                    // Webhook
                    if let Some(ref webhook_url) = args.webhook {
                        let payload = serde_json::json!({
                            "source": "xint",
                            "query": query,
                            "timestamp": chrono::Utc::now().to_rfc3339(),
                            "count": limited.len(),
                            "tweets": limited.iter().map(|t| serde_json::json!({
                                "id": t.id,
                                "username": t.username,
                                "text": t.text,
                                "likes": t.metrics.likes,
                                "retweets": t.metrics.retweets,
                                "url": t.tweet_url,
                                "created_at": t.created_at,
                            })).collect::<Vec<_>>(),
                        });
                        if let Err(e) = client.post_json(webhook_url, &payload).await {
                            eprintln!("[webhook] Failed: {}", e);
                        }
                    }
                } else if poll_count == 1 {
                    eprintln!(
                        "[{}] Seeded with {} existing tweets (watching for new)",
                        now_display(),
                        tweets.len()
                    );
                }

                // Budget check
                let budget = costs::check_budget(&config.costs_path());
                if !budget.allowed {
                    eprintln!(
                        "\n!! Budget exceeded (${:.2}/${:.2}). Stopping watch.",
                        budget.spent, budget.limit
                    );
                    break;
                }
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("Rate limited") {
                    let wait_sec = msg
                        .split("Resets in ")
                        .nth(1)
                        .and_then(|s| s.trim_end_matches('s').parse::<u64>().ok())
                        .unwrap_or(60);
                    eprintln!("[{}] Rate limited, waiting {}s...", now_display(), wait_sec);
                    tokio::time::sleep(std::time::Duration::from_secs(wait_sec)).await;
                    continue;
                }
                eprintln!("[{}] Error: {}", now_display(), msg);
            }
        }

        // Wait for next interval
        tokio::time::sleep(std::time::Duration::from_millis(interval_ms)).await;
    }

    // Stats
    let elapsed = start_time.elapsed().as_secs();
    let mins = elapsed / 60;
    let secs = elapsed % 60;
    eprintln!("\n--- Watch stopped ---");
    eprintln!(
        "Duration: {}m {}s | Polls: {} | New tweets: {} | Est. cost: ~${:.3}",
        mins, secs, poll_count, total_new, total_cost
    );

    Ok(())
}
