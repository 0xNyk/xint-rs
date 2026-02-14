use anyhow::{bail, Result};
use crate::models::*;

const XAI_ENDPOINT: &str = "https://api.x.ai/v1/chat/completions";

// Rough pricing per 1M tokens (USD)
fn model_pricing(model: &str) -> (f64, f64) {
    match model {
        "grok-3" => (3.00, 15.00),
        "grok-3-mini" => (0.10, 0.40),
        "grok-2" => (2.00, 10.00),
        _ => (0.10, 0.40),
    }
}

/// Estimate cost from token usage.
pub fn estimate_cost(model: &str, prompt_tokens: u64, completion_tokens: u64) -> String {
    let (input_rate, output_rate) = model_pricing(model);
    let input_cost = (prompt_tokens as f64 / 1_000_000.0) * input_rate;
    let output_cost = (completion_tokens as f64 / 1_000_000.0) * output_rate;
    let total = input_cost + output_cost;

    if total < 0.0001 {
        "<$0.0001".to_string()
    } else {
        format!("~${:.4}", total)
    }
}

/// Send a chat completion request to xAI's Grok API.
pub async fn grok_chat(
    http: &reqwest::Client,
    api_key: &str,
    messages: &[GrokMessage],
    opts: &GrokOpts,
) -> Result<GrokResponse> {
    let body = serde_json::json!({
        "model": opts.model,
        "messages": messages,
        "temperature": opts.temperature,
        "max_tokens": opts.max_tokens,
    });

    let res = http
        .post(XAI_ENDPOINT)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = res.status().as_u16();

    if status == 401 {
        bail!("xAI auth failed (401). Check your XAI_API_KEY.");
    }
    if status == 402 {
        bail!("xAI payment required (402). Your account may be out of credits.");
    }
    if status == 429 {
        bail!("xAI rate limited (429). Try again in a moment.");
    }
    if !res.status().is_success() {
        let text = res.text().await.unwrap_or_default();
        bail!("xAI API error ({}): {}", status, &text[..text.len().min(200)]);
    }

    let data: serde_json::Value = res.json().await?;

    let content = data
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("xAI API returned no choices"))?
        .to_string();

    let model = data
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or(&opts.model)
        .to_string();

    let usage = GrokUsage {
        prompt_tokens: data.pointer("/usage/prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        completion_tokens: data.pointer("/usage/completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        total_tokens: data.pointer("/usage/total_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
    };

    Ok(GrokResponse { content, model, usage })
}

// ---------------------------------------------------------------------------
// Analysis helpers
// ---------------------------------------------------------------------------

const TWEET_ANALYST_SYSTEM: &str = "You are a social media analyst specializing in X/Twitter. Provide concise, actionable insights. Use bullet points where appropriate. Focus on patterns, sentiment, and engagement signals.";

const GENERAL_ANALYST_SYSTEM: &str = "You are a social media analyst. Provide concise, actionable insights.";

/// Format tweets as context for Grok analysis.
pub fn format_tweets_for_context(tweets: &[Tweet]) -> String {
    tweets
        .iter()
        .enumerate()
        .map(|(i, t)| {
            format!(
                "[{}] @{} ({}L {}RT {}I) {}\n{}",
                i + 1,
                t.username,
                t.metrics.likes,
                t.metrics.retweets,
                t.metrics.impressions,
                t.created_at,
                t.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Analyze tweets with Grok.
pub async fn analyze_tweets(
    http: &reqwest::Client,
    api_key: &str,
    tweets: &[Tweet],
    prompt: Option<&str>,
    opts: &GrokOpts,
) -> Result<GrokResponse> {
    if tweets.is_empty() {
        bail!("No tweets to analyze");
    }

    let context = format_tweets_for_context(tweets);
    let user_message = prompt
        .unwrap_or("Analyze these tweets. Identify key themes, sentiment, notable insights, and engagement patterns.");

    let messages = vec![
        GrokMessage {
            role: "system".to_string(),
            content: TWEET_ANALYST_SYSTEM.to_string(),
        },
        GrokMessage {
            role: "user".to_string(),
            content: format!("Here are {} tweets:\n\n{}\n\n{}", tweets.len(), context, user_message),
        },
    ];

    grok_chat(http, api_key, &messages, opts).await
}

/// General-purpose query with optional context.
pub async fn analyze_query(
    http: &reqwest::Client,
    api_key: &str,
    query: &str,
    context: Option<&str>,
    opts: &GrokOpts,
) -> Result<GrokResponse> {
    let user_content = match context {
        Some(ctx) => format!("Context:\n{}\n\nQuestion: {}", ctx, query),
        None => query.to_string(),
    };

    let messages = vec![
        GrokMessage {
            role: "system".to_string(),
            content: GENERAL_ANALYST_SYSTEM.to_string(),
        },
        GrokMessage {
            role: "user".to_string(),
            content: user_content,
        },
    ];

    grok_chat(http, api_key, &messages, opts).await
}

/// Summarize trending topics.
#[allow(dead_code)]
pub async fn summarize_trends(
    http: &reqwest::Client,
    api_key: &str,
    topics: &[String],
    opts: &GrokOpts,
) -> Result<GrokResponse> {
    if topics.is_empty() {
        bail!("No topics to summarize");
    }

    let topic_list: String = topics
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{}. {}", i + 1, t))
        .collect::<Vec<_>>()
        .join("\n");

    let messages = vec![
        GrokMessage {
            role: "system".to_string(),
            content: "You are a trend analyst. Explain why each topic is trending, identify connections between topics, and note potential implications. Be concise.".to_string(),
        },
        GrokMessage {
            role: "user".to_string(),
            content: format!(
                "These topics are currently trending on X/Twitter:\n\n{}\n\nExplain why each is trending and identify any connections between them.",
                topic_list
            ),
        },
    ];

    grok_chat(http, api_key, &messages, opts).await
}
