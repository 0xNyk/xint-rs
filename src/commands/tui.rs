use std::cmp::max;
use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

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
    last_command: Option<String>,
    last_status: Option<String>,
    last_output_lines: Vec<String>,
}

struct MenuOption {
    key: &'static str,
    label: &'static str,
    aliases: &'static [&'static str],
    hint: &'static str,
}

struct Theme {
    accent: &'static str,
    border: &'static str,
    muted: &'static str,
    reset: &'static str,
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

const HELP_LINES: &[&str] = &[
    "Hotkeys",
    "  Up/Down: Move selection",
    "  Enter: Run selected command",
    "  /: Command palette",
    "  ?: Toggle help",
    "  q or Esc: Exit",
];

fn active_theme() -> Theme {
    match std::env::var("XINT_TUI_THEME")
        .unwrap_or_else(|_| "classic".to_string())
        .to_lowercase()
        .as_str()
    {
        "minimal" => Theme {
            accent: "\x1b[1m",
            border: "",
            muted: "",
            reset: "\x1b[0m",
        },
        "neon" => Theme {
            accent: "\x1b[1;95m",
            border: "\x1b[38;5;45m",
            muted: "\x1b[38;5;244m",
            reset: "\x1b[0m",
        },
        _ => Theme {
            accent: "\x1b[1;36m",
            border: "\x1b[2m",
            muted: "\x1b[2m",
            reset: "\x1b[0m",
        },
    }
}

fn prompt_line(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(buf.trim().to_string())
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

fn score_option(option: &MenuOption, query: &str) -> usize {
    let q = query.to_ascii_lowercase();
    if q.is_empty() {
        return 0;
    }
    let mut score = 0usize;
    if option.key == q {
        score += 100;
    }
    if option.label.eq_ignore_ascii_case(&q) {
        score += 90;
    }
    if option
        .aliases
        .iter()
        .any(|alias| alias.eq_ignore_ascii_case(&q))
    {
        score += 80;
    }
    if option.label.to_ascii_lowercase().starts_with(&q) {
        score += 70;
    }
    if option
        .aliases
        .iter()
        .any(|alias| alias.to_ascii_lowercase().starts_with(&q))
    {
        score += 60;
    }
    if option.label.to_ascii_lowercase().contains(&q) {
        score += 40;
    }
    if option.hint.to_ascii_lowercase().contains(&q) {
        score += 20;
    }
    score
}

fn match_palette(query: &str) -> Option<usize> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut best_index = None;
    let mut best_score = 0usize;
    for (index, option) in MENU_OPTIONS.iter().enumerate() {
        let score = score_option(option, trimmed);
        if score > best_score {
            best_score = score;
            best_index = Some(index);
        }
    }

    if best_score > 0 {
        best_index
    } else {
        None
    }
}

fn clip_text(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let count = value.chars().count();
    if count <= width {
        return value.to_string();
    }

    if width <= 3 {
        return ".".repeat(width);
    }

    let mut out = value.chars().take(width - 3).collect::<String>();
    out.push_str("...");
    out
}

fn pad_text(value: &str, width: usize) -> String {
    let clipped = clip_text(value, width);
    let len = clipped.chars().count();
    if len >= width {
        clipped
    } else {
        format!("{clipped:<width$}")
    }
}

fn build_left_lines(active_index: usize) -> Vec<String> {
    let mut lines = vec![
        "=== xint interactive ===".to_string(),
        String::new(),
        "Menu".to_string(),
        String::new(),
    ];

    for (index, option) in MENU_OPTIONS.iter().enumerate() {
        let pointer = if index == active_index { ">" } else { " " };
        let aliases = if option.aliases.is_empty() {
            String::new()
        } else {
            format!(" ({})", option.aliases.join(", "))
        };
        lines.push(format!(
            "{pointer} {}) {}{aliases}",
            option.key, option.label
        ));
        lines.push(format!("    {}", option.hint));
    }

    lines
}

fn build_right_lines(session: &SessionState, show_help: bool) -> Vec<String> {
    if show_help {
        let mut help = vec!["=== help ===".to_string()];
        help.extend(HELP_LINES.iter().map(|line| line.to_string()));
        return help;
    }

    let mut lines = vec![
        "=== last run ===".to_string(),
        format!(
            "command: {}",
            session.last_command.as_deref().unwrap_or("-")
        ),
        format!("status: {}", session.last_status.as_deref().unwrap_or("-")),
        String::new(),
        "output:".to_string(),
    ];

    if session.last_output_lines.is_empty() {
        lines.push("(none yet)".to_string());
    } else {
        lines.extend(session.last_output_lines.iter().cloned());
    }

    lines
}

fn render_dashboard(active_index: usize, session: &SessionState, show_help: bool) -> Result<()> {
    let theme = active_theme();
    let (cols, rows) = terminal::size().unwrap_or((120, 32));
    let total_rows = max(14usize, rows.saturating_sub(2) as usize);
    let left_width = max(42usize, (cols as usize * 45) / 100);
    let right_width = max(24usize, cols as usize - left_width - 3);

    let left_lines = build_left_lines(active_index);
    let mut right_lines = build_right_lines(session, show_help);
    if right_lines.len() > total_rows {
        right_lines = right_lines[right_lines.len() - total_rows..].to_vec();
    }

    let mut stdout = io::stdout();
    execute!(stdout, Clear(ClearType::All), MoveTo(0, 0))?;

    for row in 0..total_rows {
        let left_raw = left_lines.get(row).map(String::as_str).unwrap_or("");
        let right_raw = right_lines.get(row).map(String::as_str).unwrap_or("");
        let left = pad_text(left_raw, left_width);
        let right = pad_text(right_raw, right_width);

        if left_raw.starts_with("> ") {
            writeln!(
                stdout,
                "{}{}{} | {}",
                theme.accent, left, theme.reset, right
            )?;
        } else {
            writeln!(
                stdout,
                "{}{}{}{} | {}{}{}",
                theme.muted, left, theme.reset, theme.border, theme.muted, right, theme.reset,
            )?;
        }
    }

    let footer = " ↑↓ Navigate | Enter Run | / Palette | ? Help | q Quit ";
    writeln!(
        stdout,
        "{}{}{}",
        theme.border,
        pad_text(footer, cols as usize),
        theme.reset
    )?;
    stdout.flush()?;
    Ok(())
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

fn select_option_interactive(
    session: &mut SessionState,
    active_index_ref: &mut usize,
) -> Result<String> {
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
    let mut show_help = false;

    render_dashboard(*active_index_ref, session, show_help)?;

    loop {
        if let Event::Key(key_event) = event::read()? {
            match key_event.code {
                KeyCode::Up => {
                    *active_index_ref = if *active_index_ref == 0 {
                        MENU_OPTIONS.len() - 1
                    } else {
                        *active_index_ref - 1
                    };
                    render_dashboard(*active_index_ref, session, show_help)?;
                }
                KeyCode::Down => {
                    *active_index_ref = (*active_index_ref + 1) % MENU_OPTIONS.len();
                    render_dashboard(*active_index_ref, session, show_help)?;
                }
                KeyCode::Enter => {
                    let selected = MENU_OPTIONS
                        .get(*active_index_ref)
                        .map(|option| option.key.to_string())
                        .unwrap_or_else(|| "0".to_string());
                    execute!(io::stdout(), Clear(ClearType::All), MoveTo(0, 0))?;
                    return Ok(selected);
                }
                KeyCode::Char('q') | KeyCode::Esc => {
                    execute!(io::stdout(), Clear(ClearType::All), MoveTo(0, 0))?;
                    return Ok("0".to_string());
                }
                KeyCode::Char('?') => {
                    show_help = !show_help;
                    render_dashboard(*active_index_ref, session, show_help)?;
                }
                KeyCode::Char('/') => {
                    terminal::disable_raw_mode()?;
                    let query = prompt_line("\nPalette (/): ")?;
                    terminal::enable_raw_mode()?;
                    if let Some(index) = match_palette(&query) {
                        *active_index_ref = index;
                        let selected = MENU_OPTIONS
                            .get(*active_index_ref)
                            .map(|option| option.key.to_string())
                            .unwrap_or_else(|| "0".to_string());
                        execute!(io::stdout(), Clear(ClearType::All), MoveTo(0, 0))?;
                        return Ok(selected);
                    }
                    session.last_status = Some(format!(
                        "no palette match: {}",
                        if query.trim().is_empty() {
                            "(empty)"
                        } else {
                            query.trim()
                        }
                    ));
                    render_dashboard(*active_index_ref, session, show_help)?;
                }
                KeyCode::Char(ch) => {
                    if let Some(value) = normalize_choice(&ch.to_string()) {
                        execute!(io::stdout(), Clear(ClearType::All), MoveTo(0, 0))?;
                        return Ok(value.to_string());
                    }
                }
                _ => {}
            }
        }
    }
}

fn append_output(session: &mut SessionState, line: String) {
    let trimmed = line.trim_end().to_string();
    if trimmed.is_empty() {
        return;
    }
    session.last_output_lines.push(trimmed);
    if session.last_output_lines.len() > 120 {
        session.last_output_lines =
            session.last_output_lines[session.last_output_lines.len() - 120..].to_vec();
    }
}

fn run_subcommand(
    args: &[String],
    policy_mode: PolicyMode,
    session: &mut SessionState,
    active_index: usize,
) -> Result<()> {
    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg("--policy").arg(policy::as_str(policy_mode));
    for arg in args {
        cmd.arg(arg);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    session.last_output_lines.clear();

    let (tx, rx) = mpsc::channel::<String>();
    let mut handles = Vec::new();

    if let Some(stdout) = child.stdout.take() {
        let tx_out = tx.clone();
        handles.push(thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                let _ = tx_out.send(line);
            }
        }));
    }

