mod command;
mod events;
mod history;
mod inputs;
mod node;
mod persistence;
mod state;
mod ui;

use crate::events::ReplEvent;
use crate::inputs::input_process;
use crate::state::State;
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, SetTitle,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use ratatui_textarea::{Input, Key};
use std::io;
use std::io::StdoutLock;
use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

fn main() -> eyre::Result<()> {
    let (sender, mailbox) = mpsc::channel();

    input_process(sender.clone());
    enable_raw_mode()?;
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    execute!(stdout, EnterAlternateScreen, SetTitle("raft-playground"))?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let err = app_loop(sender, mailbox, &mut terminal);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = err {
        panic!("Unexpected error: {}", e);
    }

    Ok(())
}

fn app_loop(
    sender: mpsc::Sender<ReplEvent>,
    mailbox: mpsc::Receiver<ReplEvent>,
    terminal: &mut Terminal<CrosstermBackend<StdoutLock>>,
) -> eyre::Result<()> {
    let tick_rate = Duration::from_millis(250);
    let mut last_tick = Instant::now();
    let mut state = State::new(sender)?;

    loop {
        terminal.draw(|frame| state.draw(frame))?;
        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_default();

        match mailbox.recv_timeout(timeout) {
            Err(e) => {
                if let RecvTimeoutError::Disconnected = e {
                    break;
                }
            }

            Ok(event) => match event {
                ReplEvent::Input(input) => match input {
                    Input {
                        key: Key::Enter, ..
                    } => {
                        if !state.on_command() {
                            break;
                        }
                    }
                    input => {
                        state.on_input(input);
                    }
                },

                ReplEvent::Notification(event) => {
                    state.on_notification(event);
                }

                ReplEvent::NodeConnectivityChanged(event) => {
                    state.on_node_connectivity_changed(event);
                }
            },
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }

    Ok(())
}
