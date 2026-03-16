use anyhow::Result;
use colored::Colorize;
use std::fs;

use crate::cli::GrowthArgs;
use crate::config::Config;
use crate::format;
use crate::models::Snapshot;

pub async fn run(args: &GrowthArgs, config: &Config) -> Result<()> {
    let snapshots_dir = config.snapshots_dir();

    if !snapshots_dir.exists() {
        println!(
            "No snapshots found. Run 'xint diff @username' first to create follower snapshots."
        );
        return Ok(());
    }

    // Find all follower snapshots
    let mut snapshots = load_all_snapshots(&snapshots_dir, "followers")?;

    if snapshots.is_empty() {
        println!("No follower snapshots found. Run 'xint diff @username' to start tracking.");
        return Ok(());
    }

    snapshots.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

    if args.json {
        let data = build_growth_data(&snapshots);
        format::print_json_pretty_filtered(&data)?;
        return Ok(());
    }

    let username = &snapshots[0].username;

    if args.history {
        println!(
            "\n{}  @{}\n",
            "Follower Snapshot History".bold(),
            username.cyan()
        );
        for snap in &snapshots {
            let date = if snap.timestamp.len() >= 10 {
                &snap.timestamp[..10]
            } else {
                &snap.timestamp
            };
            println!("  {}  {} followers", date, snap.count.to_string().yellow());
        }
        return Ok(());
    }

    // Default: show growth summary with velocity
    let data = build_growth_data(&snapshots);

    println!("\n{}  @{}\n", "Follower Growth".bold(), username.cyan());

    if let Some(ref vel) = data.velocity {
        println!(
            "  Current:    {} followers",
            data.current_count.to_string().yellow()
        );
        println!("  Period:     {} days", vel.days);
        println!(
            "  Change:     {} ({})",
            format_signed(vel.net_change),
            format_signed_f64(vel.per_day, "/day")
        );
        println!("  Trend:      {}", format_trend(&vel.trend));

        if vel.per_day > 0.0 {
            let milestones = [
                1_000, 5_000, 10_000, 25_000, 50_000, 100_000, 500_000, 1_000_000,
            ];
            let current = data.current_count as f64;
            for m in milestones {
                if m as f64 > current {
                    let days_to = ((m as f64 - current) / vel.per_day).ceil() as u64;
                    println!(
                        "  Next milestone: {} in ~{} days",
                        compact_number(m).green(),
                        days_to
                    );
                    break;
                }
            }
        }
    } else {
        println!(
            "  Current: {} followers",
            data.current_count.to_string().yellow()
        );
        println!("  Only 1 snapshot available. Run 'xint diff @username' again later for trends.");
    }

    // Show recent history
    if snapshots.len() > 1 {
        println!("\n  {}", "Recent History".bold().underline());
        for snap in snapshots.iter().rev().take(10) {
            let date = if snap.timestamp.len() >= 10 {
                &snap.timestamp[..10]
            } else {
                &snap.timestamp
            };
            println!("  {}  {}", date, snap.count);
        }
    }

    Ok(())
}

#[derive(serde::Serialize)]
struct GrowthData {
    username: String,
    current_count: usize,
    snapshots: usize,
    velocity: Option<Velocity>,
}

#[derive(serde::Serialize)]
struct Velocity {
    net_change: i64,
    days: i64,
    per_day: f64,
    trend: String,
}

fn build_growth_data(snapshots: &[Snapshot]) -> GrowthData {
    let current = snapshots.last().unwrap();
    let username = current.username.clone();
    let current_count = current.count;

    let velocity = if snapshots.len() >= 2 {
        let first = &snapshots[0];
        let last = snapshots.last().unwrap();

        let days = {
            let start = chrono::DateTime::parse_from_rfc3339(&first.timestamp).ok();
            let end = chrono::DateTime::parse_from_rfc3339(&last.timestamp).ok();
            match (start, end) {
                (Some(s), Some(e)) => (e - s).num_days().max(1),
                _ => 1,
            }
        };

        let net_change = last.count as i64 - first.count as i64;
        let per_day = net_change as f64 / days as f64;

        let trend = if per_day > 1.0 {
            "growing".to_string()
        } else if per_day > 0.0 {
            "slow_growth".to_string()
        } else if per_day == 0.0 {
            "stable".to_string()
        } else {
            "declining".to_string()
        };

        Some(Velocity {
            net_change,
            days,
            per_day: (per_day * 100.0).round() / 100.0,
            trend,
        })
    } else {
        None
    };

    GrowthData {
        username,
        current_count,
        snapshots: snapshots.len(),
        velocity,
    }
}

fn load_all_snapshots(dir: &std::path::Path, snap_type: &str) -> Result<Vec<Snapshot>> {
    let mut snapshots = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(snapshots),
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.contains(&format!("-{snap_type}-")) && name.ends_with(".json") {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                if let Ok(snap) = serde_json::from_str::<Snapshot>(&content) {
                    if snap.snap_type == snap_type {
                        snapshots.push(snap);
                    }
                }
            }
        }
    }

    Ok(snapshots)
}

fn format_signed(n: i64) -> String {
    if n >= 0 {
        format!("+{}", n).green().to_string()
    } else {
        format!("{}", n).red().to_string()
    }
}

fn format_signed_f64(n: f64, suffix: &str) -> String {
    if n >= 0.0 {
        format!("+{:.1}{}", n, suffix).green().to_string()
    } else {
        format!("{:.1}{}", n, suffix).red().to_string()
    }
}

fn format_trend(trend: &str) -> String {
    match trend {
        "growing" => "Growing".green().to_string(),
        "slow_growth" => "Slow growth".yellow().to_string(),
        "stable" => "Stable".dimmed().to_string(),
        "declining" => "Declining".red().to_string(),
        _ => trend.to_string(),
    }
}

fn compact_number(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.0}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
