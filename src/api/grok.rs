use crate::costs;
use crate::models::*;
use anyhow::{bail, Result};
use std::path::Path;

const XAI_ENDPOINT: &str = "https://api.x.ai/v1/chat/completions";

pub const DEFAULT_MODEL: &str = "grok-4-1-fast";
pub const DEFAULT_VISION_MODEL: &str = "grok-4.3";

/// Budget tiers map to current Grok models. Cheap is the default — keeps
/// free console.x.ai credits ($175/mo) stretching ~5x further than `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Budget {
    Cheap,
    Balanced,
    Max,
}

impl std::str::FromStr for Budget {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "cheap" => Ok(Budget::Cheap),
            "balanced" => Ok(Budget::Balanced),
            "max" => Ok(Budget::Max),
            other => Err(format!("budget must be cheap | balanced | max (got {other})")),
        }
    }
}

impl Budget {
    pub fn model(self) -> &'static str {
        match self {
            Budget::Cheap => "grok-4-1-fast",
            Budget::Balanced => "grok-4.3",
            Budget::Max => "grok-4.20-reasoning",
        }
    }
}

pub fn resolve_model(explicit: Option<&str>, budget: Option<Budget>, has_image: bool) -> String {
    if let Some(m) = explicit {
        return m.to_string();
    }
    if has_image {
        return DEFAULT_VISION_MODEL.to_string();
    }
    if let Some(b) = budget {
        return b.model().to_string();
    }
    DEFAULT_MODEL.to_string()
}

// Pricing per 1M tokens (USD), current as of 2026-05-14.
// xAI retires grok-4, grok-3*, grok-2*, grok-code-fast-1 on 2026-05-15
// (auto-redirects to grok-4.3). Kept here as aliases for compat.
fn model_pricing(model: &str) -> (f64, f64) {
    match model {
        // Current lineup
        "grok-4.3" => (1.25, 2.50),
        "grok-4.20" | "grok-4.20-reasoning" | "grok-4.20-non-reasoning" => (2.00, 6.00),
        "grok-4-1-fast"
        | "grok-4-1-fast-reasoning"
        | "grok-4-1-fast-non-reasoning" => (0.20, 0.50),
        // Retiring 2026-05-15
        "grok-4.20-beta" => (2.00, 6.00),
        "grok-4" => (3.00, 15.00),
        "grok-code-fast-1" => (0.20, 1.50),
        "grok-3" => (3.00, 15.00),
        "grok-3-mini" => (0.10, 0.40),
        "grok-2" | "grok-2-vision" => (2.00, 10.00),
        _ => (0.20, 0.50),
    }
}

/// Resolve true USD cost: prefer `cost_in_usd_ticks` (µUSD) from xAI's
/// response when present; fall back to local estimate otherwise.
fn cost_from_response(
    ticks: Option<i64>,
    model: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
) -> f64 {
    if let Some(t) = ticks {
        if t >= 0 {
            return t as f64 / 1_000_000.0;
        }
    }
    let (input_rate, output_rate) = model_pricing(model);
    (prompt_tokens as f64 / 1_000_000.0) * input_rate
        + (completion_tokens as f64 / 1_000_000.0) * output_rate
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
        format!("~${total:.4}")
    }
}

/// Send a chat completion request to xAI's Grok API.
pub async fn grok_chat(
    http: &reqwest::Client,
    api_key: &str,
    messages: &[GrokMessage],
    opts: &GrokOpts,
) -> Result<GrokResponse> {
    grok_chat_tracked(http, api_key, messages, opts, None).await
}

