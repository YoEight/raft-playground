use clap::Parser;
use hyper::client::HttpConnector;
use raft_common::client::ApiClient;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::prelude::{Alignment, Direction};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, List, ListItem, ListState, Row, Table, TableState};
use ratatui::Frame;
use ratatui_textarea::{CursorMove, Input, Key, TextArea};
use std::io::StdoutLock;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;
use tokio::runtime::Runtime;
use tonic::Request;

use crate::command::{Command, Commands, Ping, PingCommand, SendEvent, Spawn, Start, Stop};
use crate::events::{NodeConnectivityEvent, Notification, NotificationType, ReplEvent};
use crate::history::History;
use crate::node::{Connectivity, Node};
use crate::persistence::FileBackend;
use crate::ui::popup::Popup;

pub struct State {
    history: History<FileBackend>,
    runtime: Runtime,
    clock: Instant,
    mailbox: mpsc::Sender<ReplEvent>,
    view: View,
    events: Vec<ListItem<'static>>,
    events_state: ListState,
    nodes: Vec<Node>,
    nodes_state: TableState,
    popup: Popup,
    temp_prompt: String,
}

impl State {
    pub fn new(mailbox: mpsc::Sender<ReplEvent>) -> eyre::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        let mut buf = PathBuf::new();
        let user_dirs = directories::UserDirs::new().unwrap();
        buf.push(user_dirs.home_dir());
        buf.push(".raft-repl");

