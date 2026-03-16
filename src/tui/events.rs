use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::commands::actions::normalize_interactive_choice;

use super::app::{App, UiPhase};

/// All messages the event loop can dispatch into the update function.
#[derive(Debug)]
pub enum Message {
    Quit,
    NavUp,
    NavDown,
    TabNext,
    TabCommands,
    TabOutput,
    TabHelp,
    PageUp,
    PageDown,
    SelectAction(String),
    StartFilter,
    StartPalette,
    // Input mode
    InputChar(char),
    InputBackspace,
    InputSubmit,
    InputCancel,
    // Running mode
    CancelRunning,
    // Resize events are handled directly by ratatui
    Noop,
}

pub fn map_key_event(key: KeyEvent, app: &App) -> Message {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match app.phase() {
        UiPhase::Input => map_input_key(key, ctrl),
        UiPhase::Running => map_running_key(key, ctrl),
        _ => map_idle_key(key, ctrl),
    }
}

fn map_input_key(key: KeyEvent, ctrl: bool) -> Message {
    match key.code {
        KeyCode::Enter => Message::InputSubmit,
        KeyCode::Esc => Message::InputCancel,
        KeyCode::Backspace => Message::InputBackspace,
        KeyCode::Char('c') if ctrl => Message::InputCancel,
        KeyCode::Char(ch) if !ctrl => Message::InputChar(ch),
        _ => Message::Noop,
    }
}

fn map_running_key(key: KeyEvent, ctrl: bool) -> Message {
    match key.code {
        KeyCode::Char('c') if ctrl => Message::CancelRunning,
        KeyCode::Tab => Message::TabNext,
        KeyCode::Char('1') => Message::TabCommands,
        KeyCode::Char('2') => Message::TabOutput,
        KeyCode::Char('3') => Message::TabHelp,
        KeyCode::PageUp => Message::PageUp,
        KeyCode::PageDown => Message::PageDown,
        _ => Message::Noop,
    }
}

fn map_idle_key(key: KeyEvent, ctrl: bool) -> Message {
    if ctrl {
        return Message::Noop;
    }
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Message::Quit,
        KeyCode::Up | KeyCode::Char('k') => Message::NavUp,
        KeyCode::Down | KeyCode::Char('j') => Message::NavDown,
        KeyCode::Tab => Message::TabNext,
        KeyCode::Char('1') => Message::TabCommands,
        KeyCode::Char('2') => Message::TabOutput,
        KeyCode::Char('3') => Message::TabHelp,
        KeyCode::Char('?') => Message::TabHelp,
        KeyCode::PageUp => Message::PageUp,
        KeyCode::PageDown => Message::PageDown,
        KeyCode::Char('f') | KeyCode::Char('F') => Message::StartFilter,
        KeyCode::Char('/') => Message::StartPalette,
        KeyCode::Enter => Message::SelectAction("__enter__".to_string()),
        KeyCode::Char(ch) => {
            if let Some(key) = normalize_interactive_choice(&ch.to_string()) {
                Message::SelectAction(key.to_string())
            } else {
                Message::Noop
            }
        }
        _ => Message::Noop,
    }
}
