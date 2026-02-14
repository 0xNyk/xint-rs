use anyhow::Result;
use std::fs;

use crate::cli::WatchlistArgs;
use crate::config::Config;
use crate::models::*;

fn load_watchlist(config: &Config) -> Watchlist {
    let path = config.watchlist_path();
    if !path.exists() {
        return Watchlist::default();
    }
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Watchlist::default(),
    }
}

fn save_watchlist(config: &Config, wl: &Watchlist) -> Result<()> {
    let path = config.watchlist_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(wl)?;
    fs::write(&path, &json)?;
    Ok(())
}

pub fn run(args: &WatchlistArgs, config: &Config) -> Result<()> {
    let parts: Vec<String> = args
        .subcommand
        .as_ref()
        .map(|v| v.clone())
        .unwrap_or_default();

    let sub = parts.first().map(|s| s.as_str()).unwrap_or("list");

    match sub {
        "list" | "ls" => {
            let wl = load_watchlist(config);
            if wl.accounts.is_empty() {
                println!("Watchlist is empty. Use 'xint watchlist add @username' to add accounts.");
                return Ok(());
            }
            println!("\nWatchlist ({} accounts):\n", wl.accounts.len());
            for acct in &wl.accounts {
                let note = acct
                    .note
                    .as_deref()
                    .map(|n| format!(" -- {}", n))
                    .unwrap_or_default();
                let date = &acct.added_at[..10];
                println!("  @{}{} (added {})", acct.username, note, date);
            }
            println!();
        }
        "add" => {
            let username = parts
                .get(1)
                .map(|s| s.trim_start_matches('@').to_string())
                .ok_or_else(|| anyhow::anyhow!("Usage: xint watchlist add @username [note]"))?;

            let note = if parts.len() > 2 {
                Some(parts[2..].join(" "))
            } else {
                None
            };

            let mut wl = load_watchlist(config);
            if wl
                .accounts
                .iter()
                .any(|a| a.username.to_lowercase() == username.to_lowercase())
            {
                println!("@{} is already on the watchlist.", username);
                return Ok(());
            }

            wl.accounts.push(WatchlistAccount {
                username: username.clone(),
                note,
                added_at: chrono::Utc::now().to_rfc3339(),
            });

            save_watchlist(config, &wl)?;
            println!("Added @{} to watchlist ({} total)", username, wl.accounts.len());
        }
        "remove" | "rm" => {
            let username = parts
                .get(1)
                .map(|s| s.trim_start_matches('@').to_lowercase())
                .ok_or_else(|| anyhow::anyhow!("Usage: xint watchlist remove @username"))?;

            let mut wl = load_watchlist(config);
            let before = wl.accounts.len();
            wl.accounts
                .retain(|a| a.username.to_lowercase() != username);
            let after = wl.accounts.len();

            if before == after {
                println!("@{} was not on the watchlist.", username);
                return Ok(());
            }

            save_watchlist(config, &wl)?;
            println!("Removed @{} from watchlist ({} remaining)", username, after);
        }
        "check" => {
            let wl = load_watchlist(config);
            if wl.accounts.is_empty() {
                println!("Watchlist is empty.");
                return Ok(());
            }

            let username = parts
                .get(1)
                .map(|s| s.trim_start_matches('@').to_lowercase());

            match username {
                Some(u) => {
                    let found = wl
                        .accounts
                        .iter()
                        .any(|a| a.username.to_lowercase() == u);
                    if found {
                        println!("@{} is on the watchlist.", u);
                    } else {
                        println!("@{} is NOT on the watchlist.", u);
                    }
                }
                None => {
                    println!(
                        "Watchlist: {} accounts",
                        wl.accounts.len()
                    );
                    let usernames: Vec<_> = wl
                        .accounts
                        .iter()
                        .map(|a| format!("@{}", a.username))
                        .collect();
                    println!("{}", usernames.join(", "));
                }
            }
        }
        _ => {
            println!("Usage: xint watchlist [add|remove|check|list]");
            println!();
            println!("  list                     Show all watched accounts (default)");
            println!("  add @username [note]     Add account to watchlist");
            println!("  remove @username         Remove account from watchlist");
            println!("  check [@username]        Check if account is on watchlist");
        }
    }

    Ok(())
}
