use std::io::{self, IsTerminal, Write};
use std::process::Command;

use anyhow::Result;
use crossterm::cursor::MoveTo;
use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{self, Clear, ClearType};

use crate::cli::{PolicyMode, TuiArgs};
use crate::policy;

#[derive(Default)]
struct SessionState {
    last_search: Option<String>,
    last_location: Option<String>,
    last_username: Option<String>,
    last_tweet_ref: Option<String>,
    last_article_url: Option<String>,
}

struct MenuOption {
    key: &'static str,
    label: &'static str,
    aliases: &'static [&'static str],
    hint: &'static str,
}

const MENU_OPTIONS: &[MenuOption] = &[
    MenuOption {
        key: "1",
        label: "Search",
        aliases: &["search", "s"],
        hint: "keyword, topic, or boolean query",
    },
    MenuOption {
        key: "2",
        label: "Trends",
        aliases: &["trends", "trend", "t"],
        hint: "location name or blank for global",
    },
    MenuOption {
        key: "3",
        label: "Profile",
        aliases: &["profile", "user", "p"],
        hint: "username (without @)",
    },
    MenuOption {
        key: "4",
        label: "Thread",
        aliases: &["thread", "th"],
        hint: "tweet id or tweet url",
    },
    MenuOption {
        key: "5",
        label: "Article",
        aliases: &["article", "a"],
        hint: "article url or tweet url",
    },
    MenuOption {
        key: "6",
        label: "Help",
        aliases: &["help", "h", "?"],
        hint: "show full CLI help",
    },
    MenuOption {
        key: "0",
        label: "Exit",
        aliases: &["exit", "quit", "q"],
        hint: "close interactive mode",
    },
];

fn prompt_line(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(buf.trim().to_string())
}

fn wait_for_enter() -> Result<()> {
    let _ = prompt_line("\nPress Enter to return to menu...")?;
    Ok(())
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

fn prompt_with_default(label: &str, previous: Option<&str>) -> Result<String> {
    let prompt = match previous {
        Some(value) => format!("{label} [{value}]: "),
        None => format!("{label}: "),
    };
    let input = prompt_line(&prompt)?;
    if input.trim().is_empty() {
        Ok(previous.unwrap_or_default().to_string())
    } else {
        Ok(input)
    }
}

fn normalize_choice(raw: &str) -> Option<&'static str> {
    let value = raw.trim().to_ascii_lowercase();
    if value.is_empty() {
        return None;
    }
    for option in MENU_OPTIONS {
        if option.key == value {
            return Some(option.key);
        }
        if option.aliases.iter().any(|alias| alias == &value) {
            return Some(option.key);
        }
    }
    None
}

fn print_menu() {
    println!("\n=== xint interactive ===");
    for option in MENU_OPTIONS {
        let aliases = if option.aliases.is_empty() {
            String::new()
        } else {
            format!(" ({})", option.aliases.join(", "))
        };
        println!("{}) {}{}", option.key, option.label, aliases);
        println!("   - {}", option.hint);
    }
}

fn render_interactive_menu(active_index: usize) -> Result<()> {
    let mut stdout = io::stdout();
    execute!(stdout, Clear(ClearType::All), MoveTo(0, 0))?;
    writeln!(stdout, "=== xint interactive ===")?;
    writeln!(stdout, "Use Up/Down arrows and Enter. Press q to exit.\n")?;
    for (index, option) in MENU_OPTIONS.iter().enumerate() {
        let aliases = if option.aliases.is_empty() {
            String::new()
        } else {
            format!(" ({})", option.aliases.join(", "))
        };
        if index == active_index {
            writeln!(
                stdout,
                "\x1b[1;36m› {}) {}{}\x1b[0m",
                option.key, option.label, aliases
            )?;
        } else {
            writeln!(stdout, "  {}) {}{}", option.key, option.label, aliases)?;
        }
        writeln!(stdout, "    {}", option.hint)?;
    }
    stdout.flush()?;
    Ok(())
}

