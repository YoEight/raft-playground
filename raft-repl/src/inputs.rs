use crate::events::ReplEvent;
use std::sync::mpsc;
use std::thread::spawn;
use std::time::{Duration, Instant};

pub fn input_process(mut mailbox: mpsc::Sender<ReplEvent>) {
    let tick_rate = Duration::from_millis(100);

    spawn(move || {
        loop {
            if crossterm::event::poll(tick_rate)? {
                let event = crossterm::event::read()?;
                if mailbox.send(ReplEvent::CrossTermEvent(event)).is_err() {
                    break;
                }
            }
        }

        Ok::<_, crossterm::ErrorKind>(())
    });
}