/// Send a chat completion request with cost tracking.
pub async fn grok_chat_tracked(
    http: &reqwest::Client,
    api_key: &str,
    messages: &[GrokMessage],
    opts: &GrokOpts,
    costs_path: Option<&Path>,
) -> Result<GrokResponse> {
    let body = serde_json::json!({
        "model": opts.model,
        "messages": messages,
        "temperature": opts.temperature,
        "max_tokens": opts.max_tokens,
    });

    let res = http
        .post(XAI_ENDPOINT)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = res.status().as_u16();

    if status == 401 {
        bail!("xAI auth failed (401). Check your XAI_API_KEY.");
    }
    if status == 402 {
        bail!(
            "xAI payment required (402). Your free tier may be exhausted — \
             run `xint credits` to see your burn rate, or top up at https://console.x.ai"
        );
    }
    if status == 429 {
        bail!("xAI rate limited (429). Try again in a moment.");
    }
    if !res.status().is_success() {
        let text = res.text().await.unwrap_or_default();
        bail!(
            "xAI API error ({}): {}",
            status,
            &text[..text.len().min(200)]
        );
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
        prompt_tokens: data
            .pointer("/usage/prompt_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        completion_tokens: data
            .pointer("/usage/completion_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        total_tokens: data
            .pointer("/usage/total_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    };

    // xAI started returning `usage.cost_in_usd_ticks` (µUSD) in 2026-Q2; use
    // authoritative value when present, fall back to our pricing table.
    let ticks = data
        .pointer("/usage/cost_in_usd_ticks")
        .and_then(|v| v.as_i64());

    if let Some(cp) = costs_path {
        let cost_usd = cost_from_response(ticks, &model, usage.prompt_tokens, usage.completion_tokens);
        costs::track_cost_direct(cp, "grok_chat", XAI_ENDPOINT, cost_usd);
    }

    Ok(GrokResponse {
        content,
        model,
        usage,
    })
}

// ---------------------------------------------------------------------------
// Analysis helpers
// ---------------------------------------------------------------------------

const TWEET_ANALYST_SYSTEM: &str = "You are a social media analyst specializing in X/Twitter. Provide concise, actionable insights. Use bullet points where appropriate. Focus on patterns, sentiment, and engagement signals.";

const GENERAL_ANALYST_SYSTEM: &str =
    "You are a social media analyst. Provide concise, actionable insights.";

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
#[allow(dead_code)]
pub async fn analyze_tweets(
    http: &reqwest::Client,
    api_key: &str,
    tweets: &[Tweet],
    prompt: Option<&str>,
    opts: &GrokOpts,
) -> Result<GrokResponse> {
    analyze_tweets_tracked(http, api_key, tweets, prompt, opts, None).await
}

/// Analyze tweets with Grok and cost tracking.
pub async fn analyze_tweets_tracked(
    http: &reqwest::Client,
    api_key: &str,
    tweets: &[Tweet],
    prompt: Option<&str>,
    opts: &GrokOpts,
    costs_path: Option<&Path>,
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
            content: format!(
                "Here are {} tweets:\n\n{}\n\n{}",
                tweets.len(),
                context,
                user_message
            ),
        },
    ];

    grok_chat_tracked(http, api_key, &messages, opts, costs_path).await
}

/// General-purpose query with optional context.
#[allow(dead_code)]
pub async fn analyze_query(
    http: &reqwest::Client,
    api_key: &str,
    query: &str,
    context: Option<&str>,
    opts: &GrokOpts,
) -> Result<GrokResponse> {
    analyze_query_tracked(http, api_key, query, context, opts, None).await
}

/// General-purpose query with cost tracking.
pub async fn analyze_query_tracked(
    http: &reqwest::Client,
    api_key: &str,
    query: &str,
    context: Option<&str>,
    opts: &GrokOpts,
    costs_path: Option<&Path>,
) -> Result<GrokResponse> {
    let user_content = match context {
        Some(ctx) => format!("Context:\n{ctx}\n\nQuestion: {query}"),
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

    grok_chat_tracked(http, api_key, &messages, opts, costs_path).await
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
                "These topics are currently trending on X/Twitter:\n\n{topic_list}\n\nExplain why each is trending and identify any connections between them."
            ),
        },
    ];

    grok_chat(http, api_key, &messages, opts).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn budget_cheap_maps_to_grok_4_1_fast() {
        assert_eq!(Budget::from_str("cheap").unwrap().model(), "grok-4-1-fast");
    }

    #[test]
    fn budget_balanced_maps_to_grok_4_3() {
        assert_eq!(Budget::from_str("balanced").unwrap().model(), "grok-4.3");
    }

    #[test]
    fn budget_max_maps_to_grok_4_20_reasoning() {
        assert_eq!(Budget::from_str("max").unwrap().model(), "grok-4.20-reasoning");
    }

    #[test]
    fn budget_invalid_errors() {
        assert!(Budget::from_str("bogus").is_err());
    }

    #[test]
    fn resolve_model_default_is_cheap() {
        assert_eq!(resolve_model(None, None, false), "grok-4-1-fast");
    }

    #[test]
    fn resolve_model_image_overrides_budget() {
        // Image input must auto-route to vision-capable model.
        assert_eq!(
            resolve_model(None, Some(Budget::Max), true),
            DEFAULT_VISION_MODEL
        );
    }

    #[test]
    fn resolve_model_explicit_wins_over_budget() {
        // Explicit --model must take precedence.
        assert_eq!(
            resolve_model(Some("grok-4.20"), Some(Budget::Cheap), false),
            "grok-4.20"
        );
    }

    #[test]
    fn cost_from_response_prefers_ticks_when_present() {
        // 1234 µUSD = $0.001234; local estimate would be ~$0.000045
        let cost = cost_from_response(Some(1234), "grok-4-1-fast", 100, 50);
        assert!((cost - 0.001234).abs() < 1e-9);
    }

    #[test]
    fn cost_from_response_falls_back_when_ticks_missing() {
        // 1M prompt tokens × $0.20/M input = $0.20
        let cost = cost_from_response(None, "grok-4-1-fast", 1_000_000, 0);
        assert!((cost - 0.20).abs() < 1e-9);
    }

    #[test]
    fn cost_from_response_falls_back_when_ticks_negative() {
        // Negative ticks would be a bug — fall back rather than record nonsense.
        let cost = cost_from_response(Some(-1), "grok-4-1-fast", 1_000_000, 0);
        assert!((cost - 0.20).abs() < 1e-9);
    }

    #[test]
    fn pricing_grok_4_3_matches_2026_05_rates() {
        // Lock in current Grok 4.3 pricing so a careless edit doesn't drift.
        assert_eq!(model_pricing("grok-4.3"), (1.25, 2.50));
    }

    #[test]
    fn pricing_grok_4_1_fast_variants_share_rate() {
        let expected = (0.20, 0.50);
        assert_eq!(model_pricing("grok-4-1-fast"), expected);
        assert_eq!(model_pricing("grok-4-1-fast-reasoning"), expected);
        assert_eq!(model_pricing("grok-4-1-fast-non-reasoning"), expected);
    }
}
