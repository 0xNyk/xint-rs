use crate::models::*;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

const RETENTION_DAYS: i64 = 30;

pub fn cost_rate(operation: &str) -> (f64, f64) {
    // (per_tweet, per_call)
    match operation {
        "search" => (0.005, 0.0),
        "search_archive" => (0.01, 0.0),
        "bookmarks" => (0.005, 0.0),
        "likes" => (0.005, 0.0),
        "like" | "unlike" | "follow" | "unfollow" => (0.0, 0.01),
        "following" => (0.0, 0.005),
        "media_metadata" => (0.005, 0.0),
        "stream_connect" => (0.005, 0.0),
        "stream_rules_list" | "stream_rules_add" | "stream_rules_delete" => (0.0, 0.01),
        "bookmark_save" | "bookmark_remove" => (0.0, 0.01),
        "profile" => (0.005, 0.0),
        "tweet" => (0.005, 0.0),
        "trends" => (0.0, 0.10),
        "thread" => (0.005, 0.0),
        "followers" | "following_list" => (0.0, 0.01),
        "lists_list" | "lists_create" | "lists_update" | "lists_delete" => (0.0, 0.01),
        "list_members_list" | "list_members_add" | "list_members_remove" => (0.0, 0.01),
        "blocks_list" | "blocks_add" | "blocks_remove" => (0.0, 0.01),
        "mutes_list" | "mutes_add" | "mutes_remove" => (0.0, 0.01),
        _ => (0.005, 0.0),
    }
}

fn load_data(path: &Path) -> CostData {
    if !path.exists() {
        return CostData::default();
    }
    match fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => CostData::default(),
    }
}

fn save_data(path: &Path, data: &CostData) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    if let Ok(json) = serde_json::to_string_pretty(data) {
        if fs::write(&tmp, &json).is_ok() {
            let _ = fs::rename(&tmp, path);
        }
    }
}

fn today_str() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

fn prune_entries(data: &mut CostData) {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(RETENTION_DAYS)).to_rfc3339();
    let cutoff_day = &cutoff[..10];

    data.entries
        .retain(|e| e.timestamp.as_str() >= cutoff.as_str());
    data.daily.retain(|d| d.date.as_str() >= cutoff_day);
}

/// Track a cost entry for an API call.
pub fn track_cost(
    costs_path: &Path,
    operation: &str,
    endpoint: &str,
    tweets_read: u64,
) -> CostEntry {
    let (per_tweet, per_call) = cost_rate(operation);
    let cost_usd = per_call + per_tweet * tweets_read as f64;
    let cost_usd = (cost_usd * 1e6).round() / 1e6;

    let entry = CostEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        operation: operation.to_string(),
        endpoint: endpoint.to_string(),
        tweets_read,
        cost_usd,
    };

    let mut data = load_data(costs_path);
    data.entries.push(entry.clone());
    data.total_lifetime_usd = ((data.total_lifetime_usd + cost_usd) * 1e6).round() / 1e6;

    let day = &entry.timestamp[..10];
    let agg = data.daily.iter_mut().find(|d| d.date == day);

    if let Some(agg) = agg {
        agg.total_cost += cost_usd;
        agg.calls += 1;
        agg.tweets_read += tweets_read;
        let op = agg
            .by_operation
            .entry(operation.to_string())
            .or_insert(OperationStats {
                calls: 0,
                cost: 0.0,
                tweets: 0,
            });
        op.calls += 1;
        op.cost += cost_usd;
        op.tweets += tweets_read;
    } else {
        let mut by_operation = HashMap::new();
        by_operation.insert(
            operation.to_string(),
            OperationStats {
                calls: 1,
                cost: cost_usd,
                tweets: tweets_read,
            },
        );
        data.daily.push(DailyAggregate {
            date: day.to_string(),
            total_cost: cost_usd,
            calls: 1,
            tweets_read,
            by_operation,
        });
        data.daily.sort_by(|a, b| a.date.cmp(&b.date));
    }

    prune_entries(&mut data);
    save_data(costs_path, &data);

    entry
}

/// Check if today's spend is within budget.
pub fn check_budget(costs_path: &Path) -> BudgetStatus {
    let data = load_data(costs_path);
    let today = today_str();
    let today_agg = data.daily.iter().find(|d| d.date == today);
    let spent = today_agg.map(|a| a.total_cost).unwrap_or(0.0);
    let limit = data.budget.daily_limit_usd;
    let remaining = (limit - spent).max(0.0);
    let warning = data.budget.enabled && spent >= limit * data.budget.warn_threshold;
    let allowed = !data.budget.enabled || spent < limit;

    BudgetStatus {
        allowed,
        spent: (spent * 1e4).round() / 1e4,
        limit,
        remaining: (remaining * 1e4).round() / 1e4,
        warning,
    }
}

