use crate::machine::{AppendEntriesResp, NodeClient, Persistent};
use hyper::client::HttpConnector;
use raft_common::client::RaftClient;
use raft_common::{EntriesReq, Entry, NodeId, VoteReq};
use std::cmp::min;
use std::collections::HashMap;
use std::time::Instant;
use tokio::runtime::Handle;
use tonic::Request;
use tracing::debug;

pub struct Seeds {
    node_id: NodeId,
    inner: HashMap<NodeId, Seed>,
}

impl Seeds {
    pub fn new(node_id: NodeId, seeds: Vec<Seed>) -> Self {
        let mut inner = HashMap::new();

        for seed in seeds {
            inner.insert(seed.id.clone(), seed);
        }

        Self { node_id, inner }
    }

    pub fn node_mut(&mut self, id: &NodeId) -> &mut Seed {
        self.inner.get_mut(id).unwrap()
    }

    pub fn reset(&mut self, last_index: u64) {
        for seed in self.inner.values_mut() {
            seed.next_index = last_index + 1;
            seed.match_index = 0;
        }
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn send_append_entries(&self, persistent: &Persistent, commit_index: u64) {
        for seed in self.inner.values() {
            let (prev_log_index, prev_log_term) =
                persistent.entries.get_previous_entry(seed.next_index);

            let entries = persistent.entries.read_entries_from(prev_log_index, 500);
            seed.send_append_entries(
                persistent.term,
                self.node_id.clone(),
                prev_log_index,
                prev_log_term,
                commit_index,
                entries,
            )
        }
    }

    pub fn request_vote(&self, persistent: &Persistent) {
        let last_log_index = persistent.entries.last_index();
        let last_log_term = persistent.entries.last_term();

        for seed in self.inner.values() {
            seed.request_vote(
                persistent.term,
                self.node_id.clone(),
                last_log_index,
                last_log_term,
            );
        }
    }

    pub fn compute_lowest_replicated_index(&self) -> u64 {
        let mut lowest_replicated_index = u64::MAX;
        for seed in self.inner.values() {
            lowest_replicated_index = min(lowest_replicated_index, seed.match_index);
        }

        lowest_replicated_index
    }
}

pub type HyperClient = hyper::Client<HttpConnector, tonic::body::BoxBody>;

#[derive(Debug, Clone)]
pub struct Seed {
    pub id: NodeId,
    pub mailbox: NodeClient,
    pub client: RaftClient<HyperClient>,
    pub next_index: u64,
    pub match_index: u64,
    runtime: Handle,
}

impl Seed {
    pub fn new(id: NodeId, mailbox: NodeClient, runtime: Handle) -> Self {
        let uri = hyper::Uri::from_maybe_shared(format!("http://{}:{}", id.host.as_str(), id.port))
            .unwrap();

        let hyper_client = hyper::Client::builder().http2_only(true).build_http();
        let client = RaftClient::with_origin(hyper_client, uri);

        Self {
            id,
            mailbox,
            client,
            runtime,
            next_index: 0,
            match_index: 0,
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
        let node_client = self.mailbox.clone();
        let node_id = self.id.clone();

        self.runtime.spawn(async move {
            let resp = client
                .request_vote(Request::new(VoteReq {
                    term,
                    candidate_id: Some(candidate_id),
                    last_log_index,
                    last_log_term,
                }))
                .await?
                .into_inner();

            node_client.vote_received(node_id, resp.term, resp.vote_granted);

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
        let node_client = self.mailbox.clone();
        self.runtime.spawn(async move {
            let stopwatch = Instant::now();
            let batch_end_log_index = if let Some(last) = entries.last() {
                Some(last.index)
            } else {
                None
            };

            let resp = client
                .append_entries(Request::new(EntriesReq {
                    term,
                    leader_id: Some(leader_id.clone()),
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

            debug!(
                "node_{}:{} [term={}] Sending append_entries rpc to {}:{} took {:?}",
                leader_id.host,
                leader_id.port,
                term,
                node_id.host,
                node_id.port,
                stopwatch.elapsed()
            );

            node_client.append_entries_response_received(
                node_id.clone(),
                batch_end_log_index,
                resp,
            );

            debug!(
                "node_{}:{} [term={}] append_entries response sent {}:{}",
                leader_id.host, leader_id.port, term, node_id.host, node_id.port,
            );
        });
    }
}
