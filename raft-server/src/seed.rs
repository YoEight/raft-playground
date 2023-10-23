use crate::machine::{AppendEntriesResp, Msg};
use hyper::client::HttpConnector;
use raft_common::client::RaftClient;
use raft_common::{EntriesReq, Entry, NodeId, VoteReq};
use std::sync::mpsc::Sender;
use tonic::Request;

pub type HyperClient = hyper::Client<HttpConnector, tonic::body::BoxBody>;

#[derive(Debug, Clone)]
pub struct Seed {
    pub id: NodeId,
    pub mailbox: Sender<Msg>,
    pub client: RaftClient<HyperClient>,
}

impl Seed {
    pub fn new(id: NodeId, mailbox: Sender<Msg>) -> Self {
        let uri = hyper::Uri::from_maybe_shared(format!("http://{}:{}", id.host.as_str(), id.port))
            .unwrap();

        let hyper_client = hyper::Client::builder().http2_only(true).build_http();
        let client = RaftClient::with_origin(hyper_client, uri);

        Self {
            id,
            mailbox,
            client,
        }
    }

    pub fn request_vote(
        &self,
        term: u64,
        candidate_id: NodeId,
        last_log_index: u64,
        last_log_term: u64,
    ) {
        let mut client = self.client.clone();
        let sender = self.mailbox.clone();
        let node_id = self.id.clone();

        tokio::spawn(async move {
            let resp = client
                .request_vote(Request::new(VoteReq {
                    term,
                    candidate_id: Some(candidate_id),
                    last_log_index,
                    last_log_term,
                }))
                .await?
                .into_inner();

            let _ = sender.send(Msg::VoteReceived {
                node_id,
                term: resp.term,
                granted: resp.vote_granted,
            });

            Ok::<_, tonic::Status>(())
        });
    }

    pub fn send_append_entries(
        &self,
        term: u64,
        leader_id: NodeId,
        prev_log_index: u64,
        prev_log_term: u64,
        leader_commit: u64,
        entries: Vec<Entry>,
    ) {
        let mut client = self.client.clone();
        let node_id = self.id.clone();
        let sender = self.mailbox.clone();
        tokio::spawn(async move {
            let resp = client
                .append_entries(Request::new(EntriesReq {
                    term,
                    leader_id: Some(leader_id),
                    prev_log_index,
                    prev_log_term,
                    leader_commit,
                    entries,
                }))
                .await
                .map(|resp| {
                    let resp = resp.into_inner();
                    AppendEntriesResp {
                        term: resp.term,
                        success: resp.success,
                    }
                });

            let _ = sender.send(Msg::AppendEntriesResp {
                node_id,
                prev_log_index,
                prev_log_term,
                resp,
            });
        });
    }
}