fn select_option_interactive() -> Result<String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        print_menu();
        return prompt_line("\nSelect option (number or alias): ");
    }

    struct RawModeGuard;
    impl Drop for RawModeGuard {
        fn drop(&mut self) {
            let _ = terminal::disable_raw_mode();
        }
    }

    terminal::enable_raw_mode()?;
    let _raw_mode_guard = RawModeGuard;
    let mut active_index = MENU_OPTIONS
        .iter()
        .position(|option| option.key == "1")
        .unwrap_or(0);
    render_interactive_menu(active_index)?;

    loop {
        if let Event::Key(key_event) = event::read()? {
            match key_event.code {
                KeyCode::Up => {
                    active_index = if active_index == 0 {
                        MENU_OPTIONS.len() - 1
                    } else {
                        active_index - 1
                    };
                    render_interactive_menu(active_index)?;
                }
                KeyCode::Down => {
                    active_index = (active_index + 1) % MENU_OPTIONS.len();
                    render_interactive_menu(active_index)?;
                }
                KeyCode::Enter => {
                    let selected = MENU_OPTIONS
                        .get(active_index)
                        .map(|option| option.key.to_string())
                        .unwrap_or_else(|| "0".to_string());
                    execute!(io::stdout(), Clear(ClearType::All), MoveTo(0, 0))?;
                    return Ok(selected);
                }
                KeyCode::Char(ch) => {
                    let normalized = normalize_choice(&ch.to_string());
                    if let Some(value) = normalized {
                        execute!(io::stdout(), Clear(ClearType::All), MoveTo(0, 0))?;
                        return Ok(value.to_string());
                    }
                }
                KeyCode::Esc => {
                    execute!(io::stdout(), Clear(ClearType::All), MoveTo(0, 0))?;
                    return Ok("0".to_string());
                }
                _ => {}
            }
        }
    }
}

pub async fn run(_args: &TuiArgs, policy_mode: PolicyMode) -> Result<()> {
    let mut session = SessionState::default();
    loop {
        let choice = select_option_interactive()?;
        let Some(choice) = normalize_choice(&choice) else {
            eprintln!("[tui] Unknown option. Use a number (0-6) or alias like 'search' / 'help'.");
            continue;
        };

        match choice {
            "0" => {
                println!("Exiting xint interactive mode.");
                break;
            }
            "1" => {
                let query = prompt_with_default("Search query", session.last_search.as_deref())?;
                if query.is_empty() {
                    eprintln!("[tui] Query is required.");
                    wait_for_enter()?;
                    continue;
                }
                session.last_search = Some(query.clone());
                run_subcommand(&["search".to_string(), query], policy_mode)?;
                wait_for_enter()?;
            }
            "2" => {
                let location = prompt_with_default(
                    "Location (blank for worldwide)",
                    session.last_location.as_deref(),
                )?;
                session.last_location = Some(location.clone());
                if location.is_empty() {
                    run_subcommand(&["trends".to_string()], policy_mode)?;
                } else {
                    run_subcommand(&["trends".to_string(), location], policy_mode)?;
                }
                wait_for_enter()?;
            }
            "3" => {
                let username =
                    prompt_with_default("Username (@optional)", session.last_username.as_deref())?
                        .trim_start_matches('@')
                        .to_string();
                if username.is_empty() {
                    eprintln!("[tui] Username is required.");
                    wait_for_enter()?;
                    continue;
                }
                session.last_username = Some(username.clone());
                run_subcommand(&["profile".to_string(), username], policy_mode)?;
                wait_for_enter()?;
            }
            "4" => {
                let tweet_ref =
                    prompt_with_default("Tweet ID or URL", session.last_tweet_ref.as_deref())?;
                if tweet_ref.is_empty() {
                    eprintln!("[tui] Tweet ID/URL is required.");
                    wait_for_enter()?;
                    continue;
                }
                session.last_tweet_ref = Some(tweet_ref.clone());
                run_subcommand(&["thread".to_string(), tweet_ref], policy_mode)?;
                wait_for_enter()?;
            }
            "5" => {
                let url = prompt_with_default(
                    "Article URL or Tweet URL",
                    session.last_article_url.as_deref(),
                )?;
                if url.is_empty() {
                    eprintln!("[tui] Article URL is required.");
                    wait_for_enter()?;
                    continue;
                }
                session.last_article_url = Some(url.clone());
                run_subcommand(&["article".to_string(), url], policy_mode)?;
                wait_for_enter()?;
            }
            "6" => {
                run_subcommand(&["--help".to_string()], policy_mode)?;
                wait_for_enter()?;
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::normalize_choice;

    #[test]
    fn normalize_choice_supports_numeric_and_alias_inputs() {
        assert_eq!(normalize_choice("1"), Some("1"));
        assert_eq!(normalize_choice("search"), Some("1"));
        assert_eq!(normalize_choice("Q"), Some("0"));
    }

    #[test]
    fn normalize_choice_rejects_invalid_values() {
        assert_eq!(normalize_choice(""), None);
        assert_eq!(normalize_choice("unknown"), None);
    }
}
