use crate::models::Tweet;
use chrono::Utc;
use serde::Serialize;
use serde_json::{Map, Value};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn compact_number(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn time_ago(date_str: &str) -> String {
    let Ok(date) = chrono::DateTime::parse_from_rfc3339(date_str) else {
        return String::new();
    };
    let diff = Utc::now().signed_duration_since(date);
    let mins = diff.num_minutes();
    if mins < 60 {
        return format!("{mins}m");
    }
    let hours = diff.num_hours();
    if hours < 24 {
        return format!("{hours}h");
    }
    let days = diff.num_days();
    format!("{days}d")
}

fn clean_tco(text: &str) -> String {
    let re_like = |s: &str| -> String {
        let mut result = String::new();
        let mut rest = s;
        while let Some(pos) = rest.find("https://t.co/") {
            result.push_str(&rest[..pos]);
            rest = &rest[pos..];
            // Skip until whitespace or end
            match rest.find(|c: char| c.is_whitespace()) {
                Some(end) => rest = &rest[end..],
                None => {
                    rest = "";
                    break;
                }
            }
        }
        result.push_str(rest);
        result.trim().to_string()
    };
    re_like(text)
}

// ---------------------------------------------------------------------------
// Telegram / terminal format
// ---------------------------------------------------------------------------

pub fn format_tweet_terminal(t: &Tweet, index: Option<usize>, full: bool) -> String {
    let prefix = match index {
        Some(i) => format!("{}. ", i + 1),
        None => String::new(),
    };
    let engagement = format!(
        "{}\u{2764}\u{fe0f} {}\u{1f441}",
        compact_number(t.metrics.likes),
        compact_number(t.metrics.impressions)
    );
    let time = time_ago(&t.created_at);

    let text = if full || t.text.len() <= 200 {
        t.text.clone()
    } else {
        format!(
            "{}...",
            &t.text[..t
                .text
                .char_indices()
                .nth(197)
                .map(|(i, _)| i)
                .unwrap_or(t.text.len())]
        )
    };
    let clean_text = clean_tco(&text);

    let mut out = format!(
        "{}@{} ({} \u{00b7} {})\n{}",
        prefix, t.username, engagement, time, clean_text
    );

    if let Some(u) = t.urls.first() {
        if let Some(ref title) = u.title {
            out.push_str(&format!("\n\u{1f4f0} \"{title}\""));
            if let Some(ref desc) = u.description {
                let short = if desc.len() > 120 {
                    &desc[..120]
                } else {
                    desc.as_str()
                };
                out.push_str(&format!(" \u{2014} {short}"));
            }
        }
        out.push_str(&format!("\n\u{1f517} {}", u.url));
    }
    out.push_str(&format!("\n{}", t.tweet_url));
    out
}

pub fn format_results_terminal(tweets: &[Tweet], query: Option<&str>, limit: usize) -> String {
    let shown = &tweets[..tweets.len().min(limit)];
    let mut out = String::new();

    if let Some(q) = query {
        out.push_str(&format!(
            "\u{1f50d} \"{}\" \u{2014} {} results\n\n",
            q,
            tweets.len()
        ));
    }

    for (i, t) in shown.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        out.push_str(&format_tweet_terminal(t, Some(i), false));
    }

    if tweets.len() > limit {
        out.push_str(&format!("\n\n... +{} more", tweets.len() - limit));
    }

    out
}

// ---------------------------------------------------------------------------
// Profile
// ---------------------------------------------------------------------------

