mod command;
mod data;
mod events;
mod handler;
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
use std::io;
use std::io::StdoutLock;
use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};
use tracing_appender::rolling;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;

fn main() -> eyre::Result<()> {
    let console_layer = console_subscriber::spawn();
    let logs =
        rolling::daily("./logs", "repl.txt").with_filter(|m| m.target().starts_with("raft_repl"));

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_file(false)
        .with_ansi(false)
        .with_writer(logs);

    let (sender, mailbox) = mpsc::channel();
    let subscriber = Registry::default().with(console_layer).with(fmt_layer);

    tracing::subscriber::set_global_default(subscriber).unwrap();

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
                ReplEvent::Input(input) => {
                    if !state.on_input(input) {
                        break;
                    }
                }

                ReplEvent::Notification(event) => {
                    state.on_notification(event);
                }

                ReplEvent::NodeStatusReceived(event) => {
                    state.on_node_connectivity_changed(event);
                }

                ReplEvent::StreamRead(event) => {
                    state.on_node_stream_read(event);
                }

                ReplEvent::StatusRead(event) => {
                    state.on_node_status(event);
                }
            },
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }

    Ok(())
}
