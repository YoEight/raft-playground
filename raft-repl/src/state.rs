use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::prelude::{Alignment, Direction};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;
use ratatui_textarea::{Input, TextArea};
use std::io::StdoutLock;

pub struct State {
    view: View,
    events: Vec<ListItem<'static>>,
    events_state: ListState,
}

impl State {
    pub fn new() -> Self {
        Self {
            view: View::new(),
            events: vec![],
            events_state: ListState::default(),
        }
    }

    pub fn draw(&mut self, frame: &mut Frame<CrosstermBackend<StdoutLock>>) {
        let main_chunks = self.view.main_layout.split(frame.size());
        let content_chunks = self.view.content_layout.split(main_chunks[0]);
        let event_list = List::new(self.events.clone()).block(
            Block::new()
                .title("Events")
                .borders(Borders::ALL)
                .title_alignment(Alignment::Right),
        );

        frame.render_stateful_widget(event_list, content_chunks[1], &mut self.events_state);
        frame.render_widget(self.view.shell.widget(), main_chunks[1]);
    }

    pub fn on_input(&mut self, input: Input) {
        self.view.shell.input(input);
    }

    pub fn on_command(&mut self) -> bool {
        let lines = self.view.shell.lines();
        if !lines.is_empty() {
            match lines[0].as_str() {
                "quit" | "exit" => return false,
                line => {
                    // TODO - we store anything else as
                    self.events.push(ListItem::new(line.to_string()));
                }
            }
        }

        self.view.shell.delete_line_by_head();
        true
    }
}

struct View {
    shell: TextArea<'static>,
    main_layout: Layout,
    content_layout: Layout,
}

impl View {
    pub fn new() -> Self {
        let mut shell = TextArea::default();
        shell.set_cursor_line_style(Style::default());
        shell.set_cursor_style(
            Style::default()
                .add_modifier(Modifier::UNDERLINED)
                .add_modifier(Modifier::RAPID_BLINK),
        );
        shell.set_block(Block::default().borders(Borders::ALL));

        let main_layout =
            Layout::default().constraints([Constraint::Min(1), Constraint::Length(3)]);

        let content_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(30)]);

        Self {
            shell,
            main_layout,
            content_layout,
        }
    }
}
