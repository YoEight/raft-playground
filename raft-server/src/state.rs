use crate::entry::{Entries, Entry};
use crate::env::Seed;
use crate::vote_listener::VoteMsg;
use crate::{ticking, vote_listener};
use bytes::Bytes;
use raft_common::client::RaftClient;
use raft_common::VoteReq;
use rand::{thread_rng, Rng};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex;
use tonic::Request;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Status {
    Follower,
    Candidate,
    Leader,
}

impl Default for Status {
    fn default() -> Self {
        Status::Follower
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ElectionTimeoutRange {
    pub low: u64,
    pub high: u64,
    pub duration: Duration,
}

impl Default for ElectionTimeoutRange {
    fn default() -> Self {
        Self {
            low: 150,
            high: 300,
            duration: Duration::default(),
        }
    }
}

impl ElectionTimeoutRange {
    pub fn pick_timeout_value(&mut self) {
        let mut rng = thread_rng();
        let timeout = rng.gen_range(self.low..self.high);
        self.duration = Duration::from_millis(timeout);
    }
}

pub struct State {
    pub id: Uuid,
    pub seeds: HashMap<u16, Seed>,
    pub entries: Entries,
    pub term: u64,
    pub writer: u64,
    pub status: Status,
    pub voted_for: Option<Uuid>,
    pub commit_index: u64,
    pub last_applied: u64,
    pub next_index: HashMap<Uuid, u64>,
    pub match_index: HashMap<Uuid, u64>,
    pub election_timeout: Instant,
    pub election_timeout_range: ElectionTimeoutRange,
}

impl Default for State {
    fn default() -> Self {
        Self {
            id: Default::default(),
            seeds: Default::default(),
            entries: Entries::default(),
            term: 0,
            writer: 0,
            status: Default::default(),
            voted_for: None,
            commit_index: 0,
            last_applied: 0,
            next_index: Default::default(),
            match_index: Default::default(),
            election_timeout: Instant::now(),
            election_timeout_range: Default::default(),
        }
    }
}

impl State {
    pub fn default_with_seeds(seeds: Vec<u16>) -> Self {
        let mut this = State::default();

        for seed in seeds {
            this.seeds.insert(
                seed,
                Seed {
                    host: "127.0.0.1".to_string(),
                    port: seed,
                    client: None,
                },
            );
        }

        this
    }

    pub fn start(mut self) -> NodeState {
        self.id = Uuid::new_v4();
        self.election_timeout_range.pick_timeout_value();

        let (sender, recv) = tokio::sync::mpsc::unbounded_channel();
        let node_state = NodeState::new(sender, self);

        ticking::spawn_ticking_process(node_state.clone());
        vote_listener::spawn_vote_listener(node_state.clone(), recv);

        node_state
    }

    pub fn election_timeout(&self) -> bool {
        self.election_timeout.elapsed() >= self.election_timeout_range.duration
    }

    pub fn switch_to_leader(&mut self) {
        self.status = Status::Leader;
        self.term += 1;
    }

    pub fn switch_to_candidate(&mut self, vote_sender: UnboundedSender<VoteMsg>) {
        self.status = Status::Candidate;
        self.term += 1;
        self.voted_for = Some(self.id);

        let term = self.term;
        let snapshot = self.entries.snapshot();
        let last_log_index = snapshot.index;
        let last_log_term = snapshot.term;

        for seed in self.seeds.values_mut() {
            let mut client = if let Some(client) = seed.client.clone() {
                client
            } else {
                let uri =
                    hyper::Uri::from_maybe_shared(format!("http://{}:{}", "127.0.0.1", seed.port))
                        .unwrap();

                let hyper_client = hyper::Client::builder().http2_only(true).build_http();
                let raft_client = RaftClient::with_origin(hyper_client, uri);
                seed.client = Some(raft_client.clone());

                raft_client
            };

            let candidate_id = self.id.to_string();
            let local_sender = vote_sender.clone();

            tokio::spawn(async move {
                let resp = client
                    .request_vote(Request::new(VoteReq {
                        term,
                        candidate_id,
                        last_log_index,
                        last_log_term,
                    }))
                    .await?
                    .into_inner();

                let _ = local_sender.send(VoteMsg::VoteReceived {
                    term: resp.term,
                    granted: resp.vote_granted,
                });

                Ok::<_, tonic::Status>(())
            });
        }
    }
}

#[derive(Clone)]
pub struct NodeState {
    vote_sender: UnboundedSender<VoteMsg>,
    inner: Arc<Mutex<State>>,
}

impl NodeState {
    pub fn new(vote_sender: UnboundedSender<VoteMsg>, inner: State) -> Self {
        Self {
            vote_sender,
            inner: Arc::new(Mutex::new(inner)),
        }
    }

    pub async fn request_vote(
        &self,
        term: u64,
        candidate_id: Uuid,
        last_log_index: u64,
        last_log_term: u64,
    ) -> (u64, bool) {
        let mut state = self.inner.lock().await;

        if state.term > term {
            return (0, false);
        }

        if let Some(id) = state.voted_for {
            return (
                state.term,
                id == candidate_id && state.entries.last_index() <= last_log_index,
            );
        } else {
            if state.entries.last_index() <= last_log_index
                && state.entries.last_term() <= last_log_term
            {
                state.voted_for = Some(candidate_id);
                state.status = Status::Follower;
                state.term = term;

                return (term, true);
            }

            (0, false)
        }
    }

    pub async fn append_entries(
        &self,
        term: u64,
        leader_id: Uuid,
        prev_log_index: u64,
        prev_log_term: u64,
        leader_commit: u64,
        entries: Vec<Bytes>,
    ) -> (u64, bool) {
        let mut state = self.inner.lock().await;

        if state.term > term || state.entries.contains_log(prev_log_index, prev_log_term) {
            return (state.term, false);
        }

        if state.term < term {
            state.voted_for = None;
            state.term = term;
        }

        state.status = Status::Follower;
        state.election_timeout = Instant::now();

        // Means it's a heartbeat message.
        if entries.is_empty() {
            return (state.term, true);
        }

        if state.entries.last_index() > prev_log_index && state.entries.last_term() != term {
            // TODO delete uncommitted entries.
        }

        (0, false)
    }

    pub async fn current_term(&self) -> u64 {
        let state = self.inner.lock().await;
        state.term
    }

    pub async fn on_ticking(&self) {
        let mut state = self.inner.lock().await;

        // Means we are running in single node.
        if state.seeds.is_empty() {
            return;
        }

        if state.election_timeout() {
            state.switch_to_candidate(self.vote_sender.clone());
            return;
        }
    }

    pub async fn on_vote_received(&self, term: u64, granted: bool) {}
}
