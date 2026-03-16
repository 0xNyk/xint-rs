use std::time::{SystemTime, UNIX_EPOCH};

use crate::commands::actions::{score_interactive_action, INTERACTIVE_ACTIONS};

use super::events::Message;
use super::theme::Theme;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum DashboardTab {
    Commands,
    Output,
    Help,
}

impl DashboardTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Commands => "Commands",
            Self::Output => "Output",
            Self::Help => "Help",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Commands => Self::Output,
            Self::Output => Self::Help,
            Self::Help => Self::Commands,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum UiPhase {
    Idle,
    Input,
    Running,
    Done,
    Error,
}

/// Which input prompt is currently active.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputMode {
    None,
    Filter,
    Palette,
    ActionParam {
        action_key: String,
        label: String,
        previous: Option<String>,
    },
}

/// A pending action to execute (set by update(), consumed by the run loop).
#[derive(Debug)]
pub enum PendingAction {
    ExecuteAction { action_key: String, value: String },
    Quit,
}

#[derive(Default)]
pub struct SessionState {
    pub last_search: Option<String>,
    pub last_location: Option<String>,
    pub last_username: Option<String>,
    pub last_tweet_ref: Option<String>,
    pub last_article_url: Option<String>,
    pub last_command: Option<String>,
    pub last_status: Option<String>,
    pub last_output_lines: Vec<String>,
}

pub struct App {
    pub active_index: usize,
    pub tab: DashboardTab,
    pub output_offset: usize,
    pub output_search: String,
    pub input_mode: InputMode,
    pub input_value: String,
    pub session: SessionState,
    pub theme: Theme,
    pub pending: Option<PendingAction>,
    pub should_quit: bool,
    pub is_running: bool,
}

impl App {
    pub fn new() -> Self {
        let initial_index = INTERACTIVE_ACTIONS
            .iter()
            .position(|a| a.key == "1")
            .unwrap_or(0);
        Self {
            active_index: initial_index,
            tab: DashboardTab::Output,
            output_offset: 0,
            output_search: String::new(),
            input_mode: InputMode::None,
            input_value: String::new(),
            session: SessionState::default(),
            theme: Theme::load(),
            pending: None,
            should_quit: false,
            is_running: false,
        }
    }