/// Set the daily budget limit.
pub fn set_budget(costs_path: &Path, limit_usd: f64) {
    let mut data = load_data(costs_path);
    data.budget.daily_limit_usd = limit_usd;
    save_data(costs_path, &data);
}

/// Reset today's cost data.
pub fn reset_today(costs_path: &Path) {
    let mut data = load_data(costs_path);
    let today = today_str();
    data.entries.retain(|e| e.timestamp[..10] != today);
    data.daily.retain(|d| d.date != today);
    save_data(costs_path, &data);
}

/// Return today's aggregate costs.
pub fn today_costs(costs_path: &Path) -> DailyAggregate {
    let data = load_data(costs_path);
    let today = today_str();
    data.daily
        .iter()
        .find(|d| d.date == today)
        .cloned()
        .unwrap_or_else(|| DailyAggregate {
            date: today,
            total_cost: 0.0,
            calls: 0,
            tweets_read: 0,
            by_operation: HashMap::new(),
        })
}

/// Get cost summary for a period.
pub fn get_cost_summary(costs_path: &Path, period: &str) -> String {
    let data = load_data(costs_path);
    let today = today_str();

    if period == "today" {
        let agg = data
            .daily
            .iter()
            .find(|d| d.date == today)
            .cloned()
            .unwrap_or_else(|| DailyAggregate {
                date: today.clone(),
                total_cost: 0.0,
                calls: 0,
                tweets_read: 0,
                by_operation: HashMap::new(),
            });

        let pct = if data.budget.daily_limit_usd > 0.0 {
            (agg.total_cost / data.budget.daily_limit_usd * 100.0).round() as u32
        } else {
            0
        };

        let mut out = format!(
            "\u{1f4ca} API Costs \u{2014} Today ({})\n\n  Total: ${:.2} / ${:.2} daily limit\n  Calls: {} | Tweets read: {}\n",
            today, agg.total_cost, data.budget.daily_limit_usd, agg.calls, agg.tweets_read
        );

        if !agg.by_operation.is_empty() {
            out.push_str("\n  By operation:\n");
            let mut ops: Vec<_> = agg.by_operation.iter().collect();
            ops.sort_by(|a, b| b.1.cost.total_cmp(&a.1.cost));
            for (op, stats) in ops {
                out.push_str(&format!(
                    "    {:<16} {:>3} calls, {:>5} tweets, ${:>6.2}\n",
                    format!("{}:", op),
                    stats.calls,
                    stats.tweets,
                    stats.cost
                ));
            }
        }

        out.push_str(&format!(
            "\n  Budget: {}% used ({}% remaining)",
            pct,
            100u32.saturating_sub(pct)
        ));
        return out;
    }

    let (start_date, label) = match period {
        "week" => {
            let d = chrono::Utc::now() - chrono::Duration::days(6);
            (d.format("%Y-%m-%d").to_string(), "Last 7 Days")
        }
        "month" => {
            let d = chrono::Utc::now() - chrono::Duration::days(29);
            (d.format("%Y-%m-%d").to_string(), "Last 30 Days")
        }
        _ => (String::new(), "All Time"),
    };

    let days: Vec<_> = data
        .daily
        .iter()
        .filter(|d| d.date.as_str() >= start_date.as_str())
        .cloned()
        .collect();

    let total_cost: f64 = days.iter().map(|d| d.total_cost).sum();
    let total_calls: u64 = days.iter().map(|d| d.calls).sum();
    let total_tweets: u64 = days.iter().map(|d| d.tweets_read).sum();

    let mut out = format!(
        "\u{1f4ca} API Costs \u{2014} {label}\n\n  Total: ${total_cost:.2} | Calls: {total_calls} | Tweets read: {total_tweets}\n"
    );

    if period == "all" {
        out.push_str(&format!(
            "  Lifetime total: ${:.2}\n",
            data.total_lifetime_usd
        ));
    }

    if !days.is_empty() {
        out.push_str(&format!(
            "\n  Daily breakdown:\n    {:<12} {:>8} {:>6} {:>7}\n    {:<12} {:>8} {:>6} {:>7}\n",
            "Date", "Cost", "Calls", "Tweets", "----", "----", "-----", "------"
        ));
        let mut sorted = days;
        sorted.sort_by(|a, b| b.date.cmp(&a.date));
        for d in &sorted {
            out.push_str(&format!(
                "    {:<12} {:>8} {:>6} {:>7}\n",
                d.date,
                format!("${:.2}", d.total_cost),
                d.calls,
                d.tweets_read
            ));
        }
    } else {
        out.push_str("\n  No data for this period.\n");
    }

    out
}
