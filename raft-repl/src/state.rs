use std::io::Stdout;
use tui::backend::CrosstermBackend;
use tui::Frame;

pub struct State {}

impl State {
    pub fn new() -> Self {
        Self {}
    }

    pub fn draw(&self, frame: &mut Frame<CrosstermBackend<Stdout>>) {}
}
