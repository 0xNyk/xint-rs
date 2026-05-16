//! Cost preview for `--dry-run`. Mirrors xint/lib/dryrun.ts so the CLI
//! behaves identically across the two binaries.
//!
//! Every cost-bearing command can opt in by checking `args.dry_run` early
//! and calling `preview_and_exit()` before any API call.

use crate::costs;

pub struct DryRunEstimate<'a> {
    pub command: &'a str,
    pub endpoint: &'a str,
    pub units: u64,
    pub unit_label: &'a str,
    pub cost_usd: f64,
    pub cache_predicted_hit: Option<bool>,
    pub cache_ttl_minutes: Option<u64>,
    pub notes: &'a [&'a str],
}

fn fmt_usd(n: f64) -> String {
    if n < 0.0001 {
        "<$0.0001".to_string()
    } else if n < 0.01 {
        format!("~${n:.4}")
    } else {
        format!("~${n:.2}")
    }
}

/// Estimate cost for an operation using the same rate table as track_cost.
/// Note: costs::cost_rate returns (per_tweet, per_call) — order matters.
pub fn estimate_cost(operation: &str, units: u64, per_call_units: u64) -> f64 {
    let (per_tweet, per_call) = costs::cost_rate(operation);
    per_call * per_call_units as f64 + per_tweet * units as f64
}

/// Print a structured preview and exit 0. Stdout so agents can capture.
pub fn preview_and_exit(est: &DryRunEstimate) -> ! {
    println!("\n=== DRY RUN ===");
    println!("Command:       {}", est.command);
    println!("Endpoint:      {}", est.endpoint);
    println!("Estimated:     {} {}", est.units, est.unit_label);
    println!("Estimated cost: {}", fmt_usd(est.cost_usd));
    if let Some(hit) = est.cache_predicted_hit {
        let ttl = est.cache_ttl_minutes.unwrap_or(0);
        if hit {
            println!("Cache:         likely HIT ({ttl}m TTL) — actual cost may be $0");
        } else {
            println!("Cache:         miss expected ({ttl}m TTL)");
        }
    }
    if !est.notes.is_empty() {
        println!("Notes:");
        for n in est.notes {
            println!("  • {n}");
        }
    }
    println!("\nRe-run without --dry-run to execute.\n");
    std::process::exit(0);
}
