use hyper::client::HttpConnector;
use raft_common::client::RaftClient;
use raft_common::{EntriesReq, Entry};
use tonic::Request;
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

    pub fn send_heartbeat(
        &mut self,
        term: u64,
        leader_id: Uuid,
        prev_log_index: u64,
        prev_log_term: u64,
        leader_commit: u64,
    ) {
        self.send_append_entries(
            term,
            leader_id,
            prev_log_index,
            prev_log_term,
            leader_commit,
            vec![],
        );
    }

    pub fn send_append_entries(
        &mut self,
        term: u64,
        leader_id: Uuid,
        prev_log_index: u64,
        prev_log_term: u64,
        leader_commit: u64,
        entries: Vec<Entry>,
    ) {
        let mut client = self.client();
        tokio::spawn(async move {
            let resp = client
                .append_entries(Request::new(EntriesReq {
                    term,
                    leader_id: leader_id.to_string(),
                    prev_log_index,
                    prev_log_term,
                    leader_commit,
                    entries,
                }))
                .await;
        });
    }
}
