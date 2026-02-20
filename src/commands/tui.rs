use std::cmp::max;
use std::fs;
use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use serde_json::Value;

use crate::cli::{PolicyMode, TuiArgs};
use crate::commands::actions::{
    normalize_interactive_choice, score_interactive_action, INTERACTIVE_ACTIONS,
};
use crate::commands::tui_adapter::build_tui_execution_plan;
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

#[derive(Copy, Clone)]
enum DashboardTab {
    Commands,
    Output,
    Help,
}

#[derive(Copy, Clone)]
enum UiPhase {
    Idle,
    Input,
    Running,
    Done,
    Error,
}

impl DashboardTab {
    fn label(self) -> &'static str {
        match self {
            Self::Commands => "Commands",
            Self::Output => "Output",
            Self::Help => "Help",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Commands => Self::Output,
            Self::Output => Self::Help,
            Self::Help => Self::Commands,
        }
    }
}

struct UiState {
    active_index: usize,
    tab: DashboardTab,
    output_offset: usize,
    output_search: String,
    inline_prompt_label: Option<String>,
    inline_prompt_value: String,
}

struct Theme {
    accent: String,
    border: String,
    muted: String,
    hero: String,
    reset: String,
}

struct TerminalUiGuard {
    active: bool,
}

impl TerminalUiGuard {
    fn enter_if_tty() -> Result<Self> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Ok(Self { active: false });
        }

        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            Hide,
            Clear(ClearType::All),
            MoveTo(0, 0)
        )?;
        Ok(Self { active: true })
    }
}

impl Drop for TerminalUiGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }

        let mut stdout = io::stdout();
        let _ = execute!(stdout, Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

const HELP_LINES: &[&str] = &[
    "Hotkeys",
    "  Up/Down: Move selection",
    "  Enter: Run selected command",
    "  Tab: Switch tabs",
    "  F: Output search (filter)",
    "  PgUp/PgDn: Scroll output",
    "  /: Command palette",
    "  ?: Open Help tab",
    "  q or Esc: Exit",
];

fn active_theme() -> Theme {
    let mut theme = match std::env::var("XINT_TUI_THEME")
        .unwrap_or_else(|_| "classic".to_string())
        .to_lowercase()
        .as_str()
    {
        "minimal" => Theme {
            accent: "\x1b[1m".to_string(),
            border: "".to_string(),
            muted: "".to_string(),
            hero: "\x1b[1m".to_string(),
            reset: "\x1b[0m".to_string(),
        },
        "ocean" => Theme {
            accent: "\x1b[1;96m".to_string(),
            border: "\x1b[38;5;39m".to_string(),
            muted: "\x1b[38;5;244m".to_string(),
            hero: "\x1b[1;94m".to_string(),
            reset: "\x1b[0m".to_string(),
        },
        "amber" => Theme {
            accent: "\x1b[1;33m".to_string(),
            border: "\x1b[38;5;214m".to_string(),
            muted: "\x1b[38;5;244m".to_string(),
            hero: "\x1b[1;33m".to_string(),
            reset: "\x1b[0m".to_string(),
        },
        "neon" => Theme {
            accent: "\x1b[1;95m".to_string(),
            border: "\x1b[38;5;45m".to_string(),
            muted: "\x1b[38;5;244m".to_string(),
            hero: "\x1b[1;92m".to_string(),
            reset: "\x1b[0m".to_string(),
        },
        _ => Theme {
            accent: "\x1b[1;36m".to_string(),
            border: "\x1b[2m".to_string(),
            muted: "\x1b[2m".to_string(),
            hero: "\x1b[1;34m".to_string(),
            reset: "\x1b[0m".to_string(),
        },
    };

    if let Ok(path) = std::env::var("XINT_TUI_THEME_FILE") {
        if let Ok(raw) = fs::read_to_string(path) {
            if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&raw) {
                if let Some(Value::String(v)) = map.get("accent") {
                    theme.accent = v.clone();
                }
                if let Some(Value::String(v)) = map.get("border") {
                    theme.border = v.clone();
                }
                if let Some(Value::String(v)) = map.get("muted") {
                    theme.muted = v.clone();
                }
                if let Some(Value::String(v)) = map.get("hero") {
                    theme.hero = v.clone();
                }
                if let Some(Value::String(v)) = map.get("reset") {
                    theme.reset = v.clone();
                }
            }
        }
    }

    theme
}

