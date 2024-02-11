use crate::data::RecordedEvent;
use crate::events::ReplEvent;
use crate::node::{Connectivity, ProcKind, ProcType};
use bytes::Bytes;
use hyper::client::HttpConnector;
use hyper::Client;
use raft_common::client::ApiClient;
use raft_common::{AppendReq, ReadReq};
use raft_server::{options, Node};
use std::process::Stdio;
use std::sync::mpsc::Sender;
use std::time::Duration;
use tokio::process::Command;
use tokio::runtime::Handle;
use tokio::time::sleep;
use tonic::body::BoxBody;
use tonic::codegen::tokio_stream::StreamExt;
use tonic::{Code, Request};
use tracing::{error, info};

type GrpcClient = ApiClient<Client<HttpConnector, BoxBody>>;

enum ProcState {
    Disconnected,
    Connected {
        kind: ProcKind,
        port: u16,
        client: GrpcClient,
    },
}

impl ProcState {
    fn take(&mut self) -> Option<(ProcKind, GrpcClient)> {
        if let ProcState::Connected { kind, client, .. } =
            std::mem::replace(self, ProcState::Disconnected)
        {
            return Some((kind, client));
        }

        None
    }
}

pub struct CommandHandler {
    index: usize,
    last_term: u64,
    state: Connectivity,
    mailbox: Sender<ReplEvent>,
    proc: ProcState,
}

impl CommandHandler {
    pub fn new(index: usize, mailbox: Sender<ReplEvent>) -> Self {
        Self {
            index,
            last_term: 0,
            state: Connectivity::Offline,
            mailbox,
            proc: ProcState::Disconnected,
        }
    }

    pub fn is_external(&self) -> bool {
        if let ProcState::Connected {
            kind: ProcKind::External(_),
            ..
        } = &self.proc
        {
            return true;
        }

        false
    }