        Ok(Self {
            history: crate::history::file_backed_history(buf.as_path())?,
            runtime,
            mailbox,
            clock: Instant::now(),
            view: View::new(),
            events: vec![],
            events_state: ListState::default(),
            nodes: vec![],
            nodes_state: TableState::default(),
            popup: Popup::new(),
            temp_prompt: String::new(),
        })
    }

    pub fn draw(&mut self, frame: &mut Frame<CrosstermBackend<StdoutLock>>) {
        let main_chunks = self.view.main_layout.split(frame.size());
        let content_chunks = self.view.content_layout.split(main_chunks[0]);
        let mut rows = Vec::new();

        for (idx, node) in self.nodes.iter().enumerate() {
            let mut cells = Vec::new();

            let suffix = if let Some(true) = node.is_external() {
                "(ext)"
            } else {
                ""
            };

            cells.push(Cell::from(format!(
                "{} -> localhost:{} {}",
                idx,
                node.port(),
                suffix
            )));
            match node.connectivity() {
                Connectivity::Online => {
                    cells.push(Cell::from("online").style(Style::default().fg(Color::Green)))
                }
                Connectivity::Offline => {
                    cells.push(Cell::from("offline").style(Style::default().fg(Color::Red)))
                }
            }

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
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
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

        match input.key {
            Key::Up => {
                if self.history.offset_is_head() {
                    self.temp_prompt = self.view.shell.lines()[0].clone();
                }

                if let Some(line) = self.history.prev_entry() {
                    self.view.shell.move_cursor(CursorMove::End);
                    self.view.shell.delete_line_by_head();
                    self.view.shell.insert_str(line.as_str());
                }
            }

            Key::Down => {
                if let Some(line) = self.history.next_entry() {
                    self.view.shell.move_cursor(CursorMove::End);
                    self.view.shell.delete_line_by_head();
                    self.view.shell.insert_str(line.as_str());
                } else if !self.temp_prompt.is_empty() {
                    self.view.shell.move_cursor(CursorMove::End);
                    self.view.shell.delete_line_by_head();
                    self.view.shell.insert_str(self.temp_prompt.as_str());
                }
            }

            _ => {
                self.view.shell.input(input);
            }
        }
    }

    pub fn on_command(&mut self) -> bool {
        if self.popup.shown {
            self.popup.shown = false;
            return true;
        }

        let lines = self.view.shell.lines();
        if !empty_line_cmd(lines) {
            let mut tokens = vec![" "];
            let cmd_str = lines[0].as_str().to_string();
            self.history.push(cmd_str).unwrap();
            tokens.extend(lines[0].as_str().split(" "));
            let cmd = tokens[1];
            match Commands::try_parse_from(tokens) {
                Err(e) => {
                    let title = if cmd == "help" {
                        "Help"
                    } else {
                        "Command error"
                    };

                    self.popup.shown = true;
                    self.popup.set_title(title);
                    self.popup.set_text(e.to_string());
                }

                Ok(cmd) => match cmd.command {
                    Command::Quit | Command::Exit => {
                        self.cleanup();
                        return false;
                    }

                    Command::Spawn(args) => {
                        if let Err(e) = self.spawn_cluster(args) {
                            self.popup.shown = true;
                            self.popup.set_title("Error");
                            self.popup.set_text(e.to_string());
                        }
                    }

                    Command::Stop(args) => {
                        if let Err(e) = self.stop_node(args) {
                            self.popup.shown = true;
                            self.popup.set_title("Error");
                            self.popup.set_text(e.to_string());
                        }
                    }

                    Command::Start(args) => {
                        if let Err(e) = self.start_node(args) {
                            self.popup.shown = true;
                            self.popup.set_title("Error");
                            self.popup.set_text(e.to_string());
                        }
                    }
                    Command::SendEvent(args) => {
                        if let Err(e) = self.send_event_node(args) {
                            self.popup.shown = true;
                            self.popup.set_title("Error");
                            self.popup.set_text(e.to_string());
                        }
                    }

                    Command::Ping(args) => self.ping_node(args),
                },
            }
        }

        self.view.shell.delete_line_by_head();
        true
    }

    pub fn on_previous_history_line(&mut self) {}

    pub fn on_notification(&mut self, event: Notification) {
        self.push_notification(event);
    }

    pub fn on_node_connectivity_changed(&mut self, event: NodeConnectivityEvent) {
        let status = match event.connectivity {
            Connectivity::Online => "online",
            Connectivity::Offline => "offline",
        };

        self.nodes[event.node].set_connectivity(event.connectivity);
        self.push_event(
            Color::default(),
            format!("Node {} is {}", event.node, status),
        );
    }

    fn spawn_cluster(&mut self, args: Spawn) -> eyre::Result<()> {
        if !self.nodes.is_empty() {
            eyre::bail!("You already have an active cluster configuration. Shut it down first!");
        }

        let starting_port = 2_113;
        let mut all_nodes = Vec::new();

        for idx in 0..args.count {
            all_nodes.push(starting_port + idx);
        }

        for (idx, port) in all_nodes.iter().copied().enumerate() {
            let seeds = all_nodes
                .iter()
                .copied()
                .filter(|x| *x != port)
                .collect::<Vec<_>>();

            self.nodes.push(Node::new(
                idx,
                self.runtime.handle().clone(),
                self.mailbox.clone(),
                port,
                seeds,
            )?);
        }

        Ok(())
    }

    fn stop_node(&mut self, args: Stop) -> eyre::Result<()> {
        if args.node >= self.nodes.len() {
            eyre::bail!("Node {} doesn't exist", args.node);
        }

        self.nodes[args.node].stop();

        Ok(())
    }

    fn start_node(&mut self, args: Start) -> eyre::Result<()> {
        if args.external {
            let new_node = Node::new_external(
                args.node,
                self.runtime.handle().clone(),
                self.mailbox.clone(),
                args.port.unwrap_or(2_113),
            );

            if let Some(node) = self.nodes.get_mut(args.node) {
                let mut prev = std::mem::replace(node, new_node);

                prev.stop();
                prev.cleanup();
            } else {
                if args.node > self.nodes.len() {
                    self.nodes.push(new_node);
                } else {
                    self.nodes.insert(args.node, new_node);
                }
            }

            return Ok(());
        }

        if args.node >= self.nodes.len() {
            eyre::bail!("Node {} doesn't exist", args.node);
        }

        self.nodes[args.node].start();

        Ok(())
    }

    fn send_event_node(&mut self, args: SendEvent) -> eyre::Result<()> {
        if args.node >= self.nodes.len() {
            eyre::bail!("Node {} doesn't exist", args.node);
        }

        self.nodes[args.node].send_event()?;

        Ok(())
    }

    fn push_event(&mut self, color: Color, msg: impl AsRef<str>) {
        let dur = self.clock.elapsed();

        self.events.push(
            ListItem::new(format!(
                "t={}.{}s {}",
                dur.as_secs(),
                dur.subsec_millis(),
                msg.as_ref()
            ))
            .style(Style::default().fg(color)),
        );
    }

    fn push_notification(&mut self, event: Notification) {
        let color = match event.r#type {
            NotificationType::Positive => Color::default(),
            NotificationType::Negative => Color::Red,
            NotificationType::Warning => Color::Yellow,
        };

        self.push_event(color, event.msg);
    }

    fn ping_node(&mut self, args: Ping) {
        match args.command {
            PingCommand::External(args) => {
                let mailbox = self.mailbox.clone();
                self.runtime.spawn(async move {
                    let mut connector = HttpConnector::new();
                    connector.enforce_http(false);
                    let hyper_client = hyper::Client::builder().http2_only(true).build(connector);
                    let uri = hyper::Uri::from_maybe_shared(format!(
                        "http://{}:{}",
                        args.host, args.port
                    ))
                    .unwrap();
                    let mut client = ApiClient::with_origin(hyper_client, uri);
                    match client.ping(Request::new(())).await {
                        Err(_) => {
                            let _ = mailbox.send(ReplEvent::error(format!(
                                "Ping {}:{} FAILED",
                                args.host, args.port
                            )));
                        }

                        Ok(_) => {
                            let _ = mailbox.send(ReplEvent::msg(format!(
                                "Ping {}:{} OK",
                                args.host, args.port
                            )));
                        }
                    }
                });
            }
        }
    }

    fn cleanup(&mut self) {
        while let Some(node) = self.nodes.pop() {
            node.cleanup();
        }
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
            .constraints([Constraint::Percentage(60), Constraint::Length(40)]);

        Self {
            shell,
            main_layout,
            content_layout,
            node_table_header: Row::new([
                Cell::from("Id").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("Connectivity").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("Term").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("State").style(Style::default().add_modifier(Modifier::BOLD)),
            ])
            .height(1)
            .style(Style::default().add_modifier(Modifier::REVERSED)),
        }
    }
}
