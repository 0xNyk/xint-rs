use std::fs;

use anyhow::Result;
use colored::Colorize;
use serde::{Deserialize, Serialize};

use crate::api::{grok, twitter, xai};
use crate::auth::oauth;
use crate::cli::EngageArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::models::{BookmarkKnowledgeBase, GrokMessage, GrokOpts, Tweet};
use crate::spinner::Spinner;

use super::engagement::reply_to_tweet;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContentItem {
    url: String,
    text_preview: String,
    topics: Vec<String>,
    source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReplySuggestion {
    target_tweet_id: String,
    target_tweet_url: String,
    target_author: String,
    target_text: String,
    target_metrics: TargetMetrics,
    reply_text: String,
    linked_content_url: Option<String>,
    link_reason: Option<String>,
    confidence: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TargetMetrics {
    likes: u64,
    retweets: u64,
    impressions: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EngagePlan {
    generated_at: String,
    niche: String,
    model: String,
    suggestions: Vec<ReplySuggestion>,
    executed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    execution_results: Option<Vec<ExecutionResult>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExecutionResult {
    tweet_id: String,
    reply_id: String,
    success: bool,
}

#[derive(Debug, Deserialize)]
struct GrokSuggestion {
    target_tweet_id: String,
    reply_text: String,
    linked_content_url: Option<String>,
    link_reason: Option<String>,
    confidence: Option<u8>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn extract_tweet_id(url: &str) -> Option<String> {
    // Match status/<digits> in URLs
    let marker = "status/";
    let start = url.find(marker)? + marker.len();
    let rest = &url[start..];
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    Some(rest[..end].to_string())
}

fn strip_markdown_fences(text: &str) -> String {
    let trimmed = text.trim();
    let without_start = if trimmed.starts_with("```json") {
        trimmed.strip_prefix("```json").unwrap_or(trimmed)
    } else if trimmed.starts_with("```") {
        trimmed.strip_prefix("```").unwrap_or(trimmed)
    } else {
        trimmed
    };
    let without_end = if without_start.trim_end().ends_with("```") {
        without_start
            .trim_end()
            .strip_suffix("```")
            .unwrap_or(without_start)
    } else {
        without_start
    };
    without_end.trim().to_string()
}

fn confidence_bar(score: u8) -> String {
    let filled = score.min(5) as usize;
    let empty = 5 - filled;
    format!("{}{}", "\u{2588}".repeat(filled), "\u{2591}".repeat(empty))
}

fn load_kb(config: &Config) -> Option<BookmarkKnowledgeBase> {
    let path = config.data_dir.join("bookmark-kb.json");
    if !path.exists() {
        return None;
    }
    let data = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

// ---------------------------------------------------------------------------
// Command
// ---------------------------------------------------------------------------

pub async fn run(args: &EngageArgs, config: &Config, client: &XClient) -> Result<()> {
    let api_key = config.require_xai_key()?;
    let client_id = config.require_client_id()?;
    let (access_token, tokens) =
        oauth::get_valid_token(client, &config.tokens_path(), client_id).await?;
    let bearer = config.require_bearer_token()?;
    let user_id = &tokens.user_id;

    let niche = &args.niche;
    let count = args.count as usize;
    let since = &args.since;

    // 1. Discover tweets via xSearch
    let spinner = Spinner::new("Discovering high-engagement tweets...");

    let http = reqwest::Client::new();

    let from_date = twitter::parse_since(since)
        .map(|ts| ts.split('T').next().unwrap_or("").to_string());
    let search_query = format!("popular {niche} tweets with many likes");
    let (search_results, _summary, citations) = xai::x_search(
        &http,
        &api_key,
        &search_query,
        (count * 3).max(10) as u32,
        from_date.as_deref(),
        None,
        "grok-4-1-fast",
        60,
        None,
        None,
        false,
    )
    .await?;

    // Extract tweet IDs from search results + citations
    let mut tweet_ids: Vec<String> = Vec::new();
    for r in &search_results {
        let url = r.best_url();
        if let Some(id) = extract_tweet_id(&url) {
            if !tweet_ids.contains(&id) {
                tweet_ids.push(id);
            }
        }
    }
    for c in &citations {
        if let Some(ref url) = c.url {
            if let Some(id) = extract_tweet_id(url) {
                if !tweet_ids.contains(&id) {
                    tweet_ids.push(id);
                }
            }
        }
    }

    drop(spinner);
    let spinner = Spinner::new("Enriching tweets with metrics...");

    // Fetch each tweet for exact metrics
    let mut enriched: Vec<Tweet> = Vec::new();
    for id in tweet_ids.iter().take(count * 2) {
        match twitter::get_tweet(client, &bearer, id).await {
            Ok(Some(tweet)) => enriched.push(tweet),
            _ => {} // Skip inaccessible tweets
        }
    }

    // Also discover from home feed (accounts user follows)
    drop(spinner);
    let spinner = Spinner::new("Scanning home feed for niche tweets...");
    {
        let niche_words: Vec<String> = niche.to_lowercase().split_whitespace().map(String::from).collect();
        let feed_fields = "tweet.fields=created_at,public_metrics,entities,note_tweet&expansions=author_id&user.fields=username,name,public_metrics";
        let feed_path = format!(
            "users/{user_id}/timelines/reverse_chronological?max_results=100&{feed_fields}"
        );
        if let Ok(raw) = client.oauth_get(&feed_path, &access_token).await {
            let feed_tweets = twitter::parse_tweets(&raw);
            let seen_ids: std::collections::HashSet<String> =
                enriched.iter().map(|t| t.id.clone()).collect();
            for t in feed_tweets {
                if seen_ids.contains(&t.id) {
                    continue;
                }
                let text_lower = t.text.to_lowercase();
                let matches_niche = niche_words.iter().any(|w| text_lower.contains(w.as_str()));
                let engagement = t.metrics.likes + t.metrics.retweets;
                if matches_niche && engagement >= 5 {
                    enriched.push(t);
                }
            }
        }
    }

    // Sort by engagement
    enriched.sort_by(|a, b| {
        let score_a = a.metrics.likes + a.metrics.retweets + a.metrics.bookmarks;
        let score_b = b.metrics.likes + b.metrics.retweets + b.metrics.bookmarks;
        score_b.cmp(&score_a)
    });
    enriched.truncate(count);

    if enriched.is_empty() {
        spinner.fail("No tweets found.");
        eprintln!(
            "No tweets found for the given niche. Try broader keywords or a longer time window."
        );
        return Ok(());
    }

    spinner.done(&format!("Found {} targets", enriched.len()));
    let spinner = Spinner::new("Loading your content...");

    // 2. Build content pool
    let mut content_pool: Vec<ContentItem> = Vec::new();

    // Explicit links
    if let Some(ref links) = args.links {
        for url in links.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
            content_pool.push(ContentItem {
                url: url.to_string(),
                text_preview: url.to_string(),
                topics: vec![],
                source: "explicit".to_string(),
            });
        }
    }

    // User timeline
    if let Ok(timeline) =
        twitter::get_user_timeline(client, &access_token, user_id, 100, 3, true, true, Some("30d"))
            .await
    {
        for t in &timeline {
            let url = t.tweet_url.clone();
            let preview = if t.text.len() > 200 {
                &t.text[..200]
            } else {
                &t.text
            };
            content_pool.push(ContentItem {
                url,
                text_preview: preview.to_string(),
                topics: vec![],
                source: "timeline".to_string(),
            });
        }
    }

    // Bookmark KB
    if !args.no_kb {
        if let Some(kb) = load_kb(config) {
            for ext in kb.extractions.iter().filter(|e| e.importance >= 3) {
                let url = if !ext.tweet_url.is_empty() {
                    ext.tweet_url.clone()
                } else if let Some(u) = ext.urls.first() {
                    u.clone()
                } else {
                    continue;
                };
                let preview = if ext.summary.len() > 200 {
                    &ext.summary[..200]
                } else {
                    &ext.summary
                };
                content_pool.push(ContentItem {
                    url,
                    text_preview: preview.to_string(),
                    topics: ext.topics.clone(),
                    source: "bookmark-kb".to_string(),
                });
            }
        }
    }

    drop(spinner);
    let spinner = Spinner::new("Generating reply suggestions with Grok...");

    // 3. Generate replies via Grok
    let target_data: Vec<serde_json::Value> = enriched
        .iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "url": t.tweet_url,
                "author": t.username,
                "text": &t.text[..t.text.len().min(500)],
                "metrics": {
                    "likes": t.metrics.likes,
                    "retweets": t.metrics.retweets,
                    "impressions": t.metrics.impressions,
                }
            })
        })
        .collect();

    let content_data: Vec<serde_json::Value> = content_pool
        .iter()
        .take(30)
        .map(|c| {
            let preview = if c.text_preview.len() > 200 {
                &c.text_preview[..200]
            } else {
                &c.text_preview
            };
            serde_json::json!({
                "url": c.url,
                "text_preview": preview,
                "topics": &c.topics[..c.topics.len().min(5)],
                "source": c.source,
            })
        })
        .collect();

    let system_prompt = r#"You are a strategic engagement advisor for X/Twitter. For each target tweet, craft a reply that:
1. Adds genuine value (insight, data, different angle, practical tip)
2. Naturally includes the linked URL INSIDE the reply_text itself (at the end or woven in) so the reply is copy-paste ready
3. Does NOT feel like spam or self-promotion
4. Is concise (under 280 chars preferred, 400 max)
5. Matches the tone of the original tweet

IMPORTANT: The reply_text MUST contain the full link URL if one is chosen. The user will copy-paste reply_text directly.

Return ONLY a JSON array (no markdown fences): [{ "target_tweet_id": "...", "reply_text": "...", "linked_content_url": "..." or null, "link_reason": "..." or null, "confidence": 1-5 }]
confidence: 1-5 (5 = perfect fit). Set linked_content_url to null if nothing fits naturally."#;

    let user_prompt = format!(
        "Target tweets:\n{}\n\nUser's content pool:\n{}",
        serde_json::to_string_pretty(&target_data)?,
        serde_json::to_string_pretty(&content_data)?,
    );

    let messages = vec![
        GrokMessage {
            role: "system".into(),
            content: system_prompt.into(),
        },
        GrokMessage {
            role: "user".into(),
            content: user_prompt,
        },
    ];

    let opts = GrokOpts {
        model: args.model.clone(),
        temperature: 0.7,
        max_tokens: 4096,
    };

    let grok_resp = grok::grok_chat_tracked(
        &http,
        &api_key,
        &messages,
        &opts,
        Some(&config.costs_path()),
    )
    .await?;

    let cleaned = strip_markdown_fences(&grok_resp.content);
    let parsed: Vec<GrokSuggestion> = serde_json::from_str(&cleaned)
        .map_err(|e| anyhow::anyhow!("Failed to parse Grok response: {e}\nRaw: {cleaned}"))?;

    // Build suggestions
    let mut suggestions: Vec<ReplySuggestion> = Vec::new();
    for p in &parsed {
        let target = target_data
            .iter()
            .find(|t| t["id"].as_str() == Some(&p.target_tweet_id));
        let Some(target) = target else { continue };

        suggestions.push(ReplySuggestion {
            target_tweet_id: p.target_tweet_id.clone(),
            target_tweet_url: target["url"].as_str().unwrap_or("").to_string(),
            target_author: target["author"].as_str().unwrap_or("unknown").to_string(),
            target_text: target["text"].as_str().unwrap_or("").to_string(),
            target_metrics: TargetMetrics {
                likes: target["metrics"]["likes"].as_u64().unwrap_or(0),
                retweets: target["metrics"]["retweets"].as_u64().unwrap_or(0),
                impressions: target["metrics"]["impressions"].as_u64().unwrap_or(0),
            },
            reply_text: p.reply_text.clone(),
            linked_content_url: p.linked_content_url.clone(),
            link_reason: p.link_reason.clone(),
            confidence: p.confidence.unwrap_or(3).min(5).max(1),
        });
    }

    spinner.done(&format!("Generated {} suggestions", suggestions.len()));

    if suggestions.is_empty() {
        eprintln!("No reply suggestions generated. Try different niche keywords.");
        return Ok(());
    }

    // 4. Display
    let plan = EngagePlan {
        generated_at: chrono::Utc::now().to_rfc3339(),
        niche: niche.clone(),
        model: args.model.clone(),
        suggestions: suggestions.clone(),
        executed: false,
        execution_results: None,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
    } else {
        println!(
            "\n{} -- {} ({} suggestions)\n",
            "Engage Plan".bold(),
            niche.cyan(),
            suggestions.len()
        );

        for (i, s) in suggestions.iter().enumerate() {
            let metrics = format!(
                "{}L {}RT {}I",
                s.target_metrics.likes, s.target_metrics.retweets, s.target_metrics.impressions
            )
            .dimmed();
            println!(
                "{} {} {}",
                format!("{}.", i + 1).bold(),
                format!("@{}", s.target_author).bold(),
                metrics
            );

            let preview = if s.target_text.len() > 200 {
                &s.target_text[..200]
            } else {
                &s.target_text
            };
            println!("   {}", preview.dimmed());
            println!();
            println!("   {} {}", "Comment on:".yellow(), s.target_tweet_url);
            println!();
            println!("   {}", "Reply to paste:".yellow());
            println!("   {}", s.reply_text.green());
            if let Some(ref _url) = s.linked_content_url {
                let reason = s.link_reason.as_deref().unwrap_or("relevant content");
                println!();
                println!("   {}", format!("Link reason: {reason}").dimmed());
            }
            println!();
            println!(
                "   {} {} {}/5",
                "Confidence:".yellow(),
                confidence_bar(s.confidence),
                s.confidence
            );
            println!();
        }
    }

    // 5. Execute
    if args.execute {
        println!("{}\n", "\nExecuting replies...".bold());
        let mut results: Vec<ExecutionResult> = Vec::new();

        for (i, s) in suggestions.iter().enumerate() {
            match reply_to_tweet(client, &access_token, &s.target_tweet_id, &s.reply_text).await {
                Ok(reply_id) => {
                    results.push(ExecutionResult {
                        tweet_id: s.target_tweet_id.clone(),
                        reply_id: reply_id.clone(),
                        success: true,
                    });
                    println!(
                        "{}",
                        format!("Replied to @{}: {}", s.target_author, reply_id).green()
                    );
                    costs::track_cost(&config.costs_path(), "engage_reply", "/2/tweets", 0);
                }
                Err(e) => {
                    results.push(ExecutionResult {
                        tweet_id: s.target_tweet_id.clone(),
                        reply_id: String::new(),
                        success: false,
                    });
                    eprintln!("Failed to reply to @{}: {}", s.target_author, e);
                }
            }
            // Delay between replies
            if i < suggestions.len() - 1 {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }

        let succeeded = results.iter().filter(|r| r.success).count();
        println!(
            "\n{} {}/{} replies posted",
            "Results:".bold(),
            succeeded,
            results.len()
        );
    }

    // 6. Save
    if args.save {
        let exports_dir = config.data_dir.join("exports");
        fs::create_dir_all(&exports_dir)?;
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let filepath = exports_dir.join(format!("engage-plan-{date}.json"));
        fs::write(&filepath, serde_json::to_string_pretty(&plan)?)?;
        eprintln!("\nPlan saved to {}", filepath.display());
    }

    if !args.execute {
        eprintln!(
            "{}",
            "\nDry-run mode. Add --execute to post replies.".dimmed()
        );
    }

    Ok(())
}