fn is_hero_enabled() -> bool {
    std::env::var("XINT_TUI_HERO").as_deref() != Ok("0")
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

fn prompt_with_default_dashboard(
    label: &str,
    previous: Option<&str>,
    session: &SessionState,
    ui_state: &mut UiState,
) -> Result<String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return prompt_with_default(label, previous);
    }

    ui_state.tab = DashboardTab::Output;
    ui_state.inline_prompt_label = Some(label.to_string());
    ui_state.inline_prompt_value.clear();
    render_dashboard(ui_state, session)?;

    let value = loop {
        match event::read()? {
            Event::Resize(_, _) => {
                render_dashboard(ui_state, session)?;
            }
            Event::Key(key_event) => match key_event.code {
                KeyCode::Enter => break ui_state.inline_prompt_value.clone(),
                KeyCode::Esc => break String::new(),
                KeyCode::Backspace => {
                    ui_state.inline_prompt_value.pop();
                    render_dashboard(ui_state, session)?;
                }
                KeyCode::Char(ch) => {
                    if key_event.modifiers.contains(event::KeyModifiers::CONTROL) {
                        if ch == 'c' {
                            break String::new();
                        }
                    } else {
                        ui_state.inline_prompt_value.push(ch);
                        render_dashboard(ui_state, session)?;
                    }
                }
                _ => {}
            },
            _ => {}
        }
    };

    ui_state.inline_prompt_label = None;
    ui_state.inline_prompt_value.clear();
    render_dashboard(ui_state, session)?;

    if value.trim().is_empty() {
        Ok(previous.unwrap_or_default().to_string())
    } else {
        Ok(value)
    }
}

fn match_palette(query: &str) -> Option<usize> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut best_index = None;
    let mut best_score = 0usize;
    for (index, option) in INTERACTIVE_ACTIONS.iter().enumerate() {
        let score = score_interactive_action(option, trimmed);
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

fn build_tabs(ui_state: &UiState) -> String {
    [
        DashboardTab::Commands,
        DashboardTab::Output,
        DashboardTab::Help,
    ]
    .iter()
    .enumerate()
    .map(|(index, tab)| {
        let label = format!("{}:{}", index + 1, tab.label());
        if matches!(
            (tab, ui_state.tab),
            (DashboardTab::Commands, DashboardTab::Commands)
                | (DashboardTab::Output, DashboardTab::Output)
                | (DashboardTab::Help, DashboardTab::Help)
        ) {
            format!("‹{label}›")
        } else {
            format!("[{label}]")
        }
    })
    .collect::<Vec<_>>()
    .join(" ")
}

fn build_header_tracker(ui_state: &UiState, width: usize) -> String {
    let rail_width = width.clamp(8, 18);
    let cursor_basis = if ui_state.inline_prompt_label.is_some() {
        ui_state.inline_prompt_value.chars().count()
    } else {
        ui_state.active_index.saturating_mul(4) + ui_state.output_offset
    };
    let pos = cursor_basis % rail_width;
    let left = "·".repeat(pos);
    let right = "·".repeat(rail_width.saturating_sub(pos + 1));
    format!("focus {left}●{right}")
}

fn build_hero_line(ui_state: &UiState, session: &SessionState, width: usize) -> String {
    let phase = resolve_ui_phase(session, ui_state);
    let running_palette = ["▁", "▂", "▃", "▄", "▅", "▆", "▇"];
    let idle_palette = ["·", "•", "·", "•", "·"];
    let palette = if matches!(phase, UiPhase::Running) {
        &running_palette[..]
    } else {
        &idle_palette[..]
    };
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as usize)
        .unwrap_or(0);
    let tick = millis / 110;
    let wave = (0..12)
        .map(|i| palette[(tick + i) % palette.len()])
        .collect::<Vec<_>>()
        .join("");
    pad_text(&format!(" xint intelligence console  {wave}"), width)
}

