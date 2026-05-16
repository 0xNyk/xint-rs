//! Grok free-tier credit telemetry (Rust mirror of xint/lib/credits.ts).
//!
//! Computes a Grok-only view over costs.rs data so users can see how much
//! of their console.x.ai free tier they've consumed this calendar month.
//! Models xAI's $25 signup + $150/mo data-share = $175/mo free locally.

use anyhow::Result;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::cli::CreditsArgs;
use crate::config::Config;

const SIGNUP_CREDIT_USD: f64 = 25.0;
const MONTHLY_CREDIT_USD: f64 = 150.0;
const SIGNUP_VALIDITY_DAYS: i64 = 30;

fn grok_ops() -> HashSet<&'static str> {
    [
        "grok_chat",
        "grok_analyze",
        "grok_vision",
        "grok_sentiment",
        "xai_article",
        "xai_x_search",
        "grok_live_search",
    ]
    .into_iter()
    .collect()
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct CreditsMeta {
    /// ISO timestamp of first observed xAI call (anchors 30-day signup window).
    first_call_at: Option<String>,
    /// Whether user opted into data-sharing for the $150/mo bonus.
    #[serde(default = "default_true")]
    data_sharing: bool,
}

fn default_true() -> bool {
    true
}

fn credits_path(config: &Config) -> std::path::PathBuf {
    config.data_dir.join("credits.json")
}

fn load_meta(path: &Path) -> CreditsMeta {
    if !path.exists() {
        return CreditsMeta {
            first_call_at: None,
            data_sharing: true,
        };
    }
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_meta(path: &Path, meta: &CreditsMeta) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(meta)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&tmp)?.permissions();
        perms.set_mode(0o660);
        fs::set_permissions(&tmp, perms)?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

#[derive(serde::Deserialize)]
struct CostEntry {
    timestamp: String,
    operation: String,
    cost_usd: f64,
}

#[derive(serde::Deserialize, Default)]
struct CostsShape {
    #[serde(default)]
    entries: Vec<CostEntry>,
}

fn load_costs(path: &Path) -> CostsShape {
    if !path.exists() {
        return CostsShape::default();
    }
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn month_start(date: chrono::DateTime<chrono::Utc>) -> String {
    use chrono::Datelike;
    format!("{:04}-{:02}-01", date.year(), date.month())
}

fn next_month_start(date: chrono::DateTime<chrono::Utc>) -> String {
    use chrono::Datelike;
    let (y, m) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    format!("{y:04}-{m:02}-01")
}

fn feature_bucket(op: &str) -> &'static str {
    match op {
        "grok_chat" | "grok_analyze" => "chat",
        "grok_vision" => "vision",
        "grok_sentiment" => "sentiment",
        "xai_article" => "article",
        "xai_x_search" | "grok_live_search" => "live_search",
        _ => "other",
    }
}

pub fn is_premium(status: Option<&str>) -> bool {
    matches!(status, Some("Premium" | "PremiumPlus" | "Premium+"))
}

