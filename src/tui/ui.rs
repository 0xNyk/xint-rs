use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap},
    Frame,
};

use crate::commands::actions::INTERACTIVE_ACTIONS;

use super::app::{App, DashboardTab, InputMode, UiPhase};

const HELP_TEXT: &str = "\
Hotkeys
  Up/Down / j/k : Move selection
  Enter         : Run selected command
  Tab           : Switch tabs
  f             : Output search (filter)
  PgUp/PgDn     : Scroll output
  /             : Command palette
  ?             : Open Help tab
  1/2/3         : Jump to tab
  q or Esc      : Exit
";

pub fn render(app: &mut App, frame: &mut Frame) {
    let area = frame.area();

    // Outer border
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(app.theme.border);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    // Vertical layout: hero (optional) | header | tabs | body | status | keybindings
    let hero_height = if app.theme.hero_enabled { 1 } else { 0 };
    let constraints = [
        Constraint::Length(hero_height), // hero
        Constraint::Length(1),           // tab bar
        Constraint::Length(1),           // tracker
        Constraint::Min(4),              // body
        Constraint::Length(1),           // status bar
        Constraint::Length(1),           // keybinding bar
    ];
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    // Hero line
    if app.theme.hero_enabled {
        let wave = app.hero_wave();
        let hero_text = format!(" xint intelligence console  {wave}");
        let hero = Paragraph::new(hero_text).style(app.theme.hero);
        frame.render_widget(hero, chunks[0]);
    }

    // Tab bar
    render_tabs(app, frame, chunks[1]);

    // Tracker line
    render_tracker(app, frame, chunks[2]);

    // Body — split or single pane
    let body_area = chunks[3];
    let use_double_pane = body_area.width >= 110;
    if use_double_pane {
        render_double_pane(app, frame, body_area);
    } else {
        render_single_pane(app, frame, body_area);
    }

    // Status bar
    render_status_bar(app, frame, chunks[4]);

    // Keybinding bar
    render_keybindings(app, frame, chunks[5]);

    // Inline prompt overlay (rendered over the body area when in Input phase)
    if app.input_mode != InputMode::None {
        render_input_overlay(app, frame, body_area);
    }
}

fn render_tabs(app: &App, frame: &mut Frame, area: Rect) {
    let tabs_data = [
        DashboardTab::Commands,
        DashboardTab::Output,
        DashboardTab::Help,
    ];
    let titles: Vec<Line> = tabs_data
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let label = format!("{}:{}", i + 1, tab.label());
            if *tab == app.tab {
                Line::from(Span::styled(format!(" {label} "), app.theme.tab_active))
            } else {
                Line::from(Span::styled(format!(" {label} "), app.theme.tab_inactive))
            }
        })
        .collect();
    let selected = match app.tab {
        DashboardTab::Commands => 0,
        DashboardTab::Output => 1,
        DashboardTab::Help => 2,
    };
    let tabs = Tabs::new(titles)
        .select(selected)
        .highlight_style(app.theme.tab_active)
        .divider(Span::raw(" | "));
    frame.render_widget(tabs, area);
}

fn render_tracker(app: &App, frame: &mut Frame, area: Rect) {
    let rail_width = (area.width as usize).clamp(8, 18);
    let cursor_basis = if app.input_mode != InputMode::None {
        app.input_value.chars().count()
    } else {
        app.active_index.saturating_mul(4) + app.output_offset
    };
    let pos = cursor_basis % rail_width;
    let left = "·".repeat(pos);
    let right = "·".repeat(rail_width.saturating_sub(pos + 1));
    let tracker_text = format!(" focus {left}●{right}");
    let tracker = Paragraph::new(tracker_text).style(app.theme.accent);
    frame.render_widget(tracker, area);
}

fn render_double_pane(app: &mut App, frame: &mut Frame, area: Rect) {
    let left_width = ((area.width as usize * 45) / 100).clamp(46, area.width as usize - 32);
    let right_width = area.width as usize - left_width - 1;

    let pane_constraints = [
        Constraint::Length(left_width as u16),
        Constraint::Length(1),
        Constraint::Min(right_width as u16),
    ];
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(pane_constraints)
        .split(area);

    // Left pane: actions menu
    render_action_list(app, frame, panes[0]);

    // Divider
    let divider = Paragraph::new("│".repeat(area.height as usize)).style(app.theme.border);
    frame.render_widget(divider, panes[1]);

    // Right pane: active tab content
    render_tab_content(app, frame, panes[2]);
}

