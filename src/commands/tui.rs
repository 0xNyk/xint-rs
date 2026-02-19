use std::io::{self, Write};
use std::process::Command;

use anyhow::Result;

use crate::cli::{PolicyMode, TuiArgs};
use crate::policy;

fn prompt_line(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(buf.trim().to_string())
}

fn run_subcommand(args: &[String], policy_mode: PolicyMode) -> Result<()> {
    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg("--policy").arg(policy::as_str(policy_mode));
    for arg in args {
        cmd.arg(arg);
    }
    let status = cmd.status()?;
    if !status.success() {
        eprintln!("[tui] command failed with exit status: {status}");
    }
    Ok(())
}

fn print_menu() {
    println!("\n=== xint tui ===");
    println!("1) Search");
    println!("2) Trends");
    println!("3) Profile");
    println!("4) Thread");
    println!("5) Article");
    println!("6) Help");
    println!("0) Exit");
}

pub async fn run(_args: &TuiArgs, policy_mode: PolicyMode) -> Result<()> {
    loop {
        print_menu();
        let choice = prompt_line("Select option: ")?;
        match choice.as_str() {
            "0" => {
                println!("Exiting xint tui.");
                break;
            }
            "1" => {
                let query = prompt_line("Search query: ")?;
                if query.is_empty() {
                    eprintln!("[tui] Query is required.");
                    continue;
                }
                run_subcommand(&["search".to_string(), query], policy_mode)?;
            }
            "2" => {
                let location = prompt_line("Location (blank for worldwide): ")?;
                if location.is_empty() {
                    run_subcommand(&["trends".to_string()], policy_mode)?;
                } else {
                    run_subcommand(&["trends".to_string(), location], policy_mode)?;
                }
            }
            "3" => {
                let username = prompt_line("Username (@optional): ")?
                    .trim_start_matches('@')
                    .to_string();
                if username.is_empty() {
                    eprintln!("[tui] Username is required.");
                    continue;
                }
                run_subcommand(&["profile".to_string(), username], policy_mode)?;
            }
            "4" => {
                let tweet_id = prompt_line("Tweet ID or URL: ")?;
                if tweet_id.is_empty() {
                    eprintln!("[tui] Tweet ID/URL is required.");
                    continue;
                }
                run_subcommand(&["thread".to_string(), tweet_id], policy_mode)?;
            }
            "5" => {
                let url = prompt_line("Article URL: ")?;
                if url.is_empty() {
                    eprintln!("[tui] Article URL is required.");
                    continue;
                }
                run_subcommand(&["article".to_string(), url], policy_mode)?;
            }
            "6" => run_subcommand(&["--help".to_string()], policy_mode)?,
            _ => println!("Unknown option."),
        }
    }
    Ok(())
}
