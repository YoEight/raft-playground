use crate::env::Seed;
use bytes::Bytes;
use rand::{thread_rng, Rng};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
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

// #[derive(Default)]
pub struct State {
    pub id: Uuid,
    pub seeds: HashMap<u16, Seed>,
    pub entries: Vec<Bytes>,
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
            entries: vec![],
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
                    id: Uuid::nil(),
                    host: "127.0.0.1".to_string(),
                    port: seed,
                    channel: None,
                },
            );
        }

        this
    }

    pub fn init(&mut self) {
        self.id = Uuid::new_v4();
        self.election_timeout_range.pick_timeout_value();
    }

    pub fn election_timeout(&self) -> bool {
        self.election_timeout.elapsed() >= self.election_timeout_range.duration
    }
}

#[derive(Clone)]
pub struct NodeState {
    inner: Arc<Mutex<State>>,
}

impl Default for NodeState {
    fn default() -> Self {
        NodeState::new(State::default())
    }
}

impl NodeState {
    pub fn new(inner: State) -> Self {
        Self {
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
        (0, false)
    }

    pub async fn append_entries(
        &self,
        term: u64,
        leader_id: Uuid,
        pre_log_index: u64,
        prev_log_term: u64,
        leader_commit: u64,
        entries: Vec<Bytes>,
    ) -> (u64, bool) {
        let mut state = self.inner.lock().await;

        if state.status != Status::Follower {
            // we update our leader healthcheck detector;
            state.election_timeout = Instant::now();
        }

        // Means it's a heartbeat message.
        if entries.is_empty() {
            return (state.term, true);
        }

        (0, false)
    }

    pub async fn current_term(&self) -> u64 {
        let state = self.inner.lock().await;
        state.term
    }

    pub async fn on_ticking(&self) {
        let state = self.inner.lock().await;

        // Means we are running in single node.
        if state.seeds.is_empty() {
            return;
        }

        if state.election_timeout() {
            // TODO - Moving to candidate status.
            return;
        }
    }
}
