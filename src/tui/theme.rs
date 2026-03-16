use ratatui::style::{Color, Modifier, Style};
use serde_json::Value;
use std::fs;

/// Parsed ANSI color string like "\x1b[1;36m" into a ratatui Style.
/// Supports a small subset: bold (1), dim (2), fg 256-color (38;5;N), fg basic (3N/9N), reset (0).
fn ansi_to_style(raw: &str) -> Style {
    let inner = raw
        .trim_start_matches('\x1b')
        .trim_start_matches('[')
        .trim_end_matches('m');

    let mut style = Style::default();
    let parts: Vec<&str> = inner.split(';').collect();
    let mut i = 0;
    while i < parts.len() {
        match parts[i] {
            "0" | "" => {
                style = Style::default();
            }
            "1" => {
                style = style.add_modifier(Modifier::BOLD);
            }
            "2" => {
                style = style.add_modifier(Modifier::DIM);
            }
            "38" if i + 2 < parts.len() && parts[i + 1] == "5" => {
                if let Ok(n) = parts[i + 2].parse::<u8>() {
                    style = style.fg(Color::Indexed(n));
                }
                i += 2;
            }
            "30" => style = style.fg(Color::Black),
            "31" => style = style.fg(Color::Red),
            "32" => style = style.fg(Color::Green),
            "33" => style = style.fg(Color::Yellow),
            "34" => style = style.fg(Color::Blue),
            "35" => style = style.fg(Color::Magenta),
            "36" => style = style.fg(Color::Cyan),
            "37" => style = style.fg(Color::White),
            "90" => style = style.fg(Color::DarkGray),
            "91" => style = style.fg(Color::LightRed),
            "92" => style = style.fg(Color::LightGreen),
            "93" => style = style.fg(Color::LightYellow),
            "94" => style = style.fg(Color::LightBlue),
            "95" => style = style.fg(Color::LightMagenta),
            "96" => style = style.fg(Color::LightCyan),
            "97" => style = style.fg(Color::Gray),
            _ => {}
        }
        i += 1;
    }
    style
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub accent: Style,
    pub border: Style,
    pub muted: Style,
    pub hero: Style,
    pub selected: Style,
    pub tab_active: Style,
    pub tab_inactive: Style,
    pub status_bar: Style,
    pub keybinding_key: Style,
    pub keybinding_desc: Style,
    pub hero_enabled: bool,
}

impl Default for Theme {
    fn default() -> Self {
        // classic theme
        Self {
            accent: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            border: Style::default().add_modifier(Modifier::DIM),
            muted: Style::default().add_modifier(Modifier::DIM),
            hero: Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
            selected: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            tab_active: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            tab_inactive: Style::default().add_modifier(Modifier::DIM),
            status_bar: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            keybinding_key: Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            keybinding_desc: Style::default().add_modifier(Modifier::DIM),
            hero_enabled: std::env::var("XINT_TUI_HERO").as_deref() != Ok("0"),
        }
    }
}

impl Theme {
    pub fn load() -> Self {
        let name = std::env::var("XINT_TUI_THEME")
            .unwrap_or_else(|_| "classic".to_string())
            .to_lowercase();

        let mut theme = match name.as_str() {
            "minimal" => Self {
                accent: Style::default().add_modifier(Modifier::BOLD),
                border: Style::default(),
                muted: Style::default(),
                hero: Style::default().add_modifier(Modifier::BOLD),
                selected: Style::default().add_modifier(Modifier::REVERSED),
                tab_active: Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
                tab_inactive: Style::default(),
                status_bar: Style::default().add_modifier(Modifier::BOLD),
                keybinding_key: Style::default().add_modifier(Modifier::BOLD),
                keybinding_desc: Style::default(),
                hero_enabled: std::env::var("XINT_TUI_HERO").as_deref() != Ok("0"),
            },
            "ocean" => Self {
                accent: Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
                border: Style::default().fg(Color::Indexed(39)),
                muted: Style::default().fg(Color::Indexed(244)),
                hero: Style::default()
                    .fg(Color::LightBlue)
                    .add_modifier(Modifier::BOLD),
                selected: Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
                tab_active: Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
                tab_inactive: Style::default().fg(Color::Indexed(244)),
                status_bar: Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
                keybinding_key: Style::default()
                    .fg(Color::LightBlue)
                    .add_modifier(Modifier::BOLD),
                keybinding_desc: Style::default().fg(Color::Indexed(244)),
                hero_enabled: std::env::var("XINT_TUI_HERO").as_deref() != Ok("0"),
            },
            "amber" => Self {
                accent: Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
                border: Style::default().fg(Color::Indexed(214)),
                muted: Style::default().fg(Color::Indexed(244)),
                hero: Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
                selected: Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
                tab_active: Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
                tab_inactive: Style::default().fg(Color::Indexed(244)),
                status_bar: Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
                keybinding_key: Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
                keybinding_desc: Style::default().fg(Color::Indexed(244)),
                hero_enabled: std::env::var("XINT_TUI_HERO").as_deref() != Ok("0"),
            },
            "neon" => Self {
                accent: Style::default()
                    .fg(Color::LightMagenta)
                    .add_modifier(Modifier::BOLD),
                border: Style::default().fg(Color::Indexed(45)),
                muted: Style::default().fg(Color::Indexed(244)),
                hero: Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
                selected: Style::default()
                    .fg(Color::LightMagenta)
                    .add_modifier(Modifier::BOLD),
                tab_active: Style::default()
                    .fg(Color::LightMagenta)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
                tab_inactive: Style::default().fg(Color::Indexed(244)),
                status_bar: Style::default()
                    .fg(Color::LightMagenta)
                    .add_modifier(Modifier::BOLD),
                keybinding_key: Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
                keybinding_desc: Style::default().fg(Color::Indexed(244)),
                hero_enabled: std::env::var("XINT_TUI_HERO").as_deref() != Ok("0"),
            },
            _ => Self::default(), // classic
        };

        // Apply JSON theme overrides if XINT_TUI_THEME_FILE is set
        if let Ok(path) = std::env::var("XINT_TUI_THEME_FILE") {
            if let Ok(raw) = fs::read_to_string(&path) {
                if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&raw) {
                    if let Some(Value::String(v)) = map.get("accent") {
                        theme.accent = ansi_to_style(v);
                    }
                    if let Some(Value::String(v)) = map.get("border") {
                        theme.border = ansi_to_style(v);
                    }
                    if let Some(Value::String(v)) = map.get("muted") {
                        theme.muted = ansi_to_style(v);
                    }
                    if let Some(Value::String(v)) = map.get("hero") {
                        theme.hero = ansi_to_style(v);
                    }
                }
            }
        }

        theme
    }
}