/// Print the Grok onboarding guide — same text as TS, kept in sync deliberately.
pub fn print_credit_guide(premium_status: Option<&str>) {
    if is_premium(premium_status) {
        let status = premium_status.unwrap();
        println!(
            "\nWe see you have X {status}. That gives you the Grok chatbot on x.com — separate from the API."
        );
        println!("To use Grok from xint/your agent, you still need a console.x.ai key. Here's why and how:\n");
    }

    println!("Grok API credits — how to get yours free\n");
    println!("  ✗ X Premium ($8/mo) / Premium+ ($40/mo) do NOT include API credits.");
    println!("    They unlock the Grok chatbot on x.com — a separate product.");
    println!("    No OAuth path from x.com to the API exists.\n");
    println!("  ✓ Free $175/month at console.x.ai (any account, no Premium needed):");
    println!("    1. Sign up at https://console.x.ai");
    println!("    2. Generate an API key");
    println!("    3. Opt into \"data sharing\" for the $150/mo bonus");
    println!("       ⚠ This means xAI may train models on your prompts.");
    println!("         Skip if you handle sensitive data — you'll still get $25/mo.");
    println!("    4. export XAI_API_KEY=xai-...\n");
    println!("xint routes your agent to the cheapest sufficient model by default");
    println!("(grok-4-1-fast — $0.20/$0.50 per M tokens). Run `xint credits`");
    println!("anytime to see your burn rate.\n");

    // Premium-routing tip — only emitted for Premium users since the
    // suggestion is useless to anyone without bundled chat allowance.
    if is_premium(premium_status) {
        println!("💡 Tip for Premium users: one-shot questions like \"what's trending?\"");
        println!("   can be pasted into https://grok.com instead — that spends your Premium");
        println!("   chat allowance, not API credits. xint is for the cases where you need");
        println!("   automation, piping, or agent workflows.\n");
    }
}

/// Suggest grok.com for one-shot human queries to spend Premium UI allowance
/// instead of API credits. Returns the tip string, or None if the heuristic
/// says "this query needs automation — chatbot won't work."
pub fn premium_chat_routing_tip(
    premium_status: Option<&str>,
    pipe_mode: bool,
    tweet_file: Option<&str>,
    image_url: Option<&str>,
    query: &str,
) -> Option<String> {
    if !is_premium(premium_status) {
        return None;
    }
    if pipe_mode || tweet_file.is_some() || image_url.is_some() {
        return None;
    }
    if query.is_empty() {
        return None;
    }
    let status = premium_status.unwrap();
    Some(format!(
        "💡 Premium tip: you can paste this question into https://grok.com to \
         spend your X {status} chat allowance instead of API credits. \
         Useful for one-shot questions; xint is best for piped, file-based, or agent flows."
    ))
}

