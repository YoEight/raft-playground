use crate::command::{AppendToStream, ReadStream};
use crate::data::RecordedEvent;
use crate::events::ReplEvent;
use crate::handler::CommandHandler;
use bytes::Bytes;
use hyper::client::HttpConnector;
use hyper::Client;
use names::Generator;
use raft_common::client::ApiClient;
use raft_common::{AppendReq, NodeId, ReadReq, StatusResp};
use raft_server::options::Options;
use std::process::Stdio;
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::runtime::Handle;
use tokio::sync::mpsc::{unbounded_channel, Receiver, UnboundedReceiver, UnboundedSender};
use tokio::sync::Mutex;
use tokio::time::timeout;
use tonic::body::BoxBody;
use tonic::codegen::tokio_stream::StreamExt;
use tonic::Request;
use tracing::{error, info};
use uuid::Uuid;

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

struct Proc {
    id: Uuid,
    host: String,
    port: u16,
    api_client: ApiClient<Client<HttpConnector, BoxBody>>,
    kind: ProcKind,
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
    idx: usize,
    port: usize,
    seeds: Vec<usize>,
    connectivity: Connectivity,
    handle: Handle,
    mailbox: mpsc::Sender<ReplEvent>,
    proc: Arc<Mutex<Option<Proc>>>,
    name_gen: Generator<'static>,
    local_mailbox: UnboundedSender<NodeCmd>,
}

impl Node {
    pub fn new(
        idx: usize,
        handle: Handle,
        mailbox: mpsc::Sender<ReplEvent>,
        port: usize,
        seeds: Vec<usize>,
    ) -> eyre::Result<Self> {
        let name_gen = Generator::default();
        let (local_mailbox, _) = unbounded_channel();
        let mut node = Self {
            idx,
            connectivity: Connectivity::Offline,
            handle,
            mailbox,
            port,
            seeds,
            name_gen,
            proc: Arc::new(Mutex::new(None)),
            local_mailbox,
        };

        node.start();

        Ok(node)
    }

    pub fn port(&self) -> usize {
        self.port
    }

    pub fn connectivity(&self) -> &Connectivity {
        &self.connectivity
    }

    pub fn set_connectivity(&mut self, connectivity: Connectivity) {
        self.connectivity = connectivity;
    }

    /// Indicates if the node was started outside the REPL process.
    pub fn is_external(&self) -> Option<bool> {
        let proc_ref = self.proc.clone();
        self.handle.block_on(async move {
            let proc = proc_ref.lock().await;

            if let Some(proc) = proc.as_ref() {
                let is_external = if let ProcKind::External(_) = proc.kind {
                    true
                } else {
                    false
                };

                return Some(is_external);
            }

            None
        })
    }

    pub fn stop(&mut self) {
        let proc = self.proc.clone();
        self.handle.spawn(async move {
            let mut proc = proc.lock().await;
            if let Some(mut proc_handle) = proc.take() {
                if let ProcKind::Managed(managed) = &mut proc_handle.kind {
                    if let Some(join) = managed.join.take() {
                        join.abort();
                        *proc = Some(proc_handle);
                    }
                }
            }
        });
    }

    pub fn start(&mut self) {
        let mailbox = self.mailbox.clone();
        let port = self.port;
        let idx = self.idx;
        let proc_ref = self.proc.clone();
        let seeds = self
            .seeds
            .iter()
            .copied()
            .map(|i| i as u16)
            .collect::<Vec<_>>();

        let handle = self.handle.clone();
        self.handle.spawn(async move {
            match spawn_managed_node(&handle, port as u16, seeds) {
                Err(e) => {
                    error!("node_{}:{} error when starting: {}", "localhost", port, e);
                    let _ = mailbox.send(ReplEvent::error(format!(
                        "Error when starting node {}: {}",
                        idx, e
                    )));
                }

                Ok(kind) => {
                    let mut proc = proc_ref.lock().await;
                    let mut connector = HttpConnector::new();

                    connector.enforce_http(false);
                    let client = hyper::Client::builder().http2_only(true).build(connector);
                    let uri = hyper::Uri::from_maybe_shared(format!("http://localhost:{}", port))
                        .unwrap();
                    let api_client = ApiClient::with_origin(client, uri);
                    let id = Uuid::new_v4();

                    *proc = Some(Proc {
                        id,
                        port: port as u16,
                        host: "localhost".to_string(),
                        api_client: api_client.clone(),
                        kind,
                    });

                    info!("node_{}:{} started", "localhost", port);
                    let _ = mailbox.send(ReplEvent::msg(format!("Node {} is starting", idx)));
                    spawn_healthcheck_process(id, idx, &handle, mailbox, proc_ref.clone());
                }
            }
        });
    }

