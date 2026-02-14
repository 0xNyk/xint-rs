use anyhow::Result;
use crate::api::grok;
use crate::models::*;

const SENTIMENT_SYSTEM: &str = r#"You are a sentiment analysis engine. Given tweets, return a JSON array with sentiment analysis for each tweet.

For each tweet, provide:
- id: the tweet ID
- sentiment: one of "positive", "negative", "neutral", "mixed"
- score: a number from -1.0 (very negative) to 1.0 (very positive)
- label: a 2-5 word reason (e.g., "excited about launch", "frustrated with bugs", "neutral observation")

Detect sarcasm, irony, and context. Crypto/tech enthusiasm is positive. Complaints/frustration are negative. Questions and factual statements are neutral.

Return ONLY valid JSON array, no markdown fences, no explanation."#;

/// Analyze sentiment for a batch of tweets using Grok.
/// Processes in batches of 20 to stay within token limits.
pub async fn analyze_sentiment(
    http: &reqwest::Client,
    api_key: &str,
    tweets: &[Tweet],
    model: Option<&str>,
) -> Result<Vec<SentimentResult>> {
    if tweets.is_empty() {
        return Ok(Vec::new());
    }

    let batch_size = 20;
    let mut results = Vec::new();

    for chunk in tweets.chunks(batch_size) {
        let tweet_context: String = chunk
            .iter()
            .map(|t| {
                let text = if t.text.len() > 280 { &t.text[..280] } else { &t.text };
                format!("[{}] @{}: {}", t.id, t.username, text)
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let messages = vec![
            GrokMessage {
                role: "system".to_string(),
                content: SENTIMENT_SYSTEM.to_string(),
            },
            GrokMessage {
                role: "user".to_string(),
                content: format!("Analyze sentiment for these {} tweets:\n\n{}", chunk.len(), tweet_context),
            },
        ];

        let opts = GrokOpts {
            model: model.unwrap_or("grok-3-mini").to_string(),
            temperature: 0.3,
            max_tokens: 2048,
        };

        match grok::grok_chat(http, api_key, &messages, &opts).await {
            Ok(response) => {
                let parsed = parse_json_response(&response.content, chunk);
                results.extend(parsed);
            }
            Err(e) => {
                eprintln!("[sentiment] Batch analysis failed: {}", e);
                for t in chunk {
                    results.push(SentimentResult {
                        id: t.id.clone(),
                        sentiment: "neutral".to_string(),
                        score: 0.0,
                        label: Some("analysis failed".to_string()),
                    });
                }
            }
        }
    }

    Ok(results)
}

/// Parse Grok's JSON response, with fallback handling for malformed output.
fn parse_json_response(content: &str, tweets: &[Tweet]) -> Vec<SentimentResult> {
    let mut cleaned = content.trim().to_string();

    // Strip markdown code fences if present
    if cleaned.starts_with("```") {
        if let Some(start) = cleaned.find('\n') {
            cleaned = cleaned[start + 1..].to_string();
        }
        if cleaned.ends_with("```") {
            cleaned = cleaned[..cleaned.len() - 3].trim().to_string();
        }
    }

    // Try parsing as JSON array
    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&cleaned) {
        return arr.iter().map(|item| parse_sentiment_item(item)).collect();
    }

    // Fallback: try to extract JSON array from response
    if let Some(start) = cleaned.find('[') {
        if let Some(end) = cleaned.rfind(']') {
            let slice = &cleaned[start..=end];
            if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(slice) {
                return arr.iter().map(|item| parse_sentiment_item(item)).collect();
            }
        }
    }

    // Final fallback: return neutral for all
    tweets
        .iter()
        .map(|t| SentimentResult {
            id: t.id.clone(),
            sentiment: "neutral".to_string(),
            score: 0.0,
            label: Some("parse failed".to_string()),
        })
        .collect()
}

fn parse_sentiment_item(item: &serde_json::Value) -> SentimentResult {
    let id = item
        .get("id")
        .map(|v| match v {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            _ => "?".to_string(),
        })
        .unwrap_or_else(|| "?".to_string());

    let sentiment = item
        .get("sentiment")
        .and_then(|v| v.as_str())
        .map(validate_sentiment)
        .unwrap_or("neutral")
        .to_string();

    let score = item
        .get("score")
        .and_then(|v| v.as_f64())
        .map(|s| s.clamp(-1.0, 1.0))
        .map(|s| (s * 100.0).round() / 100.0)
        .unwrap_or(0.0);

    let label = item
        .get("label")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    SentimentResult {
        id,
        sentiment,
        score,
        label,
    }
}

fn validate_sentiment(s: &str) -> &str {
    match s {
        "positive" | "negative" | "neutral" | "mixed" => s,
        _ => "neutral",
    }
}

/// Compute aggregate sentiment stats.
pub fn compute_stats(sentiments: &[SentimentResult]) -> SentimentStats {
    let mut positive = 0u32;
    let mut negative = 0u32;
    let mut neutral = 0u32;
    let mut mixed = 0u32;
    let mut total_score = 0.0f64;

    for s in sentiments {
        match s.sentiment.as_str() {
            "positive" => positive += 1,
            "negative" => negative += 1,
            "mixed" => mixed += 1,
            _ => neutral += 1,
        }
        total_score += s.score;
    }

    let average_score = if sentiments.is_empty() {
        0.0
    } else {
        (total_score / sentiments.len() as f64 * 100.0).round() / 100.0
    };

    SentimentStats {
        positive,
        negative,
        neutral,
        mixed,
        average_score,
    }
}

/// Format aggregate sentiment stats for terminal display.
pub fn format_stats(stats: &SentimentStats, total: usize) -> String {
    let pct = |n: u32| -> String {
        if total > 0 {
            format!("{}%", (n as f64 / total as f64 * 100.0).round() as u32)
        } else {
            "0%".to_string()
        }
    };

    let score_bar = {
        let normalized = ((stats.average_score + 1.0) * 5.0).round() as usize;
        let filled = normalized.min(10);
        format!("[{}{}]", "#".repeat(filled), ".".repeat(10 - filled))
    };

    let mut out = format!("\nSentiment Analysis ({} tweets):\n", total);
    out.push_str(&format!("  Avg score: {:.2} {}\n", stats.average_score, score_bar));
    out.push_str(&format!("  + Positive: {} ({})\n", stats.positive, pct(stats.positive)));
    out.push_str(&format!("  - Negative: {} ({})\n", stats.negative, pct(stats.negative)));
    out.push_str(&format!("  = Neutral:  {} ({})\n", stats.neutral, pct(stats.neutral)));
    if stats.mixed > 0 {
        out.push_str(&format!("  ~ Mixed:    {} ({})\n", stats.mixed, pct(stats.mixed)));
    }

    out
}
