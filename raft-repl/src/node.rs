use crate::command::{AppendToStream, ReadStream};
use crate::data::RecordedEvent;
use crate::events::ReplEvent;
use bytes::Bytes;
use hyper::client::HttpConnector;
use hyper::Client;
use names::Generator;
use raft_common::client::ApiClient;
use raft_common::{AppendReq, ReadReq, StatusResp};
use std::process::Stdio;
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::runtime::Handle;
use tokio::sync::Mutex;
use tonic::body::BoxBody;
use tonic::codegen::tokio_stream::StreamExt;
use tonic::Request;
use uuid::Uuid;

#[derive(Clone)]
pub enum Connectivity {
    Online(StatusResp),
    Offline,
}

struct Proc {
    id: Uuid,
    api_client: ApiClient<Client<HttpConnector, BoxBody>>,
    kind: ProcKind,
}

enum ProcKind {
    Managed(ManagedProc),
    External(ExternalProc),
}

impl ProcKind {
    fn managed(child: Child) -> Self {
        Self::Managed(ManagedProc { child })
    }

    fn external() -> Self {
        Self::External(ExternalProc)
    }
}

struct ManagedProc {
    child: Child,
}

struct ExternalProc;

pub struct Node {
    idx: usize,
    port: usize,
    seeds: Vec<usize>,
    connectivity: Connectivity,
    handle: Handle,
    mailbox: mpsc::Sender<ReplEvent>,
    proc: Arc<Mutex<Option<Proc>>>,
    name_gen: Generator<'static>,
}

impl Node {
    pub fn new(
        idx: usize,
        handle: Handle,
        mailbox: mpsc::Sender<ReplEvent>,
        port: usize,
        seeds: Vec<usize>,
    ) -> eyre::Result<Self> {
        let mut connector = HttpConnector::new();

        connector.enforce_http(false);
        let name_gen = Generator::default();
        let mut node = Self {
            idx,
            connectivity: Connectivity::Offline,
            handle,
            mailbox,
            port,
            seeds,
            name_gen,
            proc: Arc::new(Mutex::new(None)),
        };

        node.start();

        Ok(node)
    }

