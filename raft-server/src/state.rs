use crate::entry::Entries;
use crate::env::Seed;
use crate::options::Options;
use crate::vote_listener::{AppendEntriesResp, IncomingMsg};
use crate::{ticking, vote_listener};
use raft_common::client::RaftClient;
use raft_common::{EntriesReq, Entry, VoteReq};
use rand::{thread_rng, Rng};
use std::cmp::min;
use std::collections::{HashMap, HashSet};
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
    pub next_index: HashMap<u16, u64>,
    pub match_index: HashMap<u16, u64>,
    pub election_timeout: Instant,
    pub election_timeout_range: ElectionTimeoutRange,
    pub tally: HashSet<u16>,
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
            tally: Default::default(),
        }
    }
}

impl State {
    pub fn init(opts: Options) -> NodeState {
        let mut state = State::default();
        let (sender, recv) = tokio::sync::mpsc::unbounded_channel();
        state.id = Uuid::new_v4();
        state.election_timeout_range.pick_timeout_value();

        for seed in opts.seeds {
            state.seeds.insert(
                seed,
                Seed {
                    mailbox: sender.clone(),
                    host: "127.0.0.1".to_string(),
                    port: seed,
                    client: None,
                },
            );
        }
        let node_state = NodeState::new(sender, state);

        ticking::spawn_ticking_process(node_state.clone());
        vote_listener::spawn_incoming_msg_listener(node_state.clone(), recv);

        node_state
    }

    pub fn election_timeout(&self) -> bool {
        self.election_timeout.elapsed() >= self.election_timeout_range.duration
    }

    pub fn switch_to_leader(&mut self) {
        self.status = Status::Leader;
        self.tally.clear();
        self.voted_for = None;

        let last_log_index = self.entries.last_index();
        // Send heartbeat request to assert dominance.
        for seed in self.seeds.values_mut() {
            self.next_index
                .entry(seed.port)
                .or_insert(last_log_index + 1);

            let prev_log_index = *self.match_index.entry(seed.port).or_insert(0);
            let prev_log_term = self.entries.entry_term(prev_log_index);
            seed.send_heartbeat(
                self.term,
                self.id,
                prev_log_index,
                prev_log_term,
                self.commit_index,
            );
        }
    }

    pub fn switch_to_candidate(&mut self, vote_sender: UnboundedSender<IncomingMsg>) {
        self.status = Status::Candidate;
        self.term += 1;
        self.voted_for = Some(self.id);

        let last_log_index = self.entries.last_index();
        let last_log_term = self.entries.last_term();

        for seed in self.seeds.values_mut() {
            seed.request_vote(self.term, self.id, last_log_index, last_log_term);
        }
    }
}

#[derive(Clone)]
pub struct NodeState {
    vote_sender: UnboundedSender<IncomingMsg>,
    inner: Arc<Mutex<State>>,
}

impl NodeState {
    pub fn new(vote_sender: UnboundedSender<IncomingMsg>, inner: State) -> Self {
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
        entries: Vec<Entry>,
    ) -> (u64, bool) {
        let mut state = self.inner.lock().await;

        if state.term > term {
            return (state.term, false);
        }

        if state.term < term {
            state.voted_for = None;
            state.term = term;
        }

        // I'm expecting  it to be safe resetting the election timeout in this case.
        // Even if the replication request doesn't have a shared point of reference, the
        // term is a valid value and it seems we have an healthy active leader just
        // figuring out.
        state.election_timeout = Instant::now();
        state.status = Status::Follower;

        // Current node doesn't have that point of reference from this index position.
        if !state.entries.contains_log(prev_log_index, prev_log_term) {
            return (state.term, false);
        }

        // Means it's a heartbeat message.
        if entries.is_empty() {
            return (state.term, true);
        }

        if state.entries.last_index() > prev_log_index && state.entries.last_term() != term {
            state
                .entries
                .remove_uncommitted_entries_from(prev_log_index, prev_log_term);
        }

        if leader_commit > state.commit_index {
            state.commit_index = min(leader_commit, entries.last().unwrap().index);
        }

        state.entries.append(entries);

        (state.term, true)
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

    pub async fn on_vote_received(&self, seed_port: u16, term: u64, granted: bool) {
        let mut state = self.inner.lock().await;

        // Probably out-of-order message.
        if state.term < term {
            return;
        }

        if state.term > term {
            state.status = Status::Follower;
            state.election_timeout_range.pick_timeout_value();
            state.election_timeout = Instant::now();
            state.voted_for = None;
            state.next_index.clear();
            state.match_index.clear();
            return;
        }

        if granted {
            state.tally.insert(seed_port);

            // Means we reached consensus, we can move to Leader status.
            if state.tally.len() > state.seeds.len() / 2 {
                state.switch_to_leader();
                return;
            }
        }
    }

    pub async fn on_append_entries_resp(
        &self,
        seed_port: u16,
        prev_log_index: u64,
        prev_log_term: u64,
        resp: tonic::Result<AppendEntriesResp>,
    ) {
        let mut state = self.inner.lock().await;
    }
}