#[allow(clippy::while_let_on_iterator)]
fn sanitize_output_line(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if let Some('[') = chars.peek().copied() {
                let _ = chars.next();
                while let Some(next) = chars.next() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
                continue;
            }
            if let Some(']') = chars.peek().copied() {
                let _ = chars.next();
                while let Some(next) = chars.next() {
                    if next == '\u{07}' {
                        break;
                    }
                    if next == '\u{1b}' && chars.peek().copied() == Some('\\') {
                        let _ = chars.next();
                        break;
                    }
                }
                continue;
            }
            continue;
        }

        if ch == '\n' || ch == '\t' || !ch.is_control() {
            out.push(ch);
        }
    }
    out
}

fn build_menu_lines(active_index: usize) -> Vec<String> {
    let mut lines = vec!["Menu".to_string(), String::new()];

    let icon_for_action = |key: &str| match key {
        "1" => "⌕ ",
        "2" => "◍ ",
        "3" => "◉ ",
        "4" => "↳ ",
        "5" => "✦ ",
        "6" => "? ",
        _ => "",
    };

    for (index, option) in INTERACTIVE_ACTIONS.iter().enumerate() {
        let pointer = if index == active_index { ">" } else { " " };
        let aliases = if option.aliases.is_empty() {
            String::new()
        } else {
            format!(" ({})", option.aliases.join(", "))
        };
        lines.push(format!(
            "{pointer} {}) {}{}{aliases}",
            option.key,
            icon_for_action(option.key),
            option.label
        ));
        lines.push(format!("    {}", option.hint));
    }

    lines
}

fn build_command_drawer(active_index: usize) -> Vec<String> {
    let selected = INTERACTIVE_ACTIONS
        .get(active_index)
        .unwrap_or(&INTERACTIVE_ACTIONS[0]);
    vec![
        "Command details".to_string(),
        String::new(),
        format!("Selected: {}", selected.label),
        format!("Summary: {}", selected.summary),
        format!("Example: {}", selected.example),
        format!("Cost: {}", selected.cost_hint),
    ]
}

fn resolve_ui_phase(session: &SessionState, ui_state: &UiState) -> UiPhase {
    if ui_state.inline_prompt_label.is_some() {
        return UiPhase::Input;
    }
    let status = session
        .last_status
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    if status.starts_with("running") {
        UiPhase::Running
    } else if status.contains("failed") || status.contains("error") {
        UiPhase::Error
    } else if status.contains("success") {
        UiPhase::Done
    } else {
        UiPhase::Idle
    }
}

fn phase_badge(phase: UiPhase) -> String {
    match phase {
        UiPhase::Running => {
            let frames = ["|", "/", "-", "\\"];
            let millis = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as usize)
                .unwrap_or(0);
            let idx = (millis / 120) % frames.len();
            format!("[RUNNING {}]", frames[idx])
        }
        UiPhase::Input => "[INPUT <>]".to_string(),
        UiPhase::Done => "[DONE ok]".to_string(),
        UiPhase::Error => "[ERROR !!]".to_string(),
        UiPhase::Idle => "[IDLE]".to_string(),
    }
}

fn output_view_lines(
    session: &SessionState,
    ui_state: &mut UiState,
    viewport: usize,
) -> Vec<String> {
    let query = ui_state.output_search.trim().to_ascii_lowercase();

    let filtered: Vec<&String> = if query.is_empty() {
        session.last_output_lines.iter().collect()
    } else {
        session
            .last_output_lines
            .iter()
            .filter(|line| line.to_ascii_lowercase().contains(&query))
            .collect()
    };

    let visible = max(1usize, viewport);
    let max_offset = filtered.len().saturating_sub(visible);
    if ui_state.output_offset > max_offset {
        ui_state.output_offset = max_offset;
    }

    let start = filtered
        .len()
        .saturating_sub(visible.saturating_add(ui_state.output_offset));
    let end = (start + visible).min(filtered.len());

    let mut lines = vec![
        "Last run".to_string(),
        String::new(),
        format!(
            "phase: {}",
            phase_badge(resolve_ui_phase(session, ui_state))
        ),
        format!(
            "command: {}",
            session.last_command.as_deref().unwrap_or("-")
        ),
        format!("status: {}", session.last_status.as_deref().unwrap_or("-")),
        format!(
            "filter: {}",
            if ui_state.output_search.trim().is_empty() {
                "(none)"
            } else {
                ui_state.output_search.trim()
            }
        ),
        String::new(),
        "output:".to_string(),
    ];

    if let Some(label) = &ui_state.inline_prompt_label {
        lines.push(String::new());
        lines.push(label.clone());
        lines.push(format!("> {}█", ui_state.inline_prompt_value));
        lines.push(String::new());
    }

    if start >= end {
        lines.push("(no output lines for current filter)".to_string());
    } else {
        for line in filtered[start..end].iter().map(|line| line.as_str()) {
            lines.push(line.to_string());
        }
    }

    let total = filtered.len();
    let from = if total == 0 { 0 } else { start + 1 };
    let to = if total == 0 { 0 } else { end };
    lines.push(String::new());
    lines.push(format!(
        "view {}-{} of {} | offset {}",
        from, to, total, ui_state.output_offset
    ));

    lines
}