    pub fn new_external(
        idx: usize,
        handle: Handle,
        mailbox: mpsc::Sender<ReplEvent>,
        port: usize,
    ) -> Self {
        let proc_ref = Arc::new(Mutex::new(None));
        let node = Node {
            idx,
            port,
            seeds: vec![],
            connectivity: Connectivity::Offline,
            handle: handle.clone(),
            mailbox: mailbox.clone(),
            proc: proc_ref.clone(),
            name_gen: Default::default(),
        };

        let local_handle = handle.clone();
        handle.spawn(async move {
            let mut proc = proc_ref.lock().await;
            let mut connector = HttpConnector::new();

            connector.enforce_http(false);
            let client = hyper::Client::builder().http2_only(true).build(connector);
            let uri = hyper::Uri::from_maybe_shared(format!("http://localhost:{}", port)).unwrap();
            let api_client = ApiClient::with_origin(client, uri);
            let id = Uuid::new_v4();

            *proc = Some(Proc {
                id,
                api_client: api_client.clone(),
                kind: ProcKind::external(),
            });

            spawn_healthcheck_process(id, idx, &local_handle, mailbox, proc_ref.clone());
        });

        node
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
                    if managed.child.kill().await.is_err() {
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
        let seed_args = self
            .seeds
            .iter()
            .copied()
            .flat_map(|seed_port| vec!["--seed".to_string(), seed_port.to_string()])
            .collect::<Vec<_>>();

        let handle = self.handle.clone();
        self.handle.spawn(async move {
            let mut cmd = Command::new("cargo");
            cmd.arg("run")
                .arg("-p")
                .arg("raft-server")
                .arg("--")
                .arg("--port")
                .arg(port.to_string())
                .args(seed_args)
                .stdout(Stdio::null())
                .stderr(Stdio::null());

            match cmd.spawn() {
                Err(_) => {
                    let _ = mailbox.send(ReplEvent::error(format!(
                        "Error when starting node {}",
                        idx
                    )));
                }

                Ok(child) => {
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
                        api_client: api_client.clone(),
                        kind: ProcKind::managed(child),
                    });

                    let _ = mailbox.send(ReplEvent::msg(format!("Node {} is starting", idx)));
                    spawn_healthcheck_process(id, idx, &handle, mailbox, proc_ref.clone());
                }
            }
        });
    }

    pub fn ping(&self) {
        let node_id = self.idx;
        let mailbox = self.mailbox.clone();
        let proc_ref = self.proc.clone();
        self.handle.spawn(async move {
            let mut proc = proc_ref.lock().await;

            if let Some(proc) = proc.as_mut() {
                if proc.api_client.ping(Request::new(())).await.is_err() {
                    let _ = mailbox.send(ReplEvent::error(format!("Ping node {} FAILED", node_id)));
                } else {
                    let _ = mailbox.send(ReplEvent::msg(format!("Ping node {} ok", node_id)));
                }
            } else {
                let _ = mailbox.send(ReplEvent::warn(format!(
                    "Pinging node {} is not possible",
                    node_id
                )));
            }
        });
    }

    pub fn append_to_stream(&mut self, args: AppendToStream) {
        let node_id = self.idx;
        let stream_id = if let Some(name) = args.stream {
            name
        } else {
            self.name_gen.next().unwrap()
        };
        let prop_name = self.name_gen.next().unwrap();
        let value_name = self.name_gen.next().unwrap();
        let payload = serde_json::json!({
            prop_name: value_name,
        });
        let mailbox = self.mailbox.clone();
        let proc_ref = self.proc.clone();
        self.handle.spawn(async move {
            let mut proc = proc_ref.lock().await;

            if let Some(proc) = proc.as_mut() {
                if let Err(e) = proc
                    .api_client
                    .append(Request::new(AppendReq {
                        stream_id,
                        events: vec![Bytes::from(serde_json::to_vec(&payload).unwrap())],
                    }))
                    .await
                {
                    let _ = mailbox.send(ReplEvent::error(format!(
                        "node {}: Error when appending: {} ",
                        node_id,
                        e.message()
                    )));
                } else {
                    let _ = mailbox.send(ReplEvent::msg(format!(
                        "node {}: append successful",
                        node_id
                    )));
                }
            } else {
                let _ = mailbox.send(ReplEvent::warn(format!(
                    "Appending node {} is not possible",
                    node_id
                )));
            }
        });
    }

    pub fn read_stream(&mut self, args: ReadStream) {
        let node_id = self.idx;
        let mailbox = self.mailbox.clone();
        let proc_ref = self.proc.clone();

        self.handle.spawn(async move {
            let mut proc = proc_ref.lock().await;

            if let Some(proc) = proc.as_mut() {
                match proc
                    .api_client
                    .read(Request::new(ReadReq {
                        stream_id: args.stream.clone(),
                    }))
                    .await
                {
                    Err(e) => {
                        let _ = mailbox.send(ReplEvent::error(format!(
                            "Reading stream '{}' from node {} caused an error: {}",
                            args.stream,
                            args.node,
                            e.message()
                        )));
                    }
                    Ok(stream) => {
                        let mut events = Vec::new();
                        let mut stream = stream.into_inner();

                        loop {
                            match stream.try_next().await {
                                Err(e) => {
                                    let _ = mailbox.send(ReplEvent::error(format!(
                                        "Reading stream '{}' from node {} caused an error: {}",
                                        args.stream,
                                        args.node,
                                        e.message()
                                    )));
                                }

                                Ok(resp) => {
                                    if let Some(resp) = resp {
                                        events.push(RecordedEvent {
                                            stream_id: resp.stream_id,
                                            global: resp.global,
                                            revision: resp.revision,
                                            payload: resp.payload,
                                        });
                                        continue;
                                    }

                                    break;
                                }
                            }
                        }

                        let _ = mailbox.send(ReplEvent::msg(format!(
                            "Reading stream '{}' from node {} was successful",
                            args.stream, node_id
                        )));
                        let _ = mailbox.send(ReplEvent::stream_read(node_id, args.stream, events));
                    }
                }
            }
        });
    }

    pub fn cleanup(self) {
        self.handle.block_on(async move {
            let mut proc = self.proc.lock().await;

            if let Some(proc) = proc.take() {
                if let ProcKind::Managed(mut managed) = proc.kind {
                    let _ = managed.child.kill().await;
                }
            }
        });
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
                    let _ = mailbox.send(ReplEvent::warn(format!(
                        "Node {} connection error: {}",
                        node,
                        e.message(),
                    )));

                    let _ = mailbox.send(ReplEvent::node_connectivity(node, Connectivity::Offline));
                }

                Ok(resp) => {
                    let resp = resp.into_inner();
                    let _ = mailbox.send(ReplEvent::node_connectivity(
                        node,
                        Connectivity::Online(resp),
                    ));
                }
            }
        }
    });
}
