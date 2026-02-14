use anyhow::Result;

use crate::cli::CacheArgs;
use crate::config::Config;

pub fn run(args: &CacheArgs, config: &Config) -> Result<()> {
    let sub = args.subcommand.as_deref().unwrap_or("status");

    match sub {
        "clear" | "flush" => {
            crate::cache::clear(&config.cache_dir());
            println!("Cache cleared.");
        }
        "status" | "info" => {
            let stats = cache_stats(&config.cache_dir());
            println!("Cache directory: {}", config.cache_dir().display());
            println!("Files: {}", stats.0);
            println!("Total size: {}", format_bytes(stats.1));
        }
        _ => {
            println!("Usage: xint cache [clear|status]");
            println!();
            println!("  clear     Remove all cached data");
            println!("  status    Show cache info (default)");
        }
    }

    Ok(())
}

fn cache_stats(dir: &std::path::Path) -> (usize, u64) {
    if !dir.exists() {
        return (0, 0);
    }
    let mut count = 0;
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    count += 1;
                    total += meta.len();
                }
            }
        }
    }
    (count, total)
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1_024 {
        format!("{:.1} KB", bytes as f64 / 1_024.0)
    } else {
        format!("{} bytes", bytes)
    }
}