fn build_tab_lines(session: &SessionState, ui_state: &mut UiState, viewport: usize) -> Vec<String> {
    match ui_state.tab {
        DashboardTab::Help => {
            let mut help = vec!["Help".to_string(), String::new()];
            help.extend(HELP_LINES.iter().map(|line| line.to_string()));
            help
        }
        DashboardTab::Commands => build_command_drawer(ui_state.active_index),
        DashboardTab::Output => output_view_lines(session, ui_state, viewport),
    }
}

fn build_status_line(session: &SessionState, ui_state: &UiState, width: usize) -> String {
    let selected = INTERACTIVE_ACTIONS
        .get(ui_state.active_index)
        .unwrap_or(&INTERACTIVE_ACTIONS[0]);
    let phase = resolve_ui_phase(session, ui_state);
    let focus = if let Some(label) = &ui_state.inline_prompt_label {
        format!("input:{label}")
    } else {
        format!("tab:{}", ui_state.tab.label())
    };
    let status = session.last_status.as_deref().unwrap_or("-");
    pad_text(
        &format!(
            " {} {}:{} | {} | {} ",
            phase_badge(phase),
            selected.key,
            selected.label,
            focus,
            status
        ),
        max(1usize, width),
    )
}

fn render_double_pane(
    ui_state: &mut UiState,
    session: &SessionState,
    cols: usize,
    rows: usize,
) -> Result<()> {
    let theme = active_theme();
    let total_rows = max(
        12usize,
        rows.saturating_sub(if is_hero_enabled() { 10 } else { 9 }),
    );
    let left_box_width = max(46usize, (cols * 45) / 100);
    let right_box_width = max(30usize, cols.saturating_sub(left_box_width + 1));
    let left_inner = max(20usize, left_box_width.saturating_sub(2));
    let right_inner = max(20usize, right_box_width.saturating_sub(2));

    let left_lines = build_menu_lines(ui_state.active_index);
    let mut right_lines = build_tab_lines(session, ui_state, total_rows);
    if right_lines.len() > total_rows {
        right_lines = right_lines[right_lines.len() - total_rows..].to_vec();
    }

    let tabs = build_tabs(ui_state);
    let tracker = build_header_tracker(ui_state, 16);

    let mut stdout = io::stdout();
    execute!(stdout, Clear(ClearType::All), MoveTo(0, 0))?;

    writeln!(
        stdout,
        "{}+{}+{}",
        theme.border,
        "-".repeat(cols.saturating_sub(2)),
        theme.reset
    )?;
    if is_hero_enabled() {
        writeln!(
            stdout,
            "{}|{}{}{}{}|{}",
            theme.border,
            theme.reset,
            theme.hero,
            build_hero_line(ui_state, session, cols.saturating_sub(2)),
            theme.reset,
            theme.border
        )?;
    }
    writeln!(
        stdout,
        "{}|{}{}{}|{}",
        theme.border,
        theme.reset,
        pad_text(&format!(" xint dashboard {}", tabs), cols.saturating_sub(2)),
        theme.border,
        theme.reset
    )?;
    writeln!(
        stdout,
        "{}|{}{}{}{}|{}",
        theme.border,
        theme.reset,
        theme.accent,
        pad_text(&format!(" {tracker}"), cols.saturating_sub(2)),
        theme.reset,
        theme.border
    )?;
    writeln!(
        stdout,
        "{}+{}+ +{}+{}",
        theme.border,
        "-".repeat(left_box_width.saturating_sub(2)),
        "-".repeat(right_box_width.saturating_sub(2)),
        theme.reset
    )?;

    for row in 0..total_rows {
        let left_raw = left_lines.get(row).map(String::as_str).unwrap_or("");
        let right_raw = right_lines.get(row).map(String::as_str).unwrap_or("");
        let left = pad_text(left_raw, left_inner);
        let right = pad_text(right_raw, right_inner);
        let left_segment = if left_raw.starts_with("> ") {
            format!("{}{}{}", theme.accent, left, theme.reset)
        } else {
            format!("{}{}{}", theme.muted, left, theme.reset)
        };

        writeln!(
            stdout,
            "{}|{}{}{}|{} {}|{}{}{}|{}",
            theme.border,
            theme.reset,
            left_segment,
            theme.border,
            theme.reset,
            theme.border,
            theme.muted,
            right,
            theme.border,
            theme.reset
        )?;
    }

    writeln!(
        stdout,
        "{}+{}+ +{}+{}",
        theme.border,
        "-".repeat(left_box_width.saturating_sub(2)),
        "-".repeat(right_box_width.saturating_sub(2)),
        theme.reset
    )?;
    writeln!(
        stdout,
        "{}|{}{}{}{}|{}",
        theme.border,
        theme.reset,
        theme.accent,
        build_status_line(session, ui_state, cols.saturating_sub(2)),
        theme.reset,
        theme.border
    )?;

    let footer =
        " ↑↓ Move • Enter Run • Tab Views • f Filter • / Palette • PgUp/PgDn Scroll • q Quit ";
    writeln!(
        stdout,
        "{}|{}{}{}|{}",
        theme.border,
        theme.reset,
        pad_text(footer, cols.saturating_sub(2)),
        theme.border,
        theme.reset
    )?;
    writeln!(
        stdout,
        "{}+{}+{}",
        theme.border,
        "-".repeat(cols.saturating_sub(2)),
        theme.reset
    )?;

    stdout.flush()?;
    Ok(())
}

