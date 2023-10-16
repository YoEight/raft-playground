use hyper::client::HttpConnector;
use raft_common::client::RaftClient;
use tonic::transport::Channel;
use uuid::Uuid;

pub type HyperClient = hyper::Client<HttpConnector, tonic::body::BoxBody>;

#[derive(Debug)]
pub struct Seed {
    pub host: String,
    pub port: u16,
    pub client: Option<RaftClient<HyperClient>>,
}

impl Seed {
    pub fn client(&mut self) -> RaftClient<HyperClient> {
        if let Some(client) = self.client.clone() {
            client
        } else {
            let uri =
                hyper::Uri::from_maybe_shared(format!("http://{}:{}", "127.0.0.1", self.port))
                    .unwrap();

            let hyper_client = hyper::Client::builder().http2_only(true).build_http();
            let raft_client = RaftClient::with_origin(hyper_client, uri);
            self.client = Some(raft_client.clone());

            raft_client
        }
    }

    pub fn send_heartbeat(&mut self) {
        let client = self.client();
        tokio::spawn(async move {});
    }
}
