use anyhow::{bail, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::api::twitter;
use crate::cli::{StreamArgs, StreamRulesArgs};
use crate::client::{XClient, FIELDS};
use crate::config::Config;
use crate::costs;
use crate::format;
use crate::models::RawResponse;
use crate::output_meta;
use crate::webhook::validate_webhook_url;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
struct StreamRule {
    id: String,
    value: String,
    tag: Option<String>,
}

fn now_display() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn validate_backfill(minutes: Option<u8>) -> Result<Option<u8>> {
    match minutes {
        Some(v) if (1..=5).contains(&v) => Ok(Some(v)),
        Some(_) => bail!("--backfill must be between 1 and 5."),
        None => Ok(None),
    }
}

async fn fetch_rules(client: &XClient, token: &str) -> Result<Vec<StreamRule>> {
    let raw = client
        .bearer_get("tweets/search/stream/rules", token)
        .await?;
    let data = match raw.data {
        Some(serde_json::Value::Array(arr)) => serde_json::Value::Array(arr),
        Some(_) | None => serde_json::Value::Array(Vec::new()),
    };
    Ok(serde_json::from_value(data).unwrap_or_default())
}

pub async fn run_stream(args: &StreamArgs, config: &Config, client: &XClient) -> Result<()> {
    if args.json && args.jsonl {
        bail!("Use only one of --json or --jsonl.");
    }

    let token = config.require_bearer_token()?;
    let backfill = validate_backfill(args.backfill)?;
    let webhook_url = match args.webhook.as_deref() {
        Some(raw) => Some(validate_webhook_url(raw)?),
        None => None,
    };

    let mut path = format!("tweets/search/stream?{FIELDS}");
    if let Some(minutes) = backfill {
        path.push_str(&format!("&backfill_minutes={minutes}"));
    }

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })?;

    if !args.quiet {
        eprintln!("Connecting to X filtered stream...");
        eprintln!("Press Ctrl+C to stop");
    }

    let mut response = client.bearer_stream(&path, token).await?;
    let mut buffer = String::new();
    let start_time = std::time::Instant::now();
    let mut events_seen: usize = 0;

    while running.load(Ordering::SeqCst) {
        let chunk = response.chunk().await?;
        let Some(bytes) = chunk else { break };
        if bytes.is_empty() {
            continue;
        }

        buffer.push_str(&String::from_utf8_lossy(&bytes));
        while let Some(newline_idx) = buffer.find('\n') {
            let line = buffer[..newline_idx].trim().to_string();
            buffer = buffer[newline_idx + 1..].to_string();
            if line.is_empty() {
                continue;
            }

            let value: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if value.get("data").is_none() {
                continue;
            }

            let raw: RawResponse = match serde_json::from_value(value.clone()) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let tweets = twitter::parse_tweets(&raw);
            if tweets.is_empty() {
                continue;
            }

            costs::track_cost(
                &config.costs_path(),
                "stream_connect",
                "/2/tweets/search/stream",
                tweets.len() as u64,
            );

            for tweet in tweets {
                events_seen += 1;
                let matching_rules = value
                    .get("matching_rules")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!([]));
                let tweet_json = serde_json::to_value(&tweet)?;
                let event = serde_json::json!({
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "matching_rules": matching_rules,
                    "tweet": tweet_json,
                });

                let meta = output_meta::build_meta(
                    "x_api_v2",
                    start_time,
                    false,
                    1.0,
                    "/2/tweets/search/stream",
                    0.005,
                    &config.costs_path(),
                );

                if args.json {
                    output_meta::print_json_with_meta(&meta, &event)?;
                } else if args.jsonl {
                    let payload = serde_json::json!({
                        "source": meta.source,
                        "latency_ms": meta.latency_ms,
                        "cached": meta.cached,
                        "confidence": meta.confidence,
                        "api_endpoint": meta.api_endpoint,
                        "timestamp": meta.timestamp,
                        "estimated_cost_usd": meta.estimated_cost_usd,
                        "budget_remaining_usd": meta.budget_remaining_usd,
                        "event": event
                    });
                    println!("{}", serde_json::to_string(&payload)?);
                } else {
                    if !args.quiet {
                        let rule_count = event["matching_rules"]
                            .as_array()
                            .map(|a| a.len())
                            .unwrap_or(0);
                        eprintln!("[{}] Stream match ({} rule(s))", now_display(), rule_count);
                    }
                    println!("{}", format::format_tweet_terminal(&tweet, None, false));
                    println!();
                }

                if let Some(webhook) = &webhook_url {
                    if let Err(err) = client
                        .post_json(
                            webhook,
                            &serde_json::json!({
                                "source": "xint-stream",
                                "event": event,
                            }),
                        )
                        .await
                    {
                        eprintln!("[webhook] Failed: {err}");
                    }
                }

                let budget = costs::check_budget(&config.costs_path());
                if !budget.allowed {
                    eprintln!(
                        "\n!! Budget exceeded (${:.2}/${:.2}). Stopping stream.",
                        budget.spent, budget.limit
                    );
                    running.store(false, Ordering::SeqCst);
                    break;
                }

                if let Some(max_events) = args.max_events {
                    if events_seen >= max_events {
                        running.store(false, Ordering::SeqCst);
                        break;
                    }
                }
            }
        }
    }

    let elapsed = start_time.elapsed().as_secs();
    if !args.quiet {
        eprintln!("\n--- Stream stopped ---");
        eprintln!(
            "Duration: {}m {}s | Events: {}",
            elapsed / 60,
            elapsed % 60,
            events_seen
        );
    }

    Ok(())
}