pub fn run(args: &CreditsArgs, config: &Config) -> Result<()> {
    let meta_path = credits_path(config);

    if args.setup {
        // Honor XINT_X_PREMIUM env hint so Premium users see the routing tip.
        let premium = std::env::var("XINT_X_PREMIUM").ok();
        print_credit_guide(premium.as_deref());
        return Ok(());
    }

    if let Some(ref val) = args.data_sharing {
        if val != "on" && val != "off" {
            anyhow::bail!("--data-sharing must be on|off (got {val})");
        }
        let mut meta = load_meta(&meta_path);
        meta.data_sharing = val == "on";
        save_meta(&meta_path, &meta)?;
        println!(
            "Data-sharing set to {val}. Monthly free tier: {}.",
            if meta.data_sharing { "$150" } else { "$0" }
        );
        return Ok(());
    }

    // Status output
    let now = chrono::Utc::now();
    let costs = load_costs(&config.costs_path());
    let mut meta = load_meta(&meta_path);

    let ops = grok_ops();
    let grok_entries: Vec<&CostEntry> = costs
        .entries
        .iter()
        .filter(|e| ops.contains(e.operation.as_str()))
        .collect();

    // Record first call if not set yet.
    if meta.first_call_at.is_none() {
        if let Some(first) = grok_entries.first() {
            meta.first_call_at = Some(first.timestamp.clone());
            let _ = save_meta(&meta_path, &meta);
        }
    }

    let signup_expires_at = meta
        .first_call_at
        .as_deref()
        .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
        .map(|dt| {
            (dt + chrono::Duration::days(SIGNUP_VALIDITY_DAYS))
                .format("%Y-%m-%d")
                .to_string()
        });

    let lifetime_grok_spend: f64 = grok_entries.iter().map(|e| e.cost_usd).sum();
    let signup_used = lifetime_grok_spend.min(SIGNUP_CREDIT_USD);
    let signup_remaining = (SIGNUP_CREDIT_USD - signup_used).max(0.0);

    let month_floor = month_start(now);
    let month_grok: Vec<&&CostEntry> = grok_entries
        .iter()
        .filter(|e| &e.timestamp[..10] >= month_floor.as_str())
        .collect();
    let monthly_used: f64 = month_grok.iter().map(|e| e.cost_usd).sum();
    let monthly_total = if meta.data_sharing {
        MONTHLY_CREDIT_USD
    } else {
        0.0
    };
    let monthly_resets = next_month_start(now);

    println!("\nFree tier (xAI console)");
    let signup_exp_str = signup_expires_at
        .as_deref()
        .map(|d| format!("expires {d}"))
        .unwrap_or_else(|| "starts on first call".to_string());
    println!(
        "  Signup credit:    ${:.2} of ${:.2} remaining   {}",
        signup_remaining, SIGNUP_CREDIT_USD, signup_exp_str
    );

    if meta.data_sharing {
        println!(
            "  Monthly:          ${:.2} of ${:.2} used        refreshes {}",
            monthly_used, monthly_total, monthly_resets
        );
    } else {
        println!("  Monthly:          data-sharing OFF — $0/mo bonus. Run `xint credits --data-sharing on` to enable.");
    }

    // By-feature breakdown for this month
    let mut by_feature: std::collections::HashMap<&'static str, f64> =
        std::collections::HashMap::new();
    for e in &month_grok {
        *by_feature
            .entry(feature_bucket(&e.operation))
            .or_insert(0.0) += e.cost_usd;
    }
    // Filter out zero-cost buckets so empty state reads cleanly.
    by_feature.retain(|_, v| *v > 0.0);
    if !by_feature.is_empty() {
        println!("\nThis month by feature");
        let mut entries: Vec<_> = by_feature.iter().collect();
        entries.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (feature, cost) in entries {
            println!("  {feature:<16}  ${cost:.2}");
        }
    } else {
        println!("\nNo Grok calls this month yet. Try `xint analyze \"your question\"`.");
    }

    // Tip
    let chat_spend = *by_feature.get("chat").unwrap_or(&0.0);
    if monthly_used > 0.0 && chat_spend / monthly_used > 0.9 {
        println!(
            "\nTip: {}% of spend went to general queries — `--budget cheap` is working.",
            ((chat_spend / monthly_used) * 100.0).round() as u32
        );
    }

    println!();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_premium_recognizes_all_tier_strings() {
        assert!(is_premium(Some("Premium")));
        assert!(is_premium(Some("PremiumPlus")));
        assert!(is_premium(Some("Premium+")));
        assert!(!is_premium(Some("Free")));
        assert!(!is_premium(None));
    }

    #[test]
    fn tip_emitted_for_premium_oneshot_query() {
        let tip = premium_chat_routing_tip(Some("Premium+"), false, None, None, "trends in AI");
        assert!(tip.is_some());
        let tip = tip.unwrap();
        assert!(tip.contains("grok.com"));
        assert!(tip.contains("Premium+"));
    }

    #[test]
    fn tip_silent_for_non_premium() {
        assert!(premium_chat_routing_tip(None, false, None, None, "x").is_none());
        assert!(premium_chat_routing_tip(Some("Free"), false, None, None, "x").is_none());
    }

    #[test]
    fn tip_silent_when_piped() {
        assert!(premium_chat_routing_tip(Some("Premium"), true, None, None, "x").is_none());
    }

    #[test]
    fn tip_silent_when_reading_tweet_file() {
        assert!(
            premium_chat_routing_tip(Some("Premium"), false, Some("t.json"), None, "x").is_none()
        );
    }

    #[test]
    fn tip_silent_for_image_input() {
        assert!(
            premium_chat_routing_tip(Some("Premium+"), false, None, Some("img.png"), "what")
                .is_none()
        );
    }

    #[test]
    fn tip_silent_on_empty_query() {
        assert!(premium_chat_routing_tip(Some("Premium"), false, None, None, "").is_none());
    }
}