    pub fn restart(&self) {
        /*let proc_ref = self.proc.clone();
        self.handle.spawn(async move {
            let mut proc = proc_ref.lock().await;

            if let Some(proc) = proc.as_mut() {
                match proc.kind {
                    ProcKind::Managed(_) => {}
                    ProcKind::External(_) => {}
                    ProcKind::Spawn(_) => {}
                }
            }
        });*/
    }

    pub fn ping(&self) {
        let _ = self.local_mailbox.send(NodeCmd::Ping);
    }

    pub fn append_to_stream(&mut self, args: AppendToStream) {
        let node_id = self.idx;
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
        self.handle.block_on(async move {
            let mut proc = self.proc.lock().await;

            if let Some(proc) = proc.take() {
                match proc.kind {
                    ProcKind::Managed(mut managed) => {
                        if let Some(join) = managed.join.take() {
                            join.abort();
                        }
                    }

                    ProcKind::Spawn(mut child) => {
                        let _ = child.kill();
                    }

                    _ => {}
                }
            }
        });
    }
}

fn spawn_managed_node(handle: &Handle, port: u16, seeds: Vec<u16>) -> eyre::Result<ProcKind> {
    let mut node = raft_server::Node::new(
        handle.clone(),
        Options {
            port: port as u16,
            seeds,
        },
    )?;

    node.start();

    Ok(ProcKind::managed(node))
}

#[allow(dead_code)]
fn spawn_compiled_node(port: u16, seeds: Vec<u16>) -> eyre::Result<ProcKind> {
    let seeds = seeds
        .iter()
        .copied()
        .flat_map(|p| vec!["--seed".to_string(), p.to_string()])
        .collect::<Vec<_>>();

    let child = Command::new("cargo")
        .arg("run")
        .arg("-p")
        .arg("raft-server")
        .arg("--")
        .arg("--port")
        .arg(port.to_string())
        .args(seeds)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    Ok(ProcKind::spawn(child))
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
}

async fn node_command_handler(
    node: usize,
    port: u16,
    mailbox: mpsc::Sender<ReplEvent>,
    mut receiver: UnboundedReceiver<NodeCmd>,
) {
    let mut handler = CommandHandler::new(node, port, mailbox);

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
                } => handler.start(port, seeds, ProcType::Managed).await,
                NodeCmd::Ping => handler.ping().await,
            }
        } else {
            handler.status().await;
        }
    }
}

fn spawn_healthcheck_process(
    id: Uuid,
    node: usize,
    handle: &Handle,
    mailbox: mpsc::Sender<ReplEvent>,
    proc_ref: Arc<Mutex<Option<Proc>>>,
) {
    handle.spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        let mut state = Connectivity::Offline;
        let mut last_term = 0;

        loop {
            ticker.tick().await;
            let mut proc = proc_ref.lock().await;

            if proc.is_none() {
                break;
            }

            let inner = proc.as_mut().unwrap();
            if id != inner.id {
                break;
            }

            match inner.api_client.status(Request::new(())).await {
                Err(e) => {
                    error!(
                        "node_{}:{} error when requesting status: {}",
                        inner.host, inner.port, e
                    );

                    if state.is_online() {
                        let _ = mailbox.send(ReplEvent::warn(format!(
                            "Node {} connection error: {}",
                            node,
                            e.message(),
                        )));

                        let _ =
                            mailbox.send(ReplEvent::node_connectivity(node, Connectivity::Offline));
                        state = Connectivity::Offline;
                    }
                }

                Ok(resp) => {
                    let resp = resp.into_inner();
                    info!("node_{}:{} status = {:?}", inner.host, inner.port, resp);

                    if state.is_offline()
                        || last_term != resp.term
                        || state.status() != Some(resp.status.clone())
                    {
                        last_term = resp.term;
                        state = Connectivity::Online(resp);
                        let _ = mailbox.send(ReplEvent::node_connectivity(node, state.clone()));
                    }
                }
            }
        }
    });
}