    pub fn phase(&self) -> UiPhase {
        if self.is_running {
            return UiPhase::Running;
        }
        if self.input_mode != InputMode::None {
            return UiPhase::Input;
        }
        let status = self
            .session
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

    pub fn phase_badge(&self) -> String {
        match self.phase() {
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

    pub fn hero_wave(&self) -> String {
        let running_palette = ["▁", "▂", "▃", "▄", "▅", "▆", "▇"];
        let idle_palette = ["·", "•", "·", "•", "·"];
        let palette = if self.phase() == UiPhase::Running {
            &running_palette[..]
        } else {
            &idle_palette[..]
        };
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as usize)
            .unwrap_or(0);
        let tick = millis / 110;
        (0..12)
            .map(|i| palette[(tick + i) % palette.len()])
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn append_output(&mut self, line: String) {
        let trimmed = sanitize_output_line(&line).trim_end().to_string();
        if trimmed.is_empty() {
            return;
        }
        self.session.last_output_lines.push(trimmed);
        if self.session.last_output_lines.len() > 1200 {
            let excess = self.session.last_output_lines.len() - 1200;
            self.session.last_output_lines.drain(0..excess);
        }
    }

    pub fn output_visible_lines(&self, viewport: usize) -> (Vec<&str>, usize, usize, usize) {
        let query = self.output_search.trim().to_ascii_lowercase();
        let filtered: Vec<&str> = if query.is_empty() {
            self.session
                .last_output_lines
                .iter()
                .map(String::as_str)
                .collect()
        } else {
            self.session
                .last_output_lines
                .iter()
                .filter(|line| line.to_ascii_lowercase().contains(&query))
                .map(String::as_str)
                .collect()
        };

        let total = filtered.len();
        let visible = viewport.max(1);
        let max_offset = total.saturating_sub(visible);
        let offset = self.output_offset.min(max_offset);

        let start = total.saturating_sub(visible.saturating_add(offset));
        let end = (start + visible).min(total);
        (filtered[start..end].to_vec(), start, end, total)
    }

    /// Update state based on a message. Returns true if the frame needs a redraw.
    pub fn update(&mut self, msg: Message) -> bool {
        match msg {
            Message::Quit => {
                self.pending = Some(PendingAction::Quit);
                self.should_quit = true;
            }
            Message::NavUp => {
                self.active_index = if self.active_index == 0 {
                    INTERACTIVE_ACTIONS.len() - 1
                } else {
                    self.active_index - 1
                };
            }
            Message::NavDown => {
                self.active_index = (self.active_index + 1) % INTERACTIVE_ACTIONS.len();
            }
            Message::TabNext => {
                self.tab = self.tab.next();
            }
            Message::TabCommands => {
                self.tab = DashboardTab::Commands;
            }
            Message::TabOutput => {
                self.tab = DashboardTab::Output;
            }
            Message::TabHelp => {
                self.tab = DashboardTab::Help;
            }
            Message::PageUp => {
                if matches!(self.tab, DashboardTab::Output) {
                    self.output_offset = self.output_offset.saturating_add(10);
                }
            }
            Message::PageDown => {
                if matches!(self.tab, DashboardTab::Output) {
                    self.output_offset = self.output_offset.saturating_sub(10);
                }
            }
            Message::StartFilter => {
                self.tab = DashboardTab::Output;
                self.input_mode = InputMode::Filter;
                self.input_value.clear();
            }
            Message::StartPalette => {
                self.tab = DashboardTab::Output;
                self.input_mode = InputMode::Palette;
                self.input_value.clear();
            }
            Message::SelectAction(ref key) => {
                let resolved_key = if key == "__enter__" {
                    INTERACTIVE_ACTIONS
                        .get(self.active_index)
                        .map(|a| a.key.to_string())
                        .unwrap_or_else(|| "0".to_string())
                } else {
                    key.clone()
                };

                if resolved_key == "0" {
                    self.pending = Some(PendingAction::Quit);
                    self.should_quit = true;
                    return true;
                }

                // Find the action and enter prompt mode
                if let Some(action) = INTERACTIVE_ACTIONS.iter().find(|a| a.key == resolved_key) {
                    // Update active index
                    if let Some(idx) = INTERACTIVE_ACTIONS
                        .iter()
                        .position(|a| a.key == resolved_key)
                    {
                        self.active_index = idx;
                    }
                    self.tab = DashboardTab::Output;

                    match resolved_key.as_str() {
                        "6" => {
                            // Help: no prompt needed, execute directly
                            self.pending = Some(PendingAction::ExecuteAction {
                                action_key: "6".to_string(),
                                value: String::new(),
                            });
                        }
                        "1" => {
                            self.input_mode = InputMode::ActionParam {
                                action_key: "1".to_string(),
                                label: "Search query".to_string(),
                                previous: self.session.last_search.clone(),
                            };
                            self.input_value.clear();
                        }
                        "2" => {
                            self.input_mode = InputMode::ActionParam {
                                action_key: "2".to_string(),
                                label: "Location (blank for worldwide)".to_string(),
                                previous: self.session.last_location.clone(),
                            };
                            self.input_value.clear();
                        }
                        "3" => {
                            self.input_mode = InputMode::ActionParam {
                                action_key: "3".to_string(),
                                label: "Username (@optional)".to_string(),
                                previous: self.session.last_username.clone(),
                            };
                            self.input_value.clear();
                        }
                        "4" => {
                            self.input_mode = InputMode::ActionParam {
                                action_key: "4".to_string(),
                                label: "Tweet ID or URL".to_string(),
                                previous: self.session.last_tweet_ref.clone(),
                            };
                            self.input_value.clear();
                        }
                        "5" => {
                            self.input_mode = InputMode::ActionParam {
                                action_key: "5".to_string(),
                                label: "Article URL or Tweet URL".to_string(),
                                previous: self.session.last_article_url.clone(),
                            };
                            self.input_value.clear();
                        }
                        _ => {
                            let _ = action;
                        }
                    }
                }
            }
            Message::InputChar(ch) => {
                self.input_value.push(ch);
            }
            Message::InputBackspace => {
                self.input_value.pop();
            }
            Message::InputSubmit => {
                let value = self.input_value.trim().to_string();
                match self.input_mode.clone() {
                    InputMode::Filter => {
                        self.output_search = value.clone();
                        self.output_offset = 0;
                        self.session.last_status = Some(if value.is_empty() {
                            "output filter cleared".to_string()
                        } else {
                            format!("output filter active: {value}")
                        });
                        self.input_mode = InputMode::None;
                        self.input_value.clear();
                    }
                    InputMode::Palette => {
                        if let Some(index) = match_palette(&value) {
                            self.active_index = index;
                            let action_key = INTERACTIVE_ACTIONS
                                .get(index)
                                .map(|a| a.key.to_string())
                                .unwrap_or_else(|| "0".to_string());
                            self.input_mode = InputMode::None;
                            self.input_value.clear();
                            // Re-dispatch as a SelectAction
                            self.update(Message::SelectAction(action_key));
                        } else {
                            self.session.last_status = Some(format!(
                                "no palette match: {}",
                                if value.is_empty() { "(empty)" } else { &value }
                            ));
                            self.input_mode = InputMode::None;
                            self.input_value.clear();
                        }
                        return true;
                    }
                    InputMode::ActionParam { ref action_key, .. } => {
                        let action_key = action_key.clone();
                        // Use previous value if empty
                        let effective_value = if value.is_empty() {
                            match &self.input_mode {
                                InputMode::ActionParam {
                                    previous: Some(prev),
                                    ..
                                } => prev.clone(),
                                _ => String::new(),
                            }
                        } else {
                            value.clone()
                        };

                        // Validate required fields
                        let required_keys = ["1", "3", "4", "5"];
                        if required_keys.contains(&action_key.as_str())
                            && effective_value.is_empty()
                        {
                            let label = match action_key.as_str() {
                                "1" => "query is required",
                                "3" => "username is required",
                                "4" => "tweet id/url is required",
                                "5" => "article url is required",
                                _ => "value is required",
                            };
                            self.session.last_status = Some(label.to_string());
                            self.input_mode = InputMode::None;
                            self.input_value.clear();
                            return true;
                        }

                        // Store last used value
                        match action_key.as_str() {
                            "1" => self.session.last_search = Some(effective_value.clone()),
                            "2" => self.session.last_location = Some(effective_value.clone()),
                            "3" => {
                                let username = effective_value.trim_start_matches('@').to_string();
                                self.session.last_username = Some(username.clone());
                                self.input_mode = InputMode::None;
                                self.input_value.clear();
                                self.pending = Some(PendingAction::ExecuteAction {
                                    action_key,
                                    value: username,
                                });
                                return true;
                            }
                            "4" => self.session.last_tweet_ref = Some(effective_value.clone()),
                            "5" => self.session.last_article_url = Some(effective_value.clone()),
                            _ => {}
                        }

                        self.input_mode = InputMode::None;
                        self.input_value.clear();
                        self.pending = Some(PendingAction::ExecuteAction {
                            action_key,
                            value: effective_value,
                        });
                    }
                    InputMode::None => {}
                }
            }
            Message::InputCancel => {
                self.input_mode = InputMode::None;
                self.input_value.clear();
            }
            Message::CancelRunning => {
                // Signal cancellation — the run loop handles killing the child
            }
            Message::Noop => return false,
        }
        true
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