pub fn format_profile_terminal(user: &serde_json::Value, tweets: &[Tweet]) -> String {
    let username = user.get("username").and_then(|v| v.as_str()).unwrap_or("?");
    let name = user.get("name").and_then(|v| v.as_str()).unwrap_or("?");
    let desc = user
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let pm = user.get("public_metrics");
    let followers = pm
        .and_then(|m| m.get("followers_count"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let tweet_count = pm
        .and_then(|m| m.get("tweet_count"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let mut out = format!(
        "\u{1f464} @{} \u{2014} {}\n{} followers \u{00b7} {} tweets\n",
        username,
        name,
        compact_number(followers),
        compact_number(tweet_count)
    );

    if !desc.is_empty() {
        let short_desc = if desc.len() > 150 { &desc[..150] } else { desc };
        out.push_str(short_desc);
        out.push('\n');
    }

    out.push_str("\nRecent:\n\n");

    for (i, t) in tweets.iter().take(10).enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        out.push_str(&format_tweet_terminal(t, Some(i), false));
    }

    out
}

// ---------------------------------------------------------------------------
// Markdown
// ---------------------------------------------------------------------------

pub fn format_tweet_markdown(t: &Tweet) -> String {
    let engagement = format!("{}L {}I", t.metrics.likes, t.metrics.impressions);
    let clean_text = clean_tco(&t.text);
    let quoted = clean_text.replace('\n', "\n  > ");

    let mut out = format!(
        "- **@{}** ({}) [Tweet]({})\n  > {}",
        t.username, engagement, t.tweet_url, quoted
    );

    if !t.urls.is_empty() {
        let links: Vec<String> = t
            .urls
            .iter()
            .map(|u| {
                let display = match &u.title {
                    Some(t) => t.clone(),
                    None => url::Url::parse(&u.url)
                        .ok()
                        .and_then(|p| p.host_str().map(String::from))
                        .unwrap_or_else(|| "link".to_string()),
                };
                format!("[{}]({})", display, u.url)
            })
            .collect();
        out.push_str(&format!("\n  Links: {}", links.join(", ")));
    }

    out
}

pub fn format_research_markdown(query: &str, tweets: &[Tweet], queries: &[&str]) -> String {
    let date = Utc::now().format("%Y-%m-%d").to_string();
    let mut out = format!("# X Research: {query}\n\n");
    out.push_str(&format!("**Date:** {date}\n"));
    out.push_str(&format!("**Tweets found:** {}\n\n", tweets.len()));

    out.push_str("## Top Results (by engagement)\n\n");
    for t in tweets.iter().take(30) {
        out.push_str(&format_tweet_markdown(t));
        out.push_str("\n\n");
    }

    out.push_str("---\n\n## Research Metadata\n");
    out.push_str(&format!("- **Query:** {query}\n"));
    out.push_str(&format!("- **Date:** {date}\n"));
    out.push_str(&format!("- **Tweets scanned:** {}\n", tweets.len()));
    out.push_str(&format!(
        "- **Est. cost:** ~${:.2}\n",
        tweets.len() as f64 * 0.005
    ));
    if !queries.is_empty() {
        out.push_str("- **Search queries:**\n");
        for q in queries {
            out.push_str(&format!("  - `{q}`\n"));
        }
    }

    out
}

// ---------------------------------------------------------------------------
// CSV
// ---------------------------------------------------------------------------

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn format_csv(tweets: &[Tweet]) -> String {
    let header = "id,username,name,text,likes,retweets,replies,impressions,bookmarks,created_at,url,hashtags,mentions";
    let rows: Vec<String> = tweets
        .iter()
        .map(|t| {
            let clean_text = clean_tco(&t.text);
            format!(
                "{},{},{},{},{},{},{},{},{},{},{},{},{}",
                t.id,
                csv_escape(&t.username),
                csv_escape(&t.name),
                csv_escape(&clean_text),
                t.metrics.likes,
                t.metrics.retweets,
                t.metrics.replies,
                t.metrics.impressions,
                t.metrics.bookmarks,
                t.created_at,
                t.tweet_url,
                csv_escape(&t.hashtags.join(";")),
                csv_escape(&t.mentions.join(";")),
            )
        })
        .collect();

    std::iter::once(header.to_string())
        .chain(rows)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn active_fields() -> Option<String> {
    std::env::var("XINT_FIELDS")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn parse_field_paths(fields: &str) -> Vec<Vec<String>> {
    fields
        .split(',')
        .map(|field| field.trim())
        .filter(|field| !field.is_empty())
        .map(|field| field.split('.').map(|p| p.trim().to_string()).collect())
        .filter(|parts: &Vec<String>| !parts.is_empty() && parts.iter().all(|p| !p.is_empty()))
        .collect()
}

fn extract_path(value: &Value, path: &[String]) -> Option<Value> {
    if path.is_empty() {
        return Some(value.clone());
    }
    match value {
        Value::Object(map) => {
            let head = &path[0];
            let next = map.get(head)?;
            extract_path(next, &path[1..])
        }
        _ => None,
    }
}

fn insert_path(target: &mut Map<String, Value>, path: &[String], value: Value) {
    if path.is_empty() {
        return;
    }
    if path.len() == 1 {
        target.insert(path[0].clone(), value);
        return;
    }

    let key = path[0].clone();
    let entry = target
        .entry(key)
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    if let Value::Object(obj) = entry {
        insert_path(obj, &path[1..], value);
    }
}

fn filter_object(value: &Value, paths: &[Vec<String>]) -> Value {
    let mut out = Map::new();
    for path in paths {
        if let Some(found) = extract_path(value, path) {
            insert_path(&mut out, path, found);
        }
    }
    Value::Object(out)
}

pub fn filter_fields(value: &Value, fields: &str) -> Value {
    let paths = parse_field_paths(fields);
    if paths.is_empty() {
        return value.clone();
    }

    match value {
        Value::Object(_) => filter_object(value, &paths),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| {
                    if item.is_object() {
                        filter_object(item, &paths)
                    } else {
                        item.clone()
                    }
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

pub fn print_json_pretty_filtered<T: Serialize>(value: &T) -> anyhow::Result<()> {
    let mut json = serde_json::to_value(value)?;
    if let Some(fields) = active_fields() {
        json = filter_fields(&json, &fields);
    }
    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}
