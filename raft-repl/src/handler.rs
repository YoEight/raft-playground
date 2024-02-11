use crate::data::RecordedEvent;
use crate::events::ReplEvent;
use crate::node::{Connectivity, ProcKind, ProcType};
use bytes::Bytes;
use hyper::client::HttpConnector;
use hyper::Client;
use raft_common::client::ApiClient;
use raft_common::{AppendReq, ReadReq};
use std::sync::mpsc::Sender;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::sleep;
use tonic::body::BoxBody;
use tonic::codegen::tokio_stream::StreamExt;
use tonic::{Code, Request};
use tracing::{error, info};

pub struct CommandHandler {
    index: usize,
    port: u16,
    last_term: u64,
    client: ApiClient<Client<HttpConnector, BoxBody>>,
    state: Connectivity,
    mailbox: Sender<ReplEvent>,
    proc: Option<ProcKind>,
}

impl CommandHandler {
    pub fn new(index: usize, port: u16, mailbox: Sender<ReplEvent>) -> Self {
        let mut connector = HttpConnector::new();
        connector.enforce_http(false);
        let client = hyper::Client::builder().http2_only(true).build(connector);
        let uri = hyper::Uri::from_maybe_shared(format!("http://localhost:{}", port)).unwrap();
        let client = ApiClient::with_origin(client, uri);

        Self {
            index,
            port,
            last_term: 0,
            client,
            state: Connectivity::Offline,
            mailbox,
            proc: None,
        }
    }

    pub async fn read_stream(&mut self, stream_name: String) {
        match self
            .client
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
                let _ = self
                    .mailbox
                    .send(ReplEvent::stream_read(self.index, stream_name, events));
            }
        }
    }

    pub async fn append_stream(&mut self, stream_name: String, events: Vec<Bytes>) {
        if let Err(e) = self
            .client
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
    }

    pub async fn status(&mut self) {
        match self.client.status(Request::new(())).await {
            Err(e) => {
                error!(
                    "node_{}:{} error when requesting status: {}",
                    "localhost", self.port, e
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
                    ));
                    self.state = Connectivity::Offline;
                }
            }

            Ok(resp) => {
                let resp = resp.into_inner();
                info!("node_{}:{} status = {:?}", "localhost", self.port, resp);

                if self.state.is_offline()
                    || self.last_term != resp.term
                    || self.state.status() != Some(resp.status.clone())
                {
                    self.last_term = resp.term;
                    self.state = Connectivity::Online(resp);
                    let _ = self
                        .mailbox
                        .send(ReplEvent::node_connectivity(self.index, self.state.clone()));
                }
            }
        }
    }

    pub async fn ping(&mut self) {
        if self.client.ping(Request::new(())).await.is_err() {
            let _ = self
                .mailbox
                .send(ReplEvent::error(format!("Ping node {} FAILED", self.index)));
        } else {
            let _ = self
                .mailbox
                .send(ReplEvent::msg(format!("Ping node {} ok", self.index)));
        }
    }

    pub async fn start(&mut self, port: u16, seeds: Vec<u16>, r#type: ProcType) {
        self.stop().await;
    }

    pub async fn stop(&mut self) {
        if let Some(current) = self.proc.take() {
            let _ = self.mailbox.send(ReplEvent::msg(format!(
                "Node {} is stopping...",
                self.index
            )));

            match current {
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
                if let Err(status) = self.client.status(Request::new(())).await {
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