fn render_single_pane(
    ui_state: &mut UiState,
    session: &SessionState,
    cols: usize,
    rows: usize,
) -> Result<()> {
    let theme = active_theme();
    let width = max(30usize, cols.saturating_sub(2));
    let total_rows = max(
        10usize,
        rows.saturating_sub(if is_hero_enabled() { 9 } else { 8 }),
    );

    let tabs = build_tabs(ui_state);
    let tracker = build_header_tracker(ui_state, 16);

    let lines = if matches!(ui_state.tab, DashboardTab::Commands) {
        let mut merged = build_menu_lines(ui_state.active_index);
        merged.push(String::new());
        merged.extend(build_command_drawer(ui_state.active_index));
        merged
    } else {
        build_tab_lines(session, ui_state, total_rows * 2)
    };

    let mut stdout = io::stdout();
    execute!(stdout, Clear(ClearType::All), MoveTo(0, 0))?;

    writeln!(
        stdout,
        "{}+{}+{}",
        theme.border,
        "-".repeat(width),
        theme.reset
    )?;
    if is_hero_enabled() {
        writeln!(
            stdout,
            "{}|{}{}{}{}|{}",
            theme.border,
            theme.reset,
            theme.hero,
            build_hero_line(ui_state, session, width),
            theme.reset,
            theme.border
        )?;
    }
    writeln!(
        stdout,
        "{}|{}{}{}|{}",
        theme.border,
        theme.reset,
        pad_text(&format!(" xint dashboard {}", tabs), width),
        theme.border,
        theme.reset
    )?;
    writeln!(
        stdout,
        "{}|{}{}{}{}|{}",
        theme.border,
        theme.reset,
        theme.accent,
        pad_text(&format!(" {tracker}"), width),
        theme.reset,
        theme.border
    )?;
    writeln!(
        stdout,
        "{}+{}+{}",
        theme.border,
        "-".repeat(width),
        theme.reset
    )?;

    for line in lines
        .iter()
        .rev()
        .take(total_rows)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let row = pad_text(line, width);
        if line.starts_with("> ") {
            writeln!(
                stdout,
                "{}|{}{}{}{}|{}",
                theme.border, theme.reset, theme.accent, row, theme.reset, theme.border
            )?;
        } else {
            writeln!(
                stdout,
                "{}|{}{}{}{}|{}",
                theme.border, theme.reset, theme.muted, row, theme.reset, theme.border
            )?;
        }
    }

    let rendered = lines.len().min(total_rows);
    for _ in rendered..total_rows {
        writeln!(
            stdout,
            "{}|{}{}|{}",
            theme.border,
            theme.reset,
            " ".repeat(width),
            theme.border
        )?;
    }

    writeln!(
        stdout,
        "{}+{}+{}",
        theme.border,
        "-".repeat(width),
        theme.reset
    )?;
    writeln!(
        stdout,
        "{}|{}{}{}{}|{}",
        theme.border,
        theme.reset,
        theme.accent,
        build_status_line(session, ui_state, width),
        theme.reset,
        theme.border
    )?;
    let footer = " Enter Run • Tab Views • f Filter • / Palette • PgUp/PgDn • q Quit ";
    writeln!(
        stdout,
        "{}|{}{}{}|{}",
        theme.border,
        theme.reset,
        pad_text(footer, width),
        theme.border,
        theme.reset
    )?;
    writeln!(
        stdout,
        "{}+{}+{}",
        theme.border,
        "-".repeat(width),
        theme.reset
    )?;

    stdout.flush()?;
    Ok(())
}