    if let Some(stderr) = child.stderr.take() {
        let tx_err = tx.clone();
        handles.push(thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                let _ = tx_err.send(format!("[stderr] {line}"));
            }
        }));
    }

    drop(tx);

    let spinner_frames = ["|", "/", "-", "\\"];
    let mut spinner_index = 0usize;

    let status = loop {
        while let Ok(line) = rx.try_recv() {
            append_output(session, line);
            if io::stdin().is_terminal() && io::stdout().is_terminal() {
                render_dashboard(active_index, session, false)?;
            }
        }

        if let Some(status) = child.try_wait()? {
            break status;
        }

        if io::stdin().is_terminal() && io::stdout().is_terminal() {
            session.last_status = Some(format!(
                "running {}",
                spinner_frames[spinner_index % spinner_frames.len()]
            ));
            render_dashboard(active_index, session, false)?;
        }

        spinner_index += 1;
        thread::sleep(Duration::from_millis(90));
    };

    for handle in handles {
        let _ = handle.join();
    }

    while let Ok(line) = rx.try_recv() {
        append_output(session, line);
    }

    session.last_status = if status.success() {
        Some("success".to_string())
    } else {
        Some(format!(
            "failed (exit {})",
            status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "signal".to_string())
        ))
    };

    Ok(())
}

