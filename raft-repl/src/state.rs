use clap::Parser;
use hyper::client::HttpConnector;
use raft_common::client::{ApiClient, RaftClient};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::prelude::{Alignment, Direction};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, List, ListItem, ListState, Row, Table, TableState};
use ratatui::Frame;
use ratatui_textarea::{Input, Key, TextArea};
use std::io::StdoutLock;
use std::sync::mpsc;

use crate::command::{Command, Commands, Spawn};
use crate::events::{Notification, NotificationType, ReplEvent};
use crate::node::Node;
use crate::ui::popup::Popup;

pub struct State {
    mailbox: mpsc::Sender<ReplEvent>,
    view: View,
    events: Vec<ListItem<'static>>,
    events_state: ListState,
    nodes: Vec<Node>,
    nodes_state: TableState,
    popup: Popup,
}

impl State {
    pub fn new(mailbox: mpsc::Sender<ReplEvent>) -> Self {
        Self {
            mailbox,
            view: View::new(),
            events: vec![],
            events_state: ListState::default(),
            nodes: vec![],
            nodes_state: TableState::default(),
            popup: Popup::new(),
        }
    }

    pub fn draw(&mut self, frame: &mut Frame<CrosstermBackend<StdoutLock>>) {
        let main_chunks = self.view.main_layout.split(frame.size());
        let content_chunks = self.view.content_layout.split(main_chunks[0]);
        let mut rows = Vec::new();

        for (idx, node) in self.nodes.iter().enumerate() {
            let mut cells = Vec::new();

            cells.push(Cell::from(format!("{} -> localhost:{}", idx, node.port())));
            cells.push(Cell::from("0"));
            cells.push(Cell::from("-"));

            rows.push(Row::new(cells));
        }

        let node_table = Table::new(rows)
            .header(self.view.node_table_header.clone())
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title_alignment(Alignment::Right)
                    .title("Nodes"),
            )
            .widths(&[
                Constraint::Percentage(33),
                Constraint::Percentage(33),
                Constraint::Percentage(33),
            ]);

        frame.render_stateful_widget(node_table, content_chunks[0], &mut self.nodes_state);

        let event_list = List::new(self.events.clone()).block(
            Block::new()
                .title("Events")
                .borders(Borders::ALL)
                .title_alignment(Alignment::Right),
        );

        frame.render_stateful_widget(event_list, content_chunks[1], &mut self.events_state);
        frame.render_widget(self.view.shell.widget(), main_chunks[1]);
        if self.popup.shown {
            self.popup.draw(frame);
        }
    }

    pub fn on_input(&mut self, input: Input) {
        if self.popup.shown {
            return;
        }

        self.view.shell.input(input);
    }

    pub fn on_command(&mut self) -> bool {
        if self.popup.shown {
            self.popup.shown = false;
            return true;
        }

        let lines = self.view.shell.lines();
        if !empty_line_cmd(lines) {
            let mut tokens = vec![" "];
            tokens.extend(lines[0].as_str().split(" "));
            match Commands::try_parse_from(tokens) {
                Err(e) => {
                    self.popup.shown = true;
                    self.popup.set_title("Command error");
                    self.popup.set_text(e.to_string());
                }

                Ok(cmd) => match cmd.command {
                    Command::Quit | Command::Exit => return false,
                    Command::Spawn(args) => {
                        if let Err(e) = self.spawn_cluster(args) {
                            self.popup.shown = true;
                            self.popup.set_title("Error");
                            self.popup.set_text(e.to_string());
                        }
                    }
                },
            }
        }

        self.view.shell.delete_line_by_head();
        true
    }

    pub fn on_notification(&mut self, event: Notification) {
        let color = match event.r#type {
            NotificationType::Positive => Color::default(),
            NotificationType::Negative => Color::Red,
        };

        self.events
            .push(ListItem::new(event.msg).style(Style::default().fg(color)));
    }

    pub fn spawn_cluster(&mut self, args: Spawn) -> eyre::Result<()> {
        if !self.nodes.is_empty() {
            eyre::bail!("You already have an active cluster configuration. Shut it down first!");
        }

        let mut starting_port = 2_113;

        for idx in 0..args.count {
            self.nodes
                .push(Node::new(self.mailbox.clone(), starting_port + idx)?);

            let _ = self
                .mailbox
                .send(ReplEvent::msg(format!("Node {} is online", idx)));
        }

        Ok(())
    }
}

fn empty_line_cmd(lines: &[String]) -> bool {
    lines.is_empty() || lines[0].is_empty()
}

struct View {
    shell: TextArea<'static>,
    main_layout: Layout,
    content_layout: Layout,
    node_table_header: Row<'static>,
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
            .constraints([Constraint::Min(1), Constraint::Length(50)]);

        Self {
            shell,
            main_layout,
            content_layout,
            node_table_header: Row::new([
                Cell::from("Id").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("Term").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("State").style(Style::default().add_modifier(Modifier::BOLD)),
            ])
            .height(1)
            .style(Style::default().add_modifier(Modifier::REVERSED)),
        }
    }
}