    pub async fn read_stream(&mut self, stream_name: String) {
        if let ProcState::Connected { client, .. } = &mut self.proc {
            match client
                .read(Request::new(ReadReq {
                    stream_id: stream_name.clone(),
                }))
                .await
            {
                Err(e) => {
                    let _ = self.mailbox.send(ReplEvent::error(format!(
                        "Reading stream '{}' from node {} caused an error: {}",
                        stream_name,
                        self.index,
                        e.message()
                    )));
                }

                Ok(stream) => {
                    let mut events = Vec::new();
                    let mut stream = stream.into_inner();

                    loop {
                        match stream.try_next().await {
                            Err(e) => {
                                let _ = self.mailbox.send(ReplEvent::error(format!(
                                    "Reading stream '{}' from node {} caused an error: {}",
                                    stream_name,
                                    self.index,
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

                    let _ = self.mailbox.send(ReplEvent::msg(format!(
                        "Reading stream '{}' from node {} was successful",
                        stream_name, self.index
                    )));
                    let _ =
                        self.mailbox
                            .send(ReplEvent::stream_read(self.index, stream_name, events));
                }
            }

            return;
        }

        let _ = self.mailbox.send(ReplEvent::error(format!(
            "Can't read stream on node {} because it isn't connected",
            self.index
        )));
    }

    pub async fn append_stream(&mut self, stream_name: String, events: Vec<Bytes>) {
        if let ProcState::Connected { client, .. } = &mut self.proc {
            if let Err(e) = client
                .append(Request::new(AppendReq {
                    stream_id: stream_name.clone(),
                    events,
                }))
                .await
            {
                let _ = self.mailbox.send(ReplEvent::error(format!(
                    "node {}: Error when appending: {} ",
                    self.index,
                    e.message()
                )));
            } else {
                let _ = self.mailbox.send(ReplEvent::msg(format!(
                    "node {}: append successful",
                    self.index
                )));
            }

            return;
        }

        let _ = self.mailbox.send(ReplEvent::error(format!(
            "Can't append stream on node {} because it isn't connected",
            self.index
        )));
    }

    pub async fn status(&mut self) {
        if let ProcState::Connected { client, port, .. } = &mut self.proc {
            match client.status(Request::new(())).await {
                Err(e) => {
                    error!(
                        "node_{}:{} error when requesting status: {}",
                        "localhost", port, e
                    );

                    if self.state.is_online() {
                        let _ = self.mailbox.send(ReplEvent::warn(format!(
                            "Node {} connection error: {}",
                            self.index,
                            e.message(),
                        )));

                        let _ = self.mailbox.send(ReplEvent::node_connectivity(
                            self.index,
                            Connectivity::Offline,
                            self.is_external(),
                        ));
                        self.state = Connectivity::Offline;
                    }
                }

                Ok(resp) => {
                    let resp = resp.into_inner();
                    info!("node_{}:{} status = {:?}", "localhost", port, resp);

                    if self.state.is_offline()
                        || self.last_term != resp.term
                        || self.state.status() != Some(resp.status.clone())
                    {
                        self.last_term = resp.term;
                        self.state = Connectivity::Online(resp);
                        let _ = self.mailbox.send(ReplEvent::node_connectivity(
                            self.index,
                            self.state.clone(),
                            self.is_external(),
                        ));
                    }
                }
            }

            return;
        }

        let _ = self.mailbox.send(ReplEvent::error(format!(
            "Can't read status on node {} because it isn't connected",
            self.index
        )));
    }

    pub async fn cleanup(mut self) {
        self.stop().await;
    }

    pub async fn ping(&mut self) {
        if let ProcState::Connected { client, .. } = &mut self.proc {
            if client.ping(Request::new(())).await.is_err() {
                let _ = self
                    .mailbox
                    .send(ReplEvent::error(format!("Ping node {} FAILED", self.index)));
            } else {
                let _ = self
                    .mailbox
                    .send(ReplEvent::msg(format!("Ping node {} ok", self.index)));
            }

            return;
        }

        let _ = self.mailbox.send(ReplEvent::error(format!(
            "Can't read status on node {} because it isn't connected",
            self.index
        )));
    }

    pub async fn start(&mut self, handle: &Handle, port: u16, seeds: Vec<u16>, r#type: ProcType) {
        self.stop().await;
        let client = create_client(port);

        match r#type {
            ProcType::Managed => {
                match Node::new(handle.clone(), options::Options { port, seeds }) {
                    Err(e) => {
                        let _ = self.mailbox.send(ReplEvent::error(format!(
                            "Node {} errored upon starting: {}",
                            self.index, e
                        )));
                    }

                    Ok(mut node) => {
                        node.start();
                        self.proc = ProcState::Connected {
                            kind: ProcKind::managed(node),
                            client,
                            port,
                        };
                    }
                }
            }

            ProcType::Binary => {
                let seeds = seeds
                    .iter()
                    .copied()
                    .flat_map(|p| vec!["--seed".to_string(), p.to_string()])
                    .collect::<Vec<_>>();

                match Command::new("cargo")
                    .arg("run")
                    .arg("-p")
                    .arg("raft-server")
                    .arg("--")
                    .arg("--port")
                    .arg(port.to_string())
                    .args(seeds)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    Err(e) => {
                        let _ = self.mailbox.send(ReplEvent::error(format!(
                            "Node {} errored upon spawning: {}",
                            self.index, e
                        )));
                    }

                    Ok(child) => {
                        self.proc = ProcState::Connected {
                            kind: ProcKind::spawn(child),
                            client,
                            port,
                        };
                    }
                }
            }

            ProcType::External => {
                self.proc = ProcState::Connected {
                    kind: ProcKind::external(),
                    client,
                    port,
                };
            }
        }

        self.status().await;
    }

    pub async fn stop(&mut self) {
        if let Some((kind, mut client)) = self.proc.take() {
            let _ = self.mailbox.send(ReplEvent::msg(format!(
                "Node {} is stopping...",
                self.index
            )));

            match kind {
                ProcKind::Managed(args) => {
                    if let Some(handle) = args.join {
                        handle.abort();
                    }
                }

                ProcKind::External(_) => {
                    let _ = self.mailbox.send(ReplEvent::warn(format!(
                        "Node {} is an external process, we can't stop it",
                        self.index
                    )));

                    return;
                }

                ProcKind::Spawn(mut args) => {
                    let _ = args.kill();
                }
            }

            loop {
                if let Err(status) = client.status(Request::new(())).await {
                    if status.code() == Code::Unavailable {
                        let _ = self
                            .mailbox
                            .send(ReplEvent::msg(format!("Node {} is stopped", self.index)));

                        return;
                    }
                }

                info!("Node {} is still up. keep waiting...", self.index);
                sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

fn create_client(port: u16) -> ApiClient<Client<HttpConnector, BoxBody>> {
    let mut connector = HttpConnector::new();

    connector.enforce_http(false);
    let client = hyper::Client::builder().http2_only(true).build(connector);
    let uri = hyper::Uri::from_maybe_shared(format!("http://localhost:{}", port)).unwrap();

    ApiClient::with_origin(client, uri)
}