fn render_dashboard(ui_state: &mut UiState, session: &SessionState) -> Result<()> {
    let (cols, rows) = terminal::size().unwrap_or((120, 32));
    if (cols as usize) < 110 {
        render_single_pane(ui_state, session, cols as usize, rows as usize)
    } else {
        render_double_pane(ui_state, session, cols as usize, rows as usize)
    }
}

fn print_menu() {
    println!("\n=== xint interactive ===");
    for option in INTERACTIVE_ACTIONS {
        let aliases = if option.aliases.is_empty() {
            String::new()
        } else {
            format!(" ({})", option.aliases.join(", "))
        };
        println!("{}) {}{}", option.key, option.label, aliases);
        println!("   - {}", option.hint);
    }
}

fn select_option_interactive(session: &mut SessionState, ui_state: &mut UiState) -> Result<String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        print_menu();
        return prompt_line("\nSelect option (number or alias): ");
    }
    render_dashboard(ui_state, session)?;

    loop {
        match event::read()? {
            Event::Resize(_, _) => {
                render_dashboard(ui_state, session)?;
            }
            Event::Key(key_event) => match key_event.code {
                KeyCode::Up => {
                    ui_state.active_index = if ui_state.active_index == 0 {
                        INTERACTIVE_ACTIONS.len() - 1
                    } else {
                        ui_state.active_index - 1
                    };
                    render_dashboard(ui_state, session)?;
                }
                KeyCode::Down => {
                    ui_state.active_index = (ui_state.active_index + 1) % INTERACTIVE_ACTIONS.len();
                    render_dashboard(ui_state, session)?;
                }
                KeyCode::Tab => {
                    ui_state.tab = ui_state.tab.next();
                    render_dashboard(ui_state, session)?;
                }
                KeyCode::PageUp if matches!(ui_state.tab, DashboardTab::Output) => {
                    ui_state.output_offset = ui_state.output_offset.saturating_add(10);
                    render_dashboard(ui_state, session)?;
                }
                KeyCode::PageDown if matches!(ui_state.tab, DashboardTab::Output) => {
                    ui_state.output_offset = ui_state.output_offset.saturating_sub(10);
                    render_dashboard(ui_state, session)?;
                }
                KeyCode::Enter => {
                    ui_state.tab = DashboardTab::Output;
                    let selected = INTERACTIVE_ACTIONS
                        .get(ui_state.active_index)
                        .map(|option| option.key.to_string())
                        .unwrap_or_else(|| "0".to_string());
                    return Ok(selected);
                }
                KeyCode::Char('q') | KeyCode::Esc => {
                    return Ok("0".to_string());
                }
                KeyCode::Char('?') => {
                    ui_state.tab = DashboardTab::Help;
                    render_dashboard(ui_state, session)?;
                }
                KeyCode::Char('1') => {
                    ui_state.tab = DashboardTab::Commands;
                    render_dashboard(ui_state, session)?;
                }
                KeyCode::Char('2') => {
                    ui_state.tab = DashboardTab::Output;
                    render_dashboard(ui_state, session)?;
                }
                KeyCode::Char('3') => {
                    ui_state.tab = DashboardTab::Help;
                    render_dashboard(ui_state, session)?;
                }
                KeyCode::Char('f') | KeyCode::Char('F') => {
                    ui_state.tab = DashboardTab::Output;
                    return Ok("__filter__".to_string());
                }
                KeyCode::Char('/') => {
                    ui_state.tab = DashboardTab::Output;
                    return Ok("__palette__".to_string());
                }
                KeyCode::Char(ch) => {
                    if let Some(value) = normalize_interactive_choice(&ch.to_string()) {
                        return Ok(value.to_string());
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
}

fn append_output(session: &mut SessionState, line: String) {
    let trimmed = sanitize_output_line(&line).trim_end().to_string();
    if trimmed.is_empty() {
        return;
    }
    session.last_output_lines.push(trimmed);
    if session.last_output_lines.len() > 1200 {
        session.last_output_lines =
            session.last_output_lines[session.last_output_lines.len() - 1200..].to_vec();
    }
}

fn run_subcommand(
    args: &[String],
    policy_mode: PolicyMode,
    session: &mut SessionState,
    ui_state: &mut UiState,
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
    ui_state.output_offset = 0;

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
                render_dashboard(ui_state, session)?;
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
            render_dashboard(ui_state, session)?;
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
    let _terminal_guard = TerminalUiGuard::enter_if_tty()?;

    let mut session = SessionState::default();
    let initial_index = INTERACTIVE_ACTIONS
        .iter()
        .position(|option| option.key == "1")
        .unwrap_or(0);

    let mut ui_state = UiState {
        active_index: initial_index,
        tab: DashboardTab::Output,
        output_offset: 0,
        output_search: String::new(),
        inline_prompt_label: None,
        inline_prompt_value: String::new(),
    };

    loop {
        let mut choice = select_option_interactive(&mut session, &mut ui_state)?;
        if choice == "__filter__" {
            let query = prompt_with_default_dashboard(
                "Output search (blank clears)",
                Some(""),
                &session,
                &mut ui_state,
            )?;
            ui_state.output_search = query.trim().to_string();
            ui_state.output_offset = 0;
            ui_state.tab = DashboardTab::Output;
            session.last_status = Some(if ui_state.output_search.is_empty() {
                "output filter cleared".to_string()
            } else {
                format!("output filter active: {}", ui_state.output_search)
            });
            continue;
        }
        if choice == "__palette__" {
            let query =
                prompt_with_default_dashboard("Palette (/)", Some(""), &session, &mut ui_state)?;
            if let Some(index) = match_palette(&query) {
                ui_state.active_index = index;
                ui_state.tab = DashboardTab::Output;
                choice = INTERACTIVE_ACTIONS
                    .get(index)
                    .map(|option| option.key.to_string())
                    .unwrap_or_else(|| "0".to_string());
            } else {
                session.last_status = Some(format!(
                    "no palette match: {}",
                    if query.trim().is_empty() {
                        "(empty)"
                    } else {
                        query.trim()
                    }
                ));
                continue;
            }
        }
        let Some(choice) = normalize_interactive_choice(&choice) else {
            session.last_status = Some("invalid selection".to_string());
            continue;
        };

        match choice {
            "0" => {
                break;
            }
            "1" => {
                let query = prompt_with_default_dashboard(
                    "Search query",
                    session.last_search.as_deref(),
                    &session,
                    &mut ui_state,
                )?;
                if query.is_empty() {
                    session.last_status = Some("query is required".to_string());
                    continue;
                }
                session.last_search = Some(query.clone());
                let plan_result = build_tui_execution_plan(choice, Some(&query));
                let Some(plan) = plan_result.data else {
                    session.last_status = Some(plan_result.message);
                    continue;
                };
                session.last_command = Some(plan.command.clone());
                run_subcommand(&plan.args, policy_mode, &mut session, &mut ui_state)?;
            }
            "2" => {
                let location = prompt_with_default_dashboard(
                    "Location (blank for worldwide)",
                    session.last_location.as_deref(),
                    &session,
                    &mut ui_state,
                )?;
                session.last_location = Some(location.clone());
                let plan_result = build_tui_execution_plan(choice, Some(&location));
                let Some(plan) = plan_result.data else {
                    session.last_status = Some(plan_result.message);
                    continue;
                };
                session.last_command = Some(plan.command.clone());
                run_subcommand(&plan.args, policy_mode, &mut session, &mut ui_state)?;
            }
            "3" => {
                let username = prompt_with_default_dashboard(
                    "Username (@optional)",
                    session.last_username.as_deref(),
                    &session,
                    &mut ui_state,
                )?
                .trim_start_matches('@')
                .to_string();
                if username.is_empty() {
                    session.last_status = Some("username is required".to_string());
                    continue;
                }
                session.last_username = Some(username.clone());
                let plan_result = build_tui_execution_plan(choice, Some(&username));
                let Some(plan) = plan_result.data else {
                    session.last_status = Some(plan_result.message);
                    continue;
                };
                session.last_command = Some(plan.command.clone());
                run_subcommand(&plan.args, policy_mode, &mut session, &mut ui_state)?;
            }
            "4" => {
                let tweet_ref = prompt_with_default_dashboard(
                    "Tweet ID or URL",
                    session.last_tweet_ref.as_deref(),
                    &session,
                    &mut ui_state,
                )?;
                if tweet_ref.is_empty() {
                    session.last_status = Some("tweet id/url is required".to_string());
                    continue;
                }
                session.last_tweet_ref = Some(tweet_ref.clone());
                let plan_result = build_tui_execution_plan(choice, Some(&tweet_ref));
                let Some(plan) = plan_result.data else {
                    session.last_status = Some(plan_result.message);
                    continue;
                };
                session.last_command = Some(plan.command.clone());
                run_subcommand(&plan.args, policy_mode, &mut session, &mut ui_state)?;
            }
            "5" => {
                let url = prompt_with_default_dashboard(
                    "Article URL or Tweet URL",
                    session.last_article_url.as_deref(),
                    &session,
                    &mut ui_state,
                )?;
                if url.is_empty() {
                    session.last_status = Some("article url is required".to_string());
                    continue;
                }
                session.last_article_url = Some(url.clone());
                let plan_result = build_tui_execution_plan(choice, Some(&url));
                let Some(plan) = plan_result.data else {
                    session.last_status = Some(plan_result.message);
                    continue;
                };
                session.last_command = Some(plan.command.clone());
                run_subcommand(&plan.args, policy_mode, &mut session, &mut ui_state)?;
            }
            "6" => {
                let plan_result = build_tui_execution_plan(choice, None);
                let Some(plan) = plan_result.data else {
                    session.last_status = Some(plan_result.message);
                    continue;
                };
                session.last_command = Some(plan.command.clone());
                run_subcommand(&plan.args, policy_mode, &mut session, &mut ui_state)?;
            }
            _ => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::match_palette;
    use crate::commands::actions::normalize_interactive_choice;

    #[test]
    fn normalize_choice_supports_numeric_and_alias_inputs() {
        assert_eq!(normalize_interactive_choice("1"), Some("1"));
        assert_eq!(normalize_interactive_choice("search"), Some("1"));
        assert_eq!(normalize_interactive_choice("Q"), Some("0"));
    }

    #[test]
    fn normalize_choice_rejects_invalid_values() {
        assert_eq!(normalize_interactive_choice(""), None);
        assert_eq!(normalize_interactive_choice("unknown"), None);
    }

    #[test]
    fn palette_matches_expected_entries() {
        assert_eq!(match_palette("trend"), Some(1));
        assert_eq!(match_palette("profile"), Some(2));
        assert_eq!(match_palette("zzz"), None);
    }
}
