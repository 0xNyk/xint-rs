use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;

use crate::api::twitter;
use crate::auth::oauth;
use crate::cli::TimingArgs;
use crate::client::XClient;
use crate::config::Config;
use crate::costs;
use crate::format;
use crate::models::Tweet;

pub async fn run(args: &TimingArgs, config: &Config, client: &XClient) -> Result<()> {
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

    let analysis = analyze_timing(&tweets);

    if args.json {
        format::print_json_pretty_filtered(&analysis)?;
    } else {
        println!(
            "{}",
            format_timing(&analysis, &tokens.username, &args.since)
        );
    }

    Ok(())
}

#[derive(serde::Serialize)]
struct TimingAnalysis {
    total_tweets: usize,
    by_hour: Vec<HourStats>,
    by_day: Vec<DayOfWeekStats>,
    best_windows: Vec<Window>,
    heatmap: Vec<Vec<f64>>,
}

#[derive(serde::Serialize, Clone)]
struct HourStats {
    hour: u32,
    tweets: usize,
    avg_engagement_rate: f64,
    avg_likes: f64,
}

#[derive(serde::Serialize, Clone)]
struct DayOfWeekStats {
    day: String,
    tweets: usize,
    avg_engagement_rate: f64,
}

#[derive(serde::Serialize)]
struct Window {
    day: String,
    hour: u32,
    avg_engagement_rate: f64,
    sample_size: usize,
}

fn engagement_rate(t: &Tweet) -> f64 {
    if t.metrics.impressions == 0 {
        return 0.0;
    }
    let eng = t.metrics.likes + t.metrics.retweets + t.metrics.replies + t.metrics.quotes;
    eng as f64 / t.metrics.impressions as f64 * 100.0
}

fn analyze_timing(tweets: &[Tweet]) -> TimingAnalysis {
    let days_of_week = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

    // Group by hour
    let mut by_hour: HashMap<u32, Vec<&Tweet>> = HashMap::new();
    // Group by day of week
    let mut by_dow: HashMap<u32, Vec<&Tweet>> = HashMap::new();
    // Group by (day, hour) for heatmap
    let mut by_day_hour: HashMap<(u32, u32), Vec<&Tweet>> = HashMap::new();

    for t in tweets {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&t.created_at) {
            let hour = dt.format("%H").to_string().parse::<u32>().unwrap_or(0);
            let dow = dt.format("%u").to_string().parse::<u32>().unwrap_or(1) - 1; // 0=Mon

            by_hour.entry(hour).or_default().push(t);
            by_dow.entry(dow).or_default().push(t);
            by_day_hour.entry((dow, hour)).or_default().push(t);
        }
    }

    // Hour stats
    let mut hour_stats: Vec<HourStats> = (0..24)
        .map(|h| {
            let tweets_at = by_hour.get(&h).map(|v| v.as_slice()).unwrap_or(&[]);
            let n = tweets_at.len();
            let avg_er = if n > 0 {
                tweets_at.iter().map(|t| engagement_rate(t)).sum::<f64>() / n as f64
            } else {
                0.0
            };
            let avg_likes = if n > 0 {
                tweets_at
                    .iter()
                    .map(|t| t.metrics.likes as f64)
                    .sum::<f64>()
                    / n as f64
            } else {
                0.0
            };
            HourStats {
                hour: h,
                tweets: n,
                avg_engagement_rate: (avg_er * 100.0).round() / 100.0,
                avg_likes: (avg_likes * 10.0).round() / 10.0,
            }
        })
        .collect();

    // Day of week stats
    let dow_stats: Vec<DayOfWeekStats> = (0..7)
        .map(|d| {
            let tweets_at = by_dow.get(&d).map(|v| v.as_slice()).unwrap_or(&[]);
            let n = tweets_at.len();
            let avg_er = if n > 0 {
                tweets_at.iter().map(|t| engagement_rate(t)).sum::<f64>() / n as f64
            } else {
                0.0
            };
            DayOfWeekStats {
                day: days_of_week[d as usize].to_string(),
                tweets: n,
                avg_engagement_rate: (avg_er * 100.0).round() / 100.0,
            }
        })
        .collect();

    // Heatmap: 7 rows (days) x 24 cols (hours), values = avg engagement rate
    let heatmap: Vec<Vec<f64>> = (0..7)
        .map(|d| {
            (0..24)
                .map(|h| {
                    let tweets_at = by_day_hour.get(&(d, h));
                    match tweets_at {
                        Some(v) if !v.is_empty() => {
                            let er: f64 =
                                v.iter().map(|t| engagement_rate(t)).sum::<f64>() / v.len() as f64;
                            (er * 100.0).round() / 100.0
                        }
                        _ => 0.0,
                    }
                })
                .collect()
        })
        .collect();

    // Best windows: top 3 (day, hour) combos by avg engagement rate (min 1 tweet)
    let mut windows: Vec<((u32, u32), f64, usize)> = by_day_hour
        .iter()
        .map(|((d, h), tweets_list)| {
            let avg_er = tweets_list.iter().map(|t| engagement_rate(t)).sum::<f64>()
                / tweets_list.len() as f64;
            ((*d, *h), avg_er, tweets_list.len())
        })
        .collect();
    windows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let best_windows: Vec<Window> = windows
        .iter()
        .take(3)
        .map(|((d, h), er, n)| Window {
            day: days_of_week[*d as usize].to_string(),
            hour: *h,
            avg_engagement_rate: (*er * 100.0).round() / 100.0,
            sample_size: *n,
        })
        .collect();

    // Sort hour_stats for display
    hour_stats.sort_by(|a, b| a.hour.cmp(&b.hour));

    TimingAnalysis {
        total_tweets: tweets.len(),
        by_hour: hour_stats,
        by_day: dow_stats,
        best_windows,
        heatmap,
    }
}

