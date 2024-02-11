use crate::command::{AppendToStream, ReadStream};
use crate::events::ReplEvent;
use crate::handler::CommandHandler;
use bytes::Bytes;
use names::Generator;
use raft_common::StatusResp;
use std::sync::mpsc;
use std::time::Duration;
use tokio::process::Child;
use tokio::runtime::Handle;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;
use tokio::time::timeout;

#[derive(Clone)]
pub enum Connectivity {
    Online(StatusResp),
    Offline,
}

impl Connectivity {
    pub fn is_offline(&self) -> bool {
        if let Connectivity::Offline = self {
            return true;
        }

        false
    }

    pub fn is_online(&self) -> bool {
        !self.is_offline()
    }

    pub fn status(&self) -> Option<String> {
        match self {
            Connectivity::Online(r) => Some(r.status.clone()),
            Connectivity::Offline => None,
        }
    }
}

pub enum ProcKind {
    Managed(raft_server::Node),
    External(ExternalProc),
    #[allow(dead_code)]
    Spawn(Child),
}

impl ProcKind {
    pub fn managed(node: raft_server::Node) -> Self {
        Self::Managed(node)
    }

    pub fn external() -> Self {
        Self::External(ExternalProc)
    }

    #[allow(dead_code)]
    pub fn spawn(child: Child) -> Self {
        Self::Spawn(child)
    }
}

pub struct ExternalProc;

pub struct Node {
    handle: Handle,
    port: u16,
    seeds: Vec<u16>,
    connectivity: Connectivity,
    name_gen: Generator<'static>,
    local_mailbox: UnboundedSender<NodeCmd>,
}

impl Node {
    pub fn new(
        idx: usize,
        handle: Handle,
        mailbox: mpsc::Sender<ReplEvent>,
        port: u16,
        seeds: Vec<u16>,
    ) -> eyre::Result<Self> {
        let name_gen = Generator::default();
        let (local_mailbox, local_receiver) = unbounded_channel();
        let mut node = Self {
            connectivity: Connectivity::Offline,
            seeds,
            port,
            name_gen,
            handle: handle.clone(),
            local_mailbox,
        };

        let cloned_handle = handle.clone();
        handle.spawn(node_command_handler(
            cloned_handle,
            idx,
            mailbox,
            local_receiver,
        ));

        node.start();

        Ok(node)
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn connectivity(&self) -> &Connectivity {
        &self.connectivity
    }

    pub fn set_connectivity(&mut self, connectivity: Connectivity) {
        self.connectivity = connectivity;
    }

    pub fn stop(&mut self) {
        let _ = self.local_mailbox.send(NodeCmd::Stop);
    }

    pub fn start(&mut self) {
        let _ = self.local_mailbox.send(NodeCmd::Start {
            port: self.port as u16,
            seeds: self.seeds.clone(),
            r#type: ProcType::Managed,
        });
    }

    pub fn ping(&self) {
        let _ = self.local_mailbox.send(NodeCmd::Ping);
    }

    pub fn append_to_stream(&mut self, args: AppendToStream) {
        let stream_name = if let Some(name) = args.stream {
            name
        } else {
            self.name_gen.next().unwrap()
        };
        let prop_name = self.name_gen.next().unwrap();
        let value_name = self.name_gen.next().unwrap();
        let payload = serde_json::json!({
            prop_name: value_name,
        });
        let events = vec![Bytes::from(serde_json::to_vec(&payload).unwrap())];

        let _ = self.local_mailbox.send(NodeCmd::AppendStream {
            stream_name,
            events,
        });
    }

    pub fn read_stream(&mut self, args: ReadStream) {
        let _ = self.local_mailbox.send(NodeCmd::ReadStream {
            stream_name: args.stream,
        });
    }

    pub fn cleanup(self) {
        let (sender, receive) = oneshot::channel();
        let _ = self.local_mailbox.send(NodeCmd::Cleanup(sender));
        let _ = self.handle.block_on(receive);
    }
}

pub enum ProcType {
    Managed,
    Binary,
    External,
}

enum NodeCmd {
    AppendStream {
        stream_name: String,
        events: Vec<Bytes>,
    },

    ReadStream {
        stream_name: String,
    },

    Ping,

    Start {
        port: u16,
        seeds: Vec<u16>,
        r#type: ProcType,
    },

    Stop,

    Cleanup(oneshot::Sender<()>),
}

async fn node_command_handler(
    handle: Handle,
    node: usize,
    mailbox: mpsc::Sender<ReplEvent>,
    mut receiver: UnboundedReceiver<NodeCmd>,
) {
    let mut handler = CommandHandler::new(node, mailbox);

    loop {
        if let Ok(msg) = timeout(Duration::from_secs(1), receiver.recv()).await {
            let msg = if let Some(msg) = msg {
                msg
            } else {
                break;
            };

            match msg {
                NodeCmd::AppendStream {
                    stream_name,
                    events,
                } => handler.append_stream(stream_name, events).await,

                NodeCmd::ReadStream { stream_name } => handler.read_stream(stream_name).await,

                NodeCmd::Start {
                    port,
                    seeds,
                    r#type,
                } => handler.start(&handle, port, seeds, r#type).await,

                NodeCmd::Stop => handler.stop().await,

                NodeCmd::Ping => handler.ping().await,

                NodeCmd::Cleanup(complete) => {
                    handler.cleanup().await;
                    let _ = complete.send(());
                    break;
                }
            }
        } else {
            handler.status().await;
        }
    }
}
