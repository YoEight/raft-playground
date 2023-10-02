use std::io::{Stdout, StdoutLock};
use tui::backend::CrosstermBackend;
use tui::layout::{Constraint, Layout};
use tui::style::Style;
use tui::widgets::{Block, Borders};
use tui::Frame;
use tui_textarea::{Input, TextArea};

pub struct State {
    view: View,
}

impl State {
    pub fn new() -> Self {
        Self { view: View::new() }
    }

    pub fn draw(&self, frame: &mut Frame<CrosstermBackend<StdoutLock>>) {
        self.view.draw(frame);
    }

    pub fn on_input(&mut self, input: Input) {
        self.view.shell.input(input);
    }

    pub fn on_command(&mut self) -> bool {
        let lines = self.view.shell.lines();
        if !lines.is_empty() {
            match lines[0].as_str() {
                "quit" | "exit" => return false,
                _ => {}
            }
        }

        self.view.shell.delete_line_by_head();
        true
    }
}

struct View {
    shell: TextArea<'static>,
    main_layout: Layout,
}

impl View {
    pub fn new() -> Self {
        let mut shell = TextArea::default();
        shell.set_cursor_line_style(Style::default());
        shell.set_block(Block::default().borders(Borders::ALL));

        let mut main_layout =
            Layout::default().constraints([Constraint::Min(1), Constraint::Length(3)]);

        Self { shell, main_layout }
    }

    pub fn draw(&self, frame: &mut Frame<CrosstermBackend<StdoutLock>>) {
        let chunks = self.main_layout.split(frame.size());

        frame.render_widget(self.shell.widget(), chunks[1]);
    }
}