fn format_timing(analysis: &TimingAnalysis, username: &str, since: &str) -> String {
    let days_of_week = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let blocks = [
        ' ', '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}',
        '\u{2588}',
    ];

    let mut out = String::new();

    out.push_str(&format!(
        "\n{}  @{} ({} tweets, since {})\n\n",
        "Posting Timing Analysis".bold(),
        username.cyan(),
        analysis.total_tweets,
        since
    ));

    // Best windows
    if !analysis.best_windows.is_empty() {
        out.push_str(&format!(
            "  {}\n",
            "Best Posting Windows".bold().underline()
        ));
        for (i, w) in analysis.best_windows.iter().enumerate() {
            out.push_str(&format!(
                "  {}. {} {:02}:00 UTC  {}% ER  ({} tweets)\n",
                i + 1,
                w.day,
                w.hour,
                format!("{:.2}", w.avg_engagement_rate).green(),
                w.sample_size
            ));
        }
        out.push('\n');
    }

    // Heatmap
    out.push_str(&format!(
        "  {}\n",
        "Engagement Heatmap (UTC)".bold().underline()
    ));
    out.push_str("        ");
    for h in 0..24 {
        if h % 3 == 0 {
            out.push_str(&format!("{:>2} ", h));
        } else {
            out.push_str("   ");
        }
    }
    out.push('\n');

    // Find max ER for scaling
    let max_er = analysis
        .heatmap
        .iter()
        .flat_map(|row| row.iter())
        .cloned()
        .fold(0.0_f64, f64::max);

    for (d, row) in analysis.heatmap.iter().enumerate() {
        out.push_str(&format!("  {:>3}   ", days_of_week[d]));
        for val in row {
            let level = if max_er > 0.0 {
                ((val / max_er) * 8.0).round() as usize
            } else {
                0
            };
            let block = blocks[level.min(8)];
            if level >= 6 {
                out.push_str(&format!("{}", format!(" {} ", block).on_green()));
            } else {
                out.push_str(&format!(" {} ", block));
            }
        }
        out.push('\n');
    }

    // Hourly breakdown
    out.push_str(&format!("\n  {}\n", "Hourly Breakdown".bold().underline()));
    out.push_str(&format!(
        "  {:>5} {:>6} {:>8} {:>10}\n",
        "Hour", "Tweets", "ER%", "Avg Likes"
    ));
    for h in &analysis.by_hour {
        if h.tweets > 0 {
            out.push_str(&format!(
                "  {:>02}:00 {:>6} {:>7}% {:>10.1}\n",
                h.hour,
                h.tweets,
                format!("{:.2}", h.avg_engagement_rate),
                h.avg_likes
            ));
        }
    }

    // Day of week
    out.push_str(&format!("\n  {}\n", "Day of Week".bold().underline()));
    for d in &analysis.by_day {
        if d.tweets > 0 {
            out.push_str(&format!(
                "  {:>3}  {} tweets  {}% ER\n",
                d.day,
                d.tweets.to_string().yellow(),
                format!("{:.2}", d.avg_engagement_rate).green()
            ));
        }
    }

    out
}
