use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;
use std::fs;

use crate::api::twitter;
use crate::auth::oauth;
use crate::cli::AnalyticsArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::format;
use crate::models::Tweet;

pub async fn run(args: &AnalyticsArgs, config: &Config, client: &XClient) -> Result<()> {
    let client_id = config.require_client_id()?;
    let (access_token, tokens) =
        oauth::get_valid_token(client, &config.tokens_path(), client_id).await?;

    eprintln!("Fetching your tweets (since {})...", args.since);

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

    let stats = compute_stats(&tweets);

    if args.json {
        format::print_json_pretty_filtered(&stats)?;
    } else {
        println!(
            "{}",
            format_dashboard(&stats, &tokens.username, &args.since)
        );
    }

    if args.save {
        let exports_dir = config.exports_dir();
        fs::create_dir_all(&exports_dir)?;
        let path = exports_dir.join(format!(
            "analytics-{}-{}.json",
            tokens.username,
            chrono::Utc::now().format("%Y%m%d-%H%M%S")
        ));
        let json = serde_json::to_string_pretty(&stats)?;
        fs::write(&path, &json)?;
        eprintln!("Saved to {}", path.display());
    }

    costs::track_cost(&config.costs_path(), "analytics", "analytics", 0);

    Ok(())
}

#[derive(serde::Serialize)]
struct AnalyticsStats {
    overview: Overview,
    top_performers: Vec<TweetSummary>,
    content_breakdown: ContentBreakdown,
    engagement_trends: Vec<DayStats>,
}

#[derive(serde::Serialize)]
struct Overview {
    total_tweets: usize,
    total_impressions: u64,
    total_likes: u64,
    total_retweets: u64,
    total_replies: u64,
    avg_engagement_rate: f64,
    avg_likes_per_tweet: f64,
    avg_impressions_per_tweet: f64,
}

#[derive(serde::Serialize)]
struct TweetSummary {
    id: String,
    text_preview: String,
    likes: u64,
    impressions: u64,
    engagement_rate: f64,
    url: String,
}

#[derive(serde::Serialize)]
struct ContentBreakdown {
    with_links: usize,
    with_media: usize,
    with_hashtags: usize,
    with_mentions: usize,
    plain_text: usize,
}

#[derive(serde::Serialize)]
struct DayStats {
    date: String,
    tweets: usize,
    impressions: u64,
    likes: u64,
    engagement_rate: f64,
}

fn engagement_rate(t: &Tweet) -> f64 {
    if t.metrics.impressions == 0 {
        return 0.0;
    }
    let engagements = t.metrics.likes + t.metrics.retweets + t.metrics.replies + t.metrics.quotes;
    engagements as f64 / t.metrics.impressions as f64 * 100.0
}

