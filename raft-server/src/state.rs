use bytes::Bytes;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Default)]
pub struct State {
    pub entries: Vec<Bytes>,
    pub term: u64,
    pub writer: u64,
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
        (0, false)
    }

    pub async fn current_term(&self) -> u64 {
        let state = self.inner.lock().await;
        state.term
    }
}