pub async fn run_stream_rules(
    args: &StreamRulesArgs,
    config: &Config,
    client: &XClient,
) -> Result<()> {
    let token = config.require_bearer_token()?;
    let parts = args
        .subcommand
        .clone()
        .unwrap_or_else(|| vec!["list".to_string()]);
    let sub = parts[0].to_lowercase();

    match sub.as_str() {
        "list" => {
            let rules = fetch_rules(client, token).await?;
            costs::track_cost(
                &config.costs_path(),
                "stream_rules_list",
                "/2/tweets/search/stream/rules",
                0,
            );

            if args.json {
                println!("{}", serde_json::to_string_pretty(&rules)?);
                return Ok(());
            }

            if rules.is_empty() {
                println!("No stream rules configured.");
                return Ok(());
            }

            println!("\nStream Rules ({})\n", rules.len());
            for (idx, rule) in rules.iter().enumerate() {
                let tag = rule.tag.clone().unwrap_or_default();
                let tag_display = if tag.is_empty() {
                    String::new()
                } else {
                    format!(" [{tag}]")
                };
                println!("{}. {}{}", idx + 1, rule.id, tag_display);
                println!("   {}", rule.value);
            }
        }
        "add" => {
            let value = parts[1..].join(" ").trim().to_string();
            if value.is_empty() {
                bail!("Usage: xint stream-rules add <rule expression> [--tag <tag>]");
            }

            let mut rule_obj = serde_json::json!({ "value": value });
            if let Some(tag) = &args.tag {
                rule_obj["tag"] = serde_json::Value::String(tag.clone());
            }

            let body = serde_json::json!({ "add": [rule_obj] });
            let res = client
                .bearer_post("tweets/search/stream/rules", token, Some(&body))
                .await?;
            costs::track_cost(
                &config.costs_path(),
                "stream_rules_add",
                "/2/tweets/search/stream/rules",
                0,
            );

            if args.json {
                println!("{}", serde_json::to_string_pretty(&res)?);
            } else {
                println!("✅ Added stream rule.");
            }
        }
        "delete" => {
            let ids: Vec<String> = parts[1..].iter().map(ToString::to_string).collect();
            if ids.is_empty() {
                bail!("Usage: xint stream-rules delete <rule_id...>");
            }

            let body = serde_json::json!({ "delete": { "ids": ids } });
            let res = client
                .bearer_post("tweets/search/stream/rules", token, Some(&body))
                .await?;
            costs::track_cost(
                &config.costs_path(),
                "stream_rules_delete",
                "/2/tweets/search/stream/rules",
                0,
            );

            if args.json {
                println!("{}", serde_json::to_string_pretty(&res)?);
            } else {
                println!("✅ Deleted stream rule(s).");
            }
        }
        "clear" => {
            let rules = fetch_rules(client, token).await?;
            if rules.is_empty() {
                println!("No stream rules to clear.");
                return Ok(());
            }
            let ids: Vec<String> = rules.into_iter().map(|r| r.id).collect();
            let body = serde_json::json!({ "delete": { "ids": ids } });
            let res = client
                .bearer_post("tweets/search/stream/rules", token, Some(&body))
                .await?;
            costs::track_cost(
                &config.costs_path(),
                "stream_rules_delete",
                "/2/tweets/search/stream/rules",
                0,
            );
            if args.json {
                println!("{}", serde_json::to_string_pretty(&res)?);
            } else {
                println!("✅ Cleared all stream rules.");
            }
        }
        _ => {
            bail!("Unknown stream-rules subcommand: {sub}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_backfill;

    #[test]
    fn validate_backfill_accepts_valid_range() {
        assert_eq!(
            validate_backfill(Some(1)).expect("expected valid lower bound"),
            Some(1)
        );
        assert_eq!(
            validate_backfill(Some(5)).expect("expected valid upper bound"),
            Some(5)
        );
    }

    #[test]
    fn validate_backfill_rejects_invalid_values() {
        assert!(validate_backfill(Some(0)).is_err());
        assert!(validate_backfill(Some(6)).is_err());
        assert_eq!(validate_backfill(None).expect("expected None"), None);
    }
}