fn compute_stats(tweets: &[Tweet]) -> AnalyticsStats {
    let total_impressions: u64 = tweets.iter().map(|t| t.metrics.impressions).sum();
    let total_likes: u64 = tweets.iter().map(|t| t.metrics.likes).sum();
    let total_retweets: u64 = tweets.iter().map(|t| t.metrics.retweets).sum();
    let total_replies: u64 = tweets.iter().map(|t| t.metrics.replies).sum();
    let total_quotes: u64 = tweets.iter().map(|t| t.metrics.quotes).sum();
    let n = tweets.len() as f64;

    let avg_engagement_rate = if total_impressions > 0 {
        (total_likes + total_retweets + total_replies + total_quotes) as f64
            / total_impressions as f64
            * 100.0
    } else {
        0.0
    };

    let overview = Overview {
        total_tweets: tweets.len(),
        total_impressions,
        total_likes,
        total_retweets,
        total_replies,
        avg_engagement_rate: (avg_engagement_rate * 100.0).round() / 100.0,
        avg_likes_per_tweet: (total_likes as f64 / n * 10.0).round() / 10.0,
        avg_impressions_per_tweet: (total_impressions as f64 / n).round(),
    };

    // Top 5 by engagement rate
    let mut sorted: Vec<&Tweet> = tweets.iter().collect();
    sorted.sort_by(|a, b| {
        engagement_rate(b)
            .partial_cmp(&engagement_rate(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let top_performers: Vec<TweetSummary> = sorted
        .iter()
        .take(5)
        .map(|t| {
            let preview = if t.text.len() > 80 {
                format!(
                    "{}...",
                    &t.text[..t
                        .text
                        .char_indices()
                        .nth(77)
                        .map(|(i, _)| i)
                        .unwrap_or(t.text.len())]
                )
            } else {
                t.text.clone()
            };
            TweetSummary {
                id: t.id.clone(),
                text_preview: preview,
                likes: t.metrics.likes,
                impressions: t.metrics.impressions,
                engagement_rate: (engagement_rate(t) * 100.0).round() / 100.0,
                url: t.tweet_url.clone(),
            }
        })
        .collect();

    // Content breakdown
    let with_links = tweets.iter().filter(|t| !t.urls.is_empty()).count();
    let with_hashtags = tweets.iter().filter(|t| !t.hashtags.is_empty()).count();
    let with_mentions = tweets.iter().filter(|t| !t.mentions.is_empty()).count();
    let with_media = tweets
        .iter()
        .filter(|t| t.text.contains("https://t.co/") && t.urls.is_empty())
        .count();
    let plain_text = tweets.len() - with_links - with_media;

    let content_breakdown = ContentBreakdown {
        with_links,
        with_media,
        with_hashtags,
        with_mentions,
        plain_text,
    };

    // Daily engagement trends
    let mut by_day: HashMap<String, Vec<&Tweet>> = HashMap::new();
    for t in tweets {
        let day = if t.created_at.len() >= 10 {
            t.created_at[..10].to_string()
        } else {
            "unknown".to_string()
        };
        by_day.entry(day).or_default().push(t);
    }
    let mut engagement_trends: Vec<DayStats> = by_day
        .into_iter()
        .map(|(date, day_tweets)| {
            let impressions: u64 = day_tweets.iter().map(|t| t.metrics.impressions).sum();
            let likes: u64 = day_tweets.iter().map(|t| t.metrics.likes).sum();
            let total_eng: u64 = day_tweets
                .iter()
                .map(|t| {
                    t.metrics.likes + t.metrics.retweets + t.metrics.replies + t.metrics.quotes
                })
                .sum();
            let er = if impressions > 0 {
                total_eng as f64 / impressions as f64 * 100.0
            } else {
                0.0
            };
            DayStats {
                date,
                tweets: day_tweets.len(),
                impressions,
                likes,
                engagement_rate: (er * 100.0).round() / 100.0,
            }
        })
        .collect();
    engagement_trends.sort_by(|a, b| b.date.cmp(&a.date));

    AnalyticsStats {
        overview,
        top_performers,
        content_breakdown,
        engagement_trends,
    }
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

fn format_dashboard(stats: &AnalyticsStats, username: &str, since: &str) -> String {
    let o = &stats.overview;
    let mut out = String::new();

    out.push_str(&format!(
        "\n{}  @{} (last {})\n\n",
        "Account Analytics".bold(),
        username.cyan(),
        since
    ));

    // Overview
    out.push_str(&format!("  {} {}\n", "Overview".bold().underline(), ""));
    out.push_str(&format!(
        "  Tweets:       {}\n",
        o.total_tweets.to_string().yellow()
    ));
    out.push_str(&format!(
        "  Impressions:  {}\n",
        compact_number(o.total_impressions).yellow()
    ));
    out.push_str(&format!(
        "  Likes:        {}\n",
        compact_number(o.total_likes).yellow()
    ));
    out.push_str(&format!(
        "  Retweets:     {}\n",
        compact_number(o.total_retweets).yellow()
    ));
    out.push_str(&format!(
        "  Replies:      {}\n",
        compact_number(o.total_replies).yellow()
    ));
    out.push_str(&format!(
        "  Avg ER:       {}%\n",
        format!("{:.2}", o.avg_engagement_rate).green()
    ));
    out.push_str(&format!(
        "  Avg Likes:    {}/tweet\n",
        format!("{:.1}", o.avg_likes_per_tweet).yellow()
    ));

    // Top Performers
    if !stats.top_performers.is_empty() {
        out.push_str(&format!("\n  {}\n", "Top 5 Performers".bold().underline()));
        for (i, t) in stats.top_performers.iter().enumerate() {
            let preview = if t.text_preview.len() > 60 {
                format!("{}...", &t.text_preview[..57])
            } else {
                t.text_preview.clone()
            };
            out.push_str(&format!(
                "  {}. {} | {}% ER | {} likes | {} views\n     {}\n",
                i + 1,
                preview.dimmed(),
                format!("{:.2}", t.engagement_rate).green(),
                compact_number(t.likes),
                compact_number(t.impressions),
                t.url.dimmed()
            ));
        }
    }

    // Content Breakdown
    let cb = &stats.content_breakdown;
    out.push_str(&format!("\n  {}\n", "Content Breakdown".bold().underline()));
    out.push_str(&format!("  With links:    {}\n", cb.with_links));
    out.push_str(&format!("  With media:    {}\n", cb.with_media));
    out.push_str(&format!("  With hashtags: {}\n", cb.with_hashtags));
    out.push_str(&format!("  With mentions: {}\n", cb.with_mentions));
    out.push_str(&format!("  Plain text:    {}\n", cb.plain_text));

    // Daily Trends
    if !stats.engagement_trends.is_empty() {
        out.push_str(&format!("\n  {}\n", "Daily Trends".bold().underline()));
        out.push_str(&format!(
            "  {:<12} {:>6} {:>10} {:>8} {:>8}\n",
            "Date", "Tweets", "Impress.", "Likes", "ER%"
        ));
        for d in &stats.engagement_trends {
            out.push_str(&format!(
                "  {:<12} {:>6} {:>10} {:>8} {:>7}%\n",
                d.date,
                d.tweets,
                compact_number(d.impressions),
                compact_number(d.likes),
                format!("{:.2}", d.engagement_rate)
            ));
        }
    }

    out
}
