use anyhow::Result;
use colored::Colorize;
use std::fs;

use crate::api::{grok, twitter};
use crate::auth::oauth;
use crate::cli::ContentAuditArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::models::{GrokMessage, GrokOpts, Tweet};

pub async fn run(args: &ContentAuditArgs, config: &Config, client: &XClient) -> Result<()> {
    let client_id = config.require_client_id()?;
    let api_key = config.require_xai_key()?;
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

    // Identify top and bottom performers
    let mut sorted: Vec<&Tweet> = tweets.iter().collect();
    sorted.sort_by(|a, b| {
        engagement_rate(b)
            .partial_cmp(&engagement_rate(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let top_5: Vec<&Tweet> = sorted.iter().take(5).copied().collect();
    let bottom_5: Vec<&Tweet> = sorted.iter().rev().take(5).copied().collect();

    // Build prompt for Grok
    let mut tweet_context = String::new();
    tweet_context.push_str("TOP PERFORMING TWEETS:\n");
    for (i, t) in top_5.iter().enumerate() {
        tweet_context.push_str(&format!(
            "{}. [{:.2}% ER, {} likes, {} impressions]\n\"{}\"\n\n",
            i + 1,
            engagement_rate(t),
            t.metrics.likes,
            t.metrics.impressions,
            truncate(&t.text, 300),
        ));
    }
    tweet_context.push_str("\nLOWEST PERFORMING TWEETS:\n");
    for (i, t) in bottom_5.iter().enumerate() {
        tweet_context.push_str(&format!(
            "{}. [{:.2}% ER, {} likes, {} impressions]\n\"{}\"\n\n",
            i + 1,
            engagement_rate(t),
            t.metrics.likes,
            t.metrics.impressions,
            truncate(&t.text, 300),
        ));
    }

    let total_tweets = tweets.len();
    let avg_er = tweets.iter().map(engagement_rate).sum::<f64>() / total_tweets as f64;
    let avg_likes =
        tweets.iter().map(|t| t.metrics.likes).sum::<u64>() as f64 / total_tweets as f64;

    let system_prompt = format!(
        "You are an expert social media strategist analyzing X/Twitter content performance. \
         The user has {} tweets in the last {} with an average engagement rate of {:.2}% \
         and average {} likes per tweet. Analyze their top and bottom performing content \
         and provide actionable recommendations.",
        total_tweets, args.since, avg_er, avg_likes as u64
    );

    let user_prompt = format!(
        "Analyze my tweet performance and give me:\n\
         1. What patterns make my top tweets successful\n\
         2. Why my bottom tweets underperformed\n\
         3. Content strategy recommendations (3-5 specific, actionable tips)\n\
         4. Suggested content themes based on what works\n\n\
         {}",
        tweet_context
    );

    eprintln!("Analyzing with {}...", args.model);

    let http = reqwest::Client::new();
    let opts = GrokOpts {
        model: args.model.clone(),
        temperature: 0.7,
        max_tokens: 1500,
    };
    let messages = vec![
        GrokMessage {
            role: "system".to_string(),
            content: system_prompt,
        },
        GrokMessage {
            role: "user".to_string(),
            content: user_prompt,
        },
    ];

    let response =
        grok::grok_chat_tracked(&http, api_key, &messages, &opts, Some(&config.costs_path()))
            .await?;

    costs::track_cost(&config.costs_path(), "content_audit", "grok", 0);

    // Display
    println!(
        "\n{}  @{} (last {})\n",
        "Content Audit".bold(),
        tokens.username.cyan(),
        args.since
    );
    println!(
        "  {} tweets analyzed | {:.2}% avg ER | {} avg likes\n",
        total_tweets.to_string().yellow(),
        avg_er,
        (avg_likes as u64).to_string().yellow()
    );
    println!("{}\n", response.content);

    if args.save {
        let exports_dir = config.exports_dir();
        fs::create_dir_all(&exports_dir)?;
        let path = exports_dir.join(format!(
            "content-audit-{}-{}.md",
            tokens.username,
            chrono::Utc::now().format("%Y%m%d-%H%M%S")
        ));
        let report = format!(
            "# Content Audit: @{}\n\n\
             **Date:** {}\n\
             **Period:** last {}\n\
             **Tweets analyzed:** {}\n\
             **Avg engagement rate:** {:.2}%\n\
             **Avg likes:** {}\n\n\
             ---\n\n\
             {}\n",
            tokens.username,
            chrono::Utc::now().format("%Y-%m-%d"),
            args.since,
            total_tweets,
            avg_er,
            avg_likes as u64,
            response.content
        );
        fs::write(&path, &report)?;
        eprintln!("Saved to {}", path.display());
    }

    Ok(())
}

fn engagement_rate(t: &Tweet) -> f64 {
    if t.metrics.impressions == 0 {
        return 0.0;
    }
    let eng = t.metrics.likes + t.metrics.retweets + t.metrics.replies + t.metrics.quotes;
    eng as f64 / t.metrics.impressions as f64 * 100.0
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let end = s
            .char_indices()
            .nth(max - 3)
            .map(|(i, _)| i)
            .unwrap_or(s.len());
        format!("{}...", &s[..end])
    }
}
