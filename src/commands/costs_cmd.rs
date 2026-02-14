use anyhow::Result;

use crate::cli::CostsArgs;
use crate::config::Config;
use crate::costs;

pub fn run(args: &CostsArgs, config: &Config) -> Result<()> {
    let parts: Vec<String> = args
        .subcommand
        .as_ref()
        .map(|v| v.clone())
        .unwrap_or_default();

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
                        .map_err(|_| anyhow::anyhow!("Invalid budget amount: {}", v))?;
                    costs::set_budget(&config.costs_path(), limit);
                    println!("Daily budget set to ${:.2}", limit);
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
        _ => {
            println!("Usage: xint costs [today|week|month|all|budget|reset]");
            println!();
            println!("  today          Show today's costs (default)");
            println!("  week           Show last 7 days");
            println!("  month          Show last 30 days");
            println!("  all            Show all-time costs");
            println!("  budget [amt]   View or set daily budget");
            println!("  reset          Reset today's tracking");
        }
    }

    Ok(())
}
