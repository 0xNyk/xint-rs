mod app;
mod events;
mod theme;
mod ui;

use std::io::{self, BufRead, BufReader, IsTerminal};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::DefaultTerminal;

use crate::cli::PolicyMode;
use crate::commands::tui_adapter::build_tui_execution_plan;
use crate::policy;

use app::{App, PendingAction};
use events::{map_key_event, Message};

pub async fn run(policy_mode: PolicyMode) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return run_non_interactive(policy_mode);
    }

    let mut terminal = ratatui::init();
    let result = run_app(&mut terminal, policy_mode);
    ratatui::restore();
    result
}

fn run_non_interactive(policy_mode: PolicyMode) -> Result<()> {
    use crate::commands::actions::{normalize_interactive_choice, INTERACTIVE_ACTIONS};
    use std::io::Write;

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

    print!("\nSelect option (number or alias): ");
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    let choice = buf.trim();

    let Some(key) = normalize_interactive_choice(choice) else {
        eprintln!("Unknown option: {choice}");
        return Ok(());
    };

    if key == "0" {
        return Ok(());
    }

    let plan_result = build_tui_execution_plan(key, None);
    let Some(plan) = plan_result.data else {
        eprintln!("{}", plan_result.message);
        return Ok(());
    };

    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg("--policy").arg(policy::as_str(policy_mode));
    for arg in &plan.args {
        cmd.arg(arg);
    }
    let status = cmd.status()?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn run_app(terminal: &mut DefaultTerminal, policy_mode: PolicyMode) -> Result<()> {
    let mut app = App::new();

    loop {
        terminal.draw(|frame| ui::render(&mut app, frame))?;

        if app.should_quit {
            break;
        }

        // Poll with a short timeout so the hero animation ticks even without input
        if event::poll(Duration::from_millis(90))? {
            match event::read()? {
                Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                    let msg = map_key_event(key_event, &app);
                    app.update(msg);
                }
                Event::Resize(_, _) => {
                    // ratatui redraws on next loop iteration automatically
                }
                _ => {}
            }
        }

        // Handle any pending action produced by update()
        if let Some(pending) = app.pending.take() {
            match pending {
                PendingAction::Quit => break,
                PendingAction::ExecuteAction { action_key, value } => {
                    execute_action(terminal, &mut app, &action_key, &value, policy_mode)?;
                }
            }
        }
    }

    Ok(())
}

fn execute_action(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    action_key: &str,
    value: &str,
    policy_mode: PolicyMode,
) -> Result<()> {
    let plan_result = build_tui_execution_plan(
        action_key,
        if value.is_empty() { None } else { Some(value) },
    );
    let Some(plan) = plan_result.data else {
        app.session.last_status = Some(plan_result.message);
        return Ok(());
    };

    app.session.last_command = Some(plan.command.clone());
    app.session.last_output_lines.clear();
    app.output_offset = 0;
    app.tab = app::DashboardTab::Output;

    run_subcommand(terminal, app, &plan.args, policy_mode)
}

fn run_subcommand(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    args: &[String],
    policy_mode: PolicyMode,
) -> Result<()> {
    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg("--policy").arg(policy::as_str(policy_mode));
    for arg in args {
        cmd.arg(arg);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    app.is_running = true;

    let (tx, rx) = mpsc::channel::<String>();

    if let Some(stdout) = child.stdout.take() {
        let tx_out = tx.clone();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                let _ = tx_out.send(line);
            }
        });
    }

    if let Some(stderr) = child.stderr.take() {
        let tx_err = tx.clone();
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                let _ = tx_err.send(format!("[stderr] {line}"));
            }
        });
    }

    drop(tx);

    let spinner_frames = ["|", "/", "-", "\\"];
    let mut spinner_index = 0usize;

    let status = loop {
        // Drain output lines first
        while let Ok(line) = rx.try_recv() {
            app.append_output(line);
        }

        // Update spinner badge
        app.session.last_status = Some(format!(
            "running {}",
            spinner_frames[spinner_index % spinner_frames.len()]
        ));
        spinner_index += 1;

        terminal.draw(|frame| ui::render(app, frame))?;

        if let Some(exit_status) = child.try_wait()? {
            break exit_status;
        }

        // Poll for Ctrl+C / tab switching during run
        if event::poll(Duration::from_millis(90))? {
            if let Ok(Event::Key(key_event)) = event::read() {
                if key_event.kind == KeyEventKind::Press {
                    let msg = map_key_event(key_event, app);
                    if matches!(msg, Message::CancelRunning) {
                        let _ = child.kill();
                        break child.wait()?;
                    }
                    app.update(msg);
                }
            }
        }
    };

    // Drain remaining output after process exits
    while let Ok(line) = rx.try_recv() {
        app.append_output(line);
    }

    app.is_running = false;
    app.session.last_status = if status.success() {
        Some("success".to_string())
    } else {
        Some(format!(
            "failed (exit {})",
            status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".to_string())
        ))
    };

    Ok(())
}