fn render_single_pane(app: &mut App, frame: &mut Frame, area: Rect) {
    if matches!(app.tab, DashboardTab::Commands) {
        // On narrow screens when on Commands tab: show menu + drawer stacked
        let halves = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);
        render_action_list(app, frame, halves[0]);
        render_command_details(app, frame, halves[1]);
    } else {
        render_tab_content(app, frame, area);
    }
}

fn render_action_list(app: &App, frame: &mut Frame, area: Rect) {
    let items: Vec<ListItem> = INTERACTIVE_ACTIONS
        .iter()
        .enumerate()
        .map(|(idx, action)| {
            let icon = icon_for_key(action.key);
            let aliases = if action.aliases.is_empty() {
                String::new()
            } else {
                format!(" ({})", action.aliases.join(", "))
            };
            let label = format!("{}) {}{}{}", action.key, icon, action.label, aliases);
            let hint = format!("    {}", action.hint);

            let style = if idx == app.active_index {
                app.theme.selected
            } else {
                app.theme.muted
            };

            ListItem::new(vec![
                Line::from(Span::styled(label, style)),
                Line::from(Span::styled(hint, app.theme.muted)),
            ])
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().title("Menu").borders(Borders::NONE))
        .highlight_style(app.theme.selected);

    let mut state = ListState::default();
    state.select(Some(app.active_index));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_command_details(app: &App, frame: &mut Frame, area: Rect) {
    let selected = INTERACTIVE_ACTIONS
        .get(app.active_index)
        .unwrap_or(&INTERACTIVE_ACTIONS[0]);
    let text = format!(
        "Command details\n\nSelected: {}\nSummary: {}\nExample: {}\nCost: {}",
        selected.label, selected.summary, selected.example, selected.cost_hint
    );
    let para = Paragraph::new(text)
        .style(app.theme.muted)
        .wrap(Wrap { trim: true });
    frame.render_widget(para, area);
}

fn render_tab_content(app: &mut App, frame: &mut Frame, area: Rect) {
    match app.tab {
        DashboardTab::Commands => render_command_details(app, frame, area),
        DashboardTab::Output => render_output(app, frame, area),
        DashboardTab::Help => render_help(app, frame, area),
    }
}

fn render_output(app: &App, frame: &mut Frame, area: Rect) {
    let viewport = area.height.saturating_sub(8) as usize; // reserve header lines
    let (lines, start, end, total) = app.output_visible_lines(viewport.max(1));

    let from = if total == 0 { 0 } else { start + 1 };
    let to = if total == 0 { 0 } else { end };
    let filter_text = if app.output_search.trim().is_empty() {
        "(none)".to_string()
    } else {
        app.output_search.trim().to_string()
    };

    let mut para_lines: Vec<Line> = vec![
        Line::from(Span::styled("Last run", app.theme.accent)),
        Line::from(""),
        Line::from(Span::styled(
            format!("phase:   {}", app.phase_badge()),
            app.theme.muted,
        )),
        Line::from(Span::styled(
            format!(
                "command: {}",
                app.session.last_command.as_deref().unwrap_or("-")
            ),
            app.theme.muted,
        )),
        Line::from(Span::styled(
            format!(
                "status:  {}",
                app.session.last_status.as_deref().unwrap_or("-")
            ),
            app.theme.muted,
        )),
        Line::from(Span::styled(
            format!("filter:  {filter_text}"),
            app.theme.muted,
        )),
        Line::from(""),
        Line::from(Span::styled("output:", app.theme.muted)),
    ];

    if lines.is_empty() {
        para_lines.push(Line::from(Span::styled(
            "(no output lines for current filter)",
            app.theme.muted,
        )));
    } else {
        for line in &lines {
            para_lines.push(Line::from(line.to_string()));
        }
    }

    para_lines.push(Line::from(""));
    para_lines.push(Line::from(Span::styled(
        format!(
            "view {}-{} of {} | offset {}",
            from, to, total, app.output_offset
        ),
        app.theme.muted,
    )));

    let para = Paragraph::new(para_lines).wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn render_help(app: &App, frame: &mut Frame, area: Rect) {
    let para = Paragraph::new(HELP_TEXT)
        .style(app.theme.muted)
        .wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn render_status_bar(app: &App, frame: &mut Frame, area: Rect) {
    let selected = INTERACTIVE_ACTIONS
        .get(app.active_index)
        .unwrap_or(&INTERACTIVE_ACTIONS[0]);
    let focus = if app.input_mode != InputMode::None {
        match &app.input_mode {
            InputMode::ActionParam { label, .. } => format!("input:{label}"),
            InputMode::Filter => "input:filter".to_string(),
            InputMode::Palette => "input:palette".to_string(),
            InputMode::None => unreachable!(),
        }
    } else {
        format!("tab:{}", app.tab.label())
    };
    let status = app.session.last_status.as_deref().unwrap_or("-");
    let text = format!(
        " {} {}:{} | {} | {} ",
        app.phase_badge(),
        selected.key,
        selected.label,
        focus,
        status
    );
    let para = Paragraph::new(text).style(app.theme.status_bar);
    frame.render_widget(para, area);
}

fn render_keybindings(app: &App, frame: &mut Frame, area: Rect) {
    let hints: Vec<Span> = match app.phase() {
        UiPhase::Input => vec![
            Span::styled("<Enter>", app.theme.keybinding_key),
            Span::styled(" Submit  ", app.theme.keybinding_desc),
            Span::styled("<Esc>", app.theme.keybinding_key),
            Span::styled(" Cancel", app.theme.keybinding_desc),
        ],
        UiPhase::Running => vec![
            Span::styled("<Ctrl+C>", app.theme.keybinding_key),
            Span::styled(" Cancel  ", app.theme.keybinding_desc),
            Span::styled("<Tab>", app.theme.keybinding_key),
            Span::styled(" Switch tabs", app.theme.keybinding_desc),
        ],
        _ => vec![
            Span::styled("<Enter>", app.theme.keybinding_key),
            Span::styled(" Run  ", app.theme.keybinding_desc),
            Span::styled("<Tab>", app.theme.keybinding_key),
            Span::styled(" Switch  ", app.theme.keybinding_desc),
            Span::styled("</>", app.theme.keybinding_key),
            Span::styled(" Search  ", app.theme.keybinding_desc),
            Span::styled("<f>", app.theme.keybinding_key),
            Span::styled(" Filter  ", app.theme.keybinding_desc),
            Span::styled("<q>", app.theme.keybinding_key),
            Span::styled(" Quit", app.theme.keybinding_desc),
        ],
    };

    let line = Line::from(hints);
    let para = Paragraph::new(line);
    frame.render_widget(para, area);
}

fn render_input_overlay(app: &App, frame: &mut Frame, area: Rect) {
    // Render a simple prompt line at the bottom of the body area
    let prompt_area = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(3),
        width: area.width,
        height: 3,
    };

    let label = match &app.input_mode {
        InputMode::Filter => "Output search (blank clears): ".to_string(),
        InputMode::Palette => "Palette (/): ".to_string(),
        InputMode::ActionParam {
            label, previous, ..
        } => {
            if let Some(prev) = previous {
                if !prev.is_empty() {
                    format!("{label} [{prev}]: ")
                } else {
                    format!("{label}: ")
                }
            } else {
                format!("{label}: ")
            }
        }
        InputMode::None => String::new(),
    };

    let prompt_line = format!("{label}{}_", app.input_value);
    let overlay_block = Block::default()
        .borders(Borders::TOP)
        .border_style(app.theme.accent)
        .title(Span::styled(" Input ", app.theme.accent));
    let inner = overlay_block.inner(prompt_area);
    frame.render_widget(overlay_block, prompt_area);

    let prompt_para =
        Paragraph::new(prompt_line).style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_widget(prompt_para, inner);
}

fn icon_for_key(key: &str) -> &'static str {
    match key {
        "1" => "⌕ ",
        "2" => "◍ ",
        "3" => "◉ ",
        "4" => "↳ ",
        "5" => "✦ ",
        "6" => "? ",
        _ => "",
    }
}
