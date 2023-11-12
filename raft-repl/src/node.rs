use crate::events::ReplEvent;
use hyper::client::HttpConnector;
use hyper::Client;
use raft_common::client::{ApiClient, RaftClient};
use std::sync::mpsc;
use tonic::body::BoxBody;

pub struct Node {
    port: usize,
    mailbox: mpsc::Sender<ReplEvent>,
    raft_client: RaftClient<Client<HttpConnector, BoxBody>>,
    api_client: ApiClient<Client<HttpConnector, BoxBody>>,
}

impl Node {
    pub fn new(mailbox: mpsc::Sender<ReplEvent>, port: usize) -> eyre::Result<Self> {
        let mut connector = HttpConnector::new();

        connector.enforce_http(false);
        let client = hyper::Client::builder().http2_only(true).build(connector);
        let uri = hyper::Uri::from_maybe_shared(format!("http://localhost:{}", port))?;
        let raft_client = RaftClient::with_origin(client.clone(), uri.clone());
        let api_client = ApiClient::with_origin(client, uri);

        Ok(Self {
            mailbox,
            port,
            raft_client,
            api_client,
        })
    }

    pub fn port(&self) -> usize {
        self.port
    }

    pub fn send_event(&self) {
        let api_client = self.api_client.clone();
    }
}
