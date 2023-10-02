use crate::events::ReplEvent;
use crossterm::event::{Event, KeyCode};
use std::sync::mpsc;
use std::thread::spawn;
use tui_textarea::{Input, Key};

pub fn input_process(mut mailbox: mpsc::Sender<ReplEvent>) {
    spawn(move || {
        loop {
            let input: Input = crossterm::event::read()?.into();

            if mailbox.send(ReplEvent::Input(input)).is_err() {
                break;
            }
        }

        Ok::<_, crossterm::ErrorKind>(())
    });
}
