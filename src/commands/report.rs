use anyhow::Result;
use std::fs;

use crate::api::{grok, twitter};
use crate::cli::ReportArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::format;
use crate::models::*;
use crate::sentiment;

pub async fn run(args: &ReportArgs, config: &Config, client: &XClient) -> Result<()> {
    let token = config.require_bearer_token()?;
    let query = args.topic.join(" ");

    if query.is_empty() {
        println!("Usage: xint report <topic> [options]");
        return Ok(());
    }

    let time = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let date = &time[..10];

    eprintln!("Generating report: \"{}\"...", query);

    // 1. Main topic search
    eprintln!("  Searching \"{}\"...", query);
    let tweets = twitter::search(
        client,
        token,
        &query,
        args.pages,
        "relevancy",
        Some("1d"),
        None,
        false,
    )
    .await?;

    costs::track_cost(
        &config.costs_path(),
        "search",
        "/2/tweets/search/recent",
        tweets.len() as u64,
    );

    let mut top_tweets = tweets.clone();
    twitter::sort_by(&mut top_tweets, "likes");
    top_tweets.truncate(20);

    // 2. Account-specific searches
    let accounts: Vec<String> = args
        .accounts
        .as_ref()
        .map(|a| a.split(',').map(|s| s.trim().trim_start_matches('@').to_string()).collect())
        .unwrap_or_default();

    let mut account_sections = Vec::new();
    for acct in &accounts {
        eprintln!("  Checking @{}...", acct);
        match twitter::get_profile(client, token, acct, 10, false).await {
            Ok((_user, user_tweets)) => {
                costs::track_cost(
                    &config.costs_path(),
                    "profile",
                    &format!("/2/users/by/username/{}", acct),
                    user_tweets.len() as u64 + 1,
                );
                account_sections.push((acct.clone(), user_tweets));
            }
            Err(e) => {
                eprintln!("  Warning: @{}: {}", acct, e);
            }
        }
    }

    // 3. Sentiment analysis
    let mut sentiment_section = String::new();
    if args.sentiment && !top_tweets.is_empty() {
        if let Ok(api_key) = config.require_xai_key() {
            eprintln!("  Running sentiment analysis...");
            let http = reqwest::Client::new();
            match sentiment::analyze_sentiment(&http, api_key, &top_tweets, Some(&args.model)).await
            {
                Ok(sentiments) => {
                    let stats = sentiment::compute_stats(&sentiments);
                    let total = top_tweets.len();
                    let pct = |n: u32| -> u32 {
                        if total > 0 {
                            (n as f64 / total as f64 * 100.0).round() as u32
                        } else {
                            0
                        }
                    };

                    sentiment_section.push_str("## Sentiment Analysis\n\n");
                    sentiment_section.push_str(&format!(
                        "**Average score:** {:.2}/1.0\n",
                        stats.average_score
                    ));
                    sentiment_section
                        .push_str("| Sentiment | Count | Pct |\n|-----------|-------|-----|\n");
                    sentiment_section.push_str(&format!(
                        "| Positive | {} | {}% |\n",
                        stats.positive,
                        pct(stats.positive)
                    ));
                    sentiment_section.push_str(&format!(
                        "| Negative | {} | {}% |\n",
                        stats.negative,
                        pct(stats.negative)
                    ));
                    sentiment_section.push_str(&format!(
                        "| Neutral | {} | {}% |\n",
                        stats.neutral,
                        pct(stats.neutral)
                    ));
                    if stats.mixed > 0 {
                        sentiment_section.push_str(&format!(
                            "| Mixed | {} | {}% |\n",
                            stats.mixed,
                            pct(stats.mixed)
                        ));
                    }
                    sentiment_section.push('\n');
                }
                Err(e) => {
                    sentiment_section
                        .push_str(&format!("## Sentiment Analysis\n\n*Analysis failed: {}*\n\n", e));
                }
            }
        }
    }

    // 4. AI Summary
    eprintln!("  Generating AI summary...");
    let ai_summary;
    if let Ok(api_key) = config.require_xai_key() {
        let http = reqwest::Client::new();
        let tweet_context = grok::format_tweets_for_context(
            &top_tweets[..top_tweets.len().min(15)],
        );

        let prompt = format!(
            "Based on these {} tweets about \"{}\", provide:\n\
             1. A 2-3 sentence executive summary of the current conversation\n\
             2. Key themes (3-5 bullet points)\n\
             3. Notable signals or trends\n\
             4. Any emerging narratives or sentiment shifts\n\n\
             Be concise and actionable.",
            top_tweets.len(),
            query
        );

        let opts = GrokOpts {
            model: args.model.clone(),
            ..Default::default()
        };

        match grok::analyze_query(&http, api_key, &prompt, Some(&tweet_context), &opts).await {
            Ok(response) => ai_summary = response.content,
            Err(e) => ai_summary = format!("*AI summary unavailable: {}*", e),
        }
    } else {
        ai_summary = "*AI summary unavailable: XAI_API_KEY not set*".to_string();
    }

    // Build report
    let mut report = format!("# Intelligence Report: {}\n\n", query);
    report.push_str(&format!("**Generated:** {}\n", time));
    report.push_str(&format!("**Tweets analyzed:** {}\n", tweets.len()));
    if !accounts.is_empty() {
        report.push_str(&format!(
            "**Tracked accounts:** {}\n",
            accounts.iter().map(|a| format!("@{}", a)).collect::<Vec<_>>().join(", ")
        ));
    }
    report.push_str("\n---\n\n");

    report.push_str(&format!("## Executive Summary\n\n{}\n\n", ai_summary));

    if !sentiment_section.is_empty() {
        report.push_str(&sentiment_section);
    }

    report.push_str("## Top Tweets (by engagement)\n\n");
    for t in top_tweets.iter().take(10) {
        report.push_str(&format::format_tweet_markdown(t));
        report.push_str("\n\n");
    }

    for (username, user_tweets) in &account_sections {
        if !user_tweets.is_empty() {
            report.push_str(&format!("## @{} — Recent Activity\n\n", username));
            for t in user_tweets.iter().take(5) {
                report.push_str(&format::format_tweet_markdown(t));
                report.push_str("\n\n");
            }
        }
    }

    report.push_str("---\n\n## Report Metadata\n\n");
    report.push_str(&format!("- **Query:** {}\n", query));
    report.push_str(&format!("- **Date:** {}\n", date));
    report.push_str(&format!("- **Tweets scanned:** {}\n", tweets.len()));
    report.push_str(&format!(
        "- **Est. cost:** ~${:.2} (search) + Grok API\n",
        tweets.len() as f64 * 0.005
    ));
    report.push_str("- **Generated by:** [xint](https://github.com/0xNyk/xint)\n");

    println!("{}", report);

    if args.save {
        let exports_dir = config.exports_dir();
        fs::create_dir_all(&exports_dir)?;
        let slug = query
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ')
            .collect::<String>()
            .replace(' ', "-")
            .to_lowercase();
        let slug = &slug[..slug.len().min(40)];
        let path = exports_dir.join(format!("report-{}-{}.md", slug, date));
        fs::write(&path, &report)?;
        eprintln!("\nSaved to {}", path.display());
    }

    Ok(())
}
