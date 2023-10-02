use crate::events::ReplEvent;
use std::sync::mpsc;
use std::thread::spawn;
use tui_textarea::Input;

pub fn input_process(mailbox: mpsc::Sender<ReplEvent>) {
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
