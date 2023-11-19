use crate::events::ReplEvent;
use bytes::Bytes;
use hyper::client::HttpConnector;
use hyper::Client;
use names::Generator;
use raft_common::client::{ApiClient, RaftClient};
use raft_common::AppendReq;
use std::process::Stdio;
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::runtime::Handle;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tonic::body::BoxBody;
use tonic::Request;

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum Connectivity {
    Online,
    Offline,
}

struct Proc {
    raft_client: RaftClient<Client<HttpConnector, BoxBody>>,
    api_client: ApiClient<Client<HttpConnector, BoxBody>>,
    process: Child,
}

const MAX_CONNECTION_ATTEMPTS: usize = 30;

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
        let client = hyper::Client::builder().http2_only(true).build(connector);
        let uri = hyper::Uri::from_maybe_shared(format!("http://localhost:{}", port))?;
        let raft_client = RaftClient::with_origin(client.clone(), uri.clone());
        let api_client = ApiClient::with_origin(client, uri);
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

    pub fn port(&self) -> usize {
        self.port
    }

    pub fn connectivity(&self) -> Connectivity {
        self.connectivity
    }

    pub fn set_connectivity(&mut self, connectivity: Connectivity) {
        self.connectivity = connectivity;
        if connectivity == Connectivity::Offline {
            let proc_ref = self.proc.clone();
            self.handle.block_on(async move {
                let mut proc = proc_ref.lock().await;
                *proc = None;
            });
        }
    }

    pub fn send_event(&mut self) -> eyre::Result<()> {
        let prop = self.name_gen.next().unwrap();
        let value = self.name_gen.next().unwrap();
        let payload = serde_json::to_vec(&serde_json::json!({
            prop: value,
        }))?;

        let idx = self.idx;
        let proc_ref = self.proc.clone();
        let mailbox = self.mailbox.clone();
        self.handle.spawn(async move {
            let mut proc = proc_ref.lock().await;

            if let Some(proc) = proc.as_mut() {
                let result = proc
                    .api_client
                    .append(Request::new(AppendReq {
                        stream_id: "foobar".to_string(),
                        events: vec![Bytes::from(payload)],
                    }))
                    .await;

                match result {
                    Err(status) => {
                        let _ = mailbox.send(ReplEvent::error(format!(
                            "Error when sending event to node {}: {}",
                            idx,
                            status.message()
                        )));
                    }
                    Ok(resp) => {
                        let position = resp.into_inner().position;
                        let _ = mailbox.send(ReplEvent::msg(format!(
                            "Node {} appended to position {}",
                            idx, position
                        )));
                    }
                }
            } else {
            }
        });

        Ok(())
    }

    pub fn stop(&mut self) {
        let mailbox = self.mailbox.clone();
        let proc = self.proc.clone();
        let idx = self.idx;
        self.handle.spawn(async move {
            let mut proc = proc.lock().await;
            if let Some(mut proc_handle) = proc.take() {
                if proc_handle.process.kill().await.is_ok() {
                    let _ = mailbox.send(ReplEvent::node_connectivity(idx, Connectivity::Offline));
                } else {
                    *proc = Some(proc_handle);
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

                Ok(mut child) => {
                    let mut proc = proc_ref.lock().await;
                    let mut connector = HttpConnector::new();

                    connector.enforce_http(false);
                    let client = hyper::Client::builder().http2_only(true).build(connector);
                    let uri = hyper::Uri::from_maybe_shared(format!("http://localhost:{}", port))
                        .unwrap();
                    let raft_client = RaftClient::with_origin(client.clone(), uri.clone());
                    let mut api_client = ApiClient::with_origin(client, uri);

                    // We make sure that the node is able to receive requests.
                    let mut attempts = 0;
                    let mut error = None;
                    while attempts < MAX_CONNECTION_ATTEMPTS {
                        match api_client.ping(tonic::Request::new(())).await {
                            Err(s) => {
                                error = Some(s);
                                let _ = mailbox.send(ReplEvent::warn(format!(
                                    "Node {} attempt {}/{} failed",
                                    idx,
                                    attempts + 1,
                                    MAX_CONNECTION_ATTEMPTS,
                                )));
                            }
                            Ok(_) => {
                                error = None;
                                break;
                            }
                        }

                        attempts += 1;
                        sleep(Duration::from_secs(1)).await;
                    }

                    if error.is_none() {
                        *proc = Some(Proc {
                            raft_client,
                            api_client: api_client.clone(),
                            process: child,
                        });

                        let _ =
                            mailbox.send(ReplEvent::node_connectivity(idx, Connectivity::Online));
                        spawn_healthcheck_process(idx, &handle, mailbox, api_client);
                        return;
                    }

                    let _ = child.kill().await;
                    let _ = mailbox.send(ReplEvent::error(format!(
                        "Error when starting node {}: {}",
                        idx,
                        error.unwrap().message()
                    )));
                }
            }
        });
    }

    pub fn cleanup(self) {
        self.handle.block_on(async move {
            let mut proc = self.proc.lock().await;

            if let Some(mut proc) = proc.take() {
                let _ = proc.process.kill().await;
            }
        });
    }
}

fn spawn_healthcheck_process(
    node: usize,
    handle: &Handle,
    mailbox: mpsc::Sender<ReplEvent>,
    mut api_client: ApiClient<Client<HttpConnector, BoxBody>>,
) {
    handle.spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));

        loop {
            ticker.tick().await;
            if api_client.ping(Request::new(())).await.is_err() {
                let _ = mailbox.send(ReplEvent::node_connectivity(node, Connectivity::Offline));
                break;
            }
        }
    });
}
