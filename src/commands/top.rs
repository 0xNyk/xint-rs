use anyhow::Result;
use colored::Colorize;

use crate::api::twitter;
use crate::auth::oauth;
use crate::cli::TopArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::format;
use crate::models::Tweet;

pub async fn run(args: &TopArgs, config: &Config, client: &XClient) -> Result<()> {
    let client_id = config.require_client_id()?;
    let (access_token, tokens) =
        oauth::get_valid_token(client, &config.tokens_path(), client_id).await?;

    eprintln!("Fetching tweets (since {})...", args.since);

    let tweets = twitter::get_user_timeline(
        client,
        &access_token,
        &tokens.user_id,
        100,
        5,
        true,
        true,
        Some(&args.since),
    )
    .await?;

    costs::track_cost(
        &config.costs_path(),
        "timeline",
        &format!("/2/users/{}/tweets", tokens.user_id),
        tweets.len() as u64,
    );

    if tweets.is_empty() {
        println!("No tweets found in the specified time window.");
        return Ok(());
    }

    // Classify and filter by type
    let tweets: Vec<Tweet> = match args.r#type.as_deref() {
        Some("thread") => tweets
            .into_iter()
            .filter(|t| classify_type(t) == "thread")
            .collect(),
        Some("media") => tweets
            .into_iter()
            .filter(|t| classify_type(t) == "media")
            .collect(),
        Some("single") => tweets
            .into_iter()
            .filter(|t| classify_type(t) == "single")
            .collect(),
        _ => tweets,
    };

    if tweets.is_empty() {
        println!("No tweets matching the specified type filter.");
        return Ok(());
    }

    // Sort by chosen metric
    let mut sorted: Vec<&Tweet> = tweets.iter().collect();
    sorted.sort_by(|a, b| {
        let val = |t: &Tweet| -> f64 {
            match args.by.as_str() {
                "likes" => t.metrics.likes as f64,
                "impressions" => t.metrics.impressions as f64,
                "retweets" => t.metrics.retweets as f64,
                _ => engagement_rate(t),
            }
        };
        val(b)
            .partial_cmp(&val(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let top: Vec<&Tweet> = sorted.into_iter().take(args.limit).collect();

    if args.json {
        let summaries: Vec<serde_json::Value> = top
            .iter()
            .enumerate()
            .map(|(i, t)| {
                serde_json::json!({
                    "rank": i + 1,
                    "id": t.id,
                    "text": t.text,
                    "type": classify_type(t),
                    "likes": t.metrics.likes,
                    "impressions": t.metrics.impressions,
                    "retweets": t.metrics.retweets,
                    "engagement_rate": (engagement_rate(t) * 100.0).round() / 100.0,
                    "url": t.tweet_url,
                    "created_at": t.created_at,
                })
            })
            .collect();
        format::print_json_pretty_filtered(&summaries)?;
    } else {
        let type_label = args.r#type.as_deref().unwrap_or("all");
        println!(
            "\n{}  Top {} tweets by {} (type: {}, since {})\n",
            "Best Performers".bold(),
            args.limit,
            args.by.cyan(),
            type_label,
            args.since
        );

        for (i, t) in top.iter().enumerate() {
            let preview = if t.text.len() > 100 {
                format!(
                    "{}...",
                    &t.text[..t
                        .text
                        .char_indices()
                        .nth(97)
                        .map(|(idx, _)| idx)
                        .unwrap_or(t.text.len())]
                )
            } else {
                t.text.clone()
            };

            let er = engagement_rate(t);
            let content_type = classify_type(t);

            println!(
                "  {}. {} [{}]\n     {} likes | {} views | {}% ER\n     {}\n",
                (i + 1).to_string().bold(),
                preview.dimmed(),
                content_type.cyan(),
                compact_number(t.metrics.likes).yellow(),
                compact_number(t.metrics.impressions),
                format!("{:.2}", er).green(),
                t.tweet_url.dimmed()
            );
        }
    }

    Ok(())
}

fn engagement_rate(t: &Tweet) -> f64 {
    if t.metrics.impressions == 0 {
        return 0.0;
    }
    let engagements = t.metrics.likes + t.metrics.retweets + t.metrics.replies + t.metrics.quotes;
    engagements as f64 / t.metrics.impressions as f64 * 100.0
}

fn classify_type(t: &Tweet) -> &'static str {
    // Thread: conversation_id matches the tweet's own id (thread starter)
    // or the tweet text is very long (note tweet / thread continuation)
    if !t.conversation_id.is_empty() && t.conversation_id == t.id && t.text.len() > 280 {
        return "thread";
    }
    // Media: has t.co links but no URL entities (typically image/video attachments)
    if t.text.contains("https://t.co/") && t.urls.is_empty() {
        return "media";
    }
    // Links: has URL entities
    if !t.urls.is_empty() {
        return "link";
    }
    "single"
}

fn compact_number(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
