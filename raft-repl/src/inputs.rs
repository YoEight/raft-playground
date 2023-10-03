use crate::events::ReplEvent;
use std::io;
use std::sync::mpsc;
use std::thread::spawn;

pub fn input_process(mailbox: mpsc::Sender<ReplEvent>) {
    spawn(move || {
        loop {
            let input = crossterm::event::read()?.into();

            if mailbox.send(ReplEvent::Input(input)).is_err() {
                break;
            }
        }

        Ok::<_, io::Error>(())
    });
}