pub async fn run(_args: &TuiArgs, policy_mode: PolicyMode) -> Result<()> {
    let mut session = SessionState::default();
    let mut active_index = MENU_OPTIONS
        .iter()
        .position(|option| option.key == "1")
        .unwrap_or(0);

    loop {
        let choice = select_option_interactive(&mut session, &mut active_index)?;
        let Some(choice) = normalize_choice(&choice) else {
            session.last_status = Some("invalid selection".to_string());
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
                    session.last_status = Some("query is required".to_string());
                    continue;
                }
                session.last_search = Some(query.clone());
                session.last_command = Some(format!("xint search {query}"));
                run_subcommand(
                    &["search".to_string(), query],
                    policy_mode,
                    &mut session,
                    active_index,
                )?;
            }
            "2" => {
                let location = prompt_with_default(
                    "Location (blank for worldwide)",
                    session.last_location.as_deref(),
                )?;
                session.last_location = Some(location.clone());
                session.last_command = if location.is_empty() {
                    Some("xint trends".to_string())
                } else {
                    Some(format!("xint trends {location}"))
                };
                if location.is_empty() {
                    run_subcommand(
                        &["trends".to_string()],
                        policy_mode,
                        &mut session,
                        active_index,
                    )?;
                } else {
                    run_subcommand(
                        &["trends".to_string(), location],
                        policy_mode,
                        &mut session,
                        active_index,
                    )?;
                }
            }
            "3" => {
                let username =
                    prompt_with_default("Username (@optional)", session.last_username.as_deref())?
                        .trim_start_matches('@')
                        .to_string();
                if username.is_empty() {
                    session.last_status = Some("username is required".to_string());
                    continue;
                }
                session.last_username = Some(username.clone());
                session.last_command = Some(format!("xint profile {username}"));
                run_subcommand(
                    &["profile".to_string(), username],
                    policy_mode,
                    &mut session,
                    active_index,
                )?;
            }
            "4" => {
                let tweet_ref =
                    prompt_with_default("Tweet ID or URL", session.last_tweet_ref.as_deref())?;
                if tweet_ref.is_empty() {
                    session.last_status = Some("tweet id/url is required".to_string());
                    continue;
                }
                session.last_tweet_ref = Some(tweet_ref.clone());
                session.last_command = Some(format!("xint thread {tweet_ref}"));
                run_subcommand(
                    &["thread".to_string(), tweet_ref],
                    policy_mode,
                    &mut session,
                    active_index,
                )?;
            }
            "5" => {
                let url = prompt_with_default(
                    "Article URL or Tweet URL",
                    session.last_article_url.as_deref(),
                )?;
                if url.is_empty() {
                    session.last_status = Some("article url is required".to_string());
                    continue;
                }
                session.last_article_url = Some(url.clone());
                session.last_command = Some(format!("xint article {url}"));
                run_subcommand(
                    &["article".to_string(), url],
                    policy_mode,
                    &mut session,
                    active_index,
                )?;
            }
            "6" => {
                session.last_command = Some("xint --help".to_string());
                run_subcommand(
                    &["--help".to_string()],
                    policy_mode,
                    &mut session,
                    active_index,
                )?;
            }
            _ => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{match_palette, normalize_choice};

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

    #[test]
    fn palette_matches_expected_entries() {
        assert_eq!(match_palette("trend"), Some(1));
        assert_eq!(match_palette("profile"), Some(2));
        assert_eq!(match_palette("zzz"), None);
    }
}
