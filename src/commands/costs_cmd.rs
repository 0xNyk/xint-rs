use anyhow::Result;

use crate::cli::CostsArgs;
use crate::config::Config;
use crate::costs;

pub fn run(args: &CostsArgs, config: &Config) -> Result<()> {
    let parts: Vec<String> = args.subcommand.clone().unwrap_or_default();

    let sub = parts.first().map(|s| s.as_str()).unwrap_or("today");

    match sub {
        "today" | "t" => {
            println!("{}", costs::get_cost_summary(&config.costs_path(), "today"));
        }
        "week" | "w" | "7d" => {
            println!("{}", costs::get_cost_summary(&config.costs_path(), "week"));
        }
        "month" | "m" | "30d" => {
            println!("{}", costs::get_cost_summary(&config.costs_path(), "month"));
        }
        "all" | "a" => {
            println!("{}", costs::get_cost_summary(&config.costs_path(), "all"));
        }
        "budget" => {
            let limit_str = parts.get(1).map(|s| s.as_str());
            match limit_str {
                Some(v) => {
                    let limit: f64 = v
                        .trim_start_matches('$')
                        .parse()
                        .map_err(|_| anyhow::anyhow!("Invalid budget amount: {v}"))?;
                    costs::set_budget(&config.costs_path(), limit);
                    println!("Daily budget set to ${limit:.2}");
                }
                None => {
                    let status = costs::check_budget(&config.costs_path());
                    println!("Daily budget: ${:.2}", status.limit);
                    println!("Spent today:  ${:.4}", status.spent);
                    println!("Remaining:    ${:.4}", status.remaining);
                    if status.warning {
                        println!("\nWarning: approaching budget limit!");
                    }
                }
            }
        }
        "reset" => {
            costs::reset_today(&config.costs_path());
            println!("Today's cost data has been reset.");
        }
        "forecast" | "--forecast" => {
            let f = costs::forecast_month(&config.costs_path());
            if parts.iter().any(|p| p == "--json") {
                println!("{}", serde_json::to_string_pretty(&f)?);
                return Ok(());
            }
            println!("\n\u{1F4C8} Cost forecast — month-to-date and projection\n");
            println!(
                "  Spent so far:  ${:.2} ({} of {} days)",
                f.mtd_usd, f.days_elapsed, f.days_in_month
            );
            println!("  Projected:     ${:.2} by end of month", f.projected_usd);
            let method_label = if f.method == "mtd_extrapolation" {
                "MTD × (days_in_month / days_elapsed)"
            } else {
                "7-day trailing average × days_in_month (MTD too short)"
            };
            println!("  Method:        {method_label}");
            let confidence_note = if f.confidence == "low" {
                " (≤2 days of data — re-check after a few more days)"
            } else {
                ""
            };
            println!("  Confidence:    {}{confidence_note}", f.confidence);
            if !f.top_operations.is_empty() {
                println!("\n  Top operations this month:");
                for op in &f.top_operations {
                    let share_pct = (op.share * 100.0).round() as u32;
                    println!(
                        "    {:<20} ${:>7.2} ({share_pct}%)",
                        format!("{}:", op.operation),
                        op.cost
                    );
                }
            }
            println!();
        }
        _ => {
            println!("Usage: xint costs [today|week|month|all|forecast|budget|reset]");
            println!();
            println!("  today          Show today's costs (default)");
            println!("  week           Show last 7 days");
            println!("  month          Show last 30 days");
            println!("  all            Show all-time costs");
            println!("  forecast       Project end-of-month spend from MTD burn");
            println!("  budget [amt]   View or set daily budget");
            println!("  reset          Reset today's tracking");
        }
    }

    Ok(())
}
