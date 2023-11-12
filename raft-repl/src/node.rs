use crate::events::ReplEvent;
use hyper::client::HttpConnector;
use hyper::Client;
use raft_common::client::{ApiClient, RaftClient};
use std::fmt::{Display, Formatter};
use std::sync::mpsc;
use tokio::runtime::Handle;
use tonic::body::BoxBody;

#[derive(Copy, Clone)]
pub enum Connectivity {
    Online,
    Offline,
}

pub struct Node {
    idx: usize,
    port: usize,
    connectivity: Connectivity,
    handle: Handle,
    mailbox: mpsc::Sender<ReplEvent>,
    raft_client: RaftClient<Client<HttpConnector, BoxBody>>,
    api_client: ApiClient<Client<HttpConnector, BoxBody>>,
}

impl Node {
    pub fn new(
        idx: usize,
        handle: Handle,
        mailbox: mpsc::Sender<ReplEvent>,
        port: usize,
    ) -> eyre::Result<Self> {
        let mut connector = HttpConnector::new();

        connector.enforce_http(false);
        let client = hyper::Client::builder().http2_only(true).build(connector);
        let uri = hyper::Uri::from_maybe_shared(format!("http://localhost:{}", port))?;
        let raft_client = RaftClient::with_origin(client.clone(), uri.clone());
        let api_client = ApiClient::with_origin(client, uri);
        let local_mailbox = mailbox.clone();

        handle.spawn(async move {
            let _ = local_mailbox.send(ReplEvent::node_connectivity(idx, Connectivity::Online));
        });

        Ok(Self {
            idx,
            connectivity: Connectivity::Offline,
            handle,
            mailbox,
            port,
            raft_client,
            api_client,
        })
    }

    pub fn port(&self) -> usize {
        self.port
    }

    pub fn connectivity(&self) -> Connectivity {
        self.connectivity
    }

    pub fn set_connectivity(&mut self, connectivity: Connectivity) {
        self.connectivity = connectivity;
    }

    pub fn send_event(&self) {
        let api_client = self.api_client.clone();
    }
}
