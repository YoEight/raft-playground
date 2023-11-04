use crate::entry::{Entries, RecordedEvent};
use crate::seed::Seed;
use bytes::Bytes;
use raft_common::{Entry, NodeId};
use rand::{thread_rng, Rng};
use std::cmp::min;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

const HEARTBEAT_DELAY: Duration = Duration::from_millis(30);

pub enum Msg {
    RequestVote {
        term: u64,
        candidate_id: NodeId,
        last_log_index: u64,
        last_log_term: u64,
        resp: oneshot::Sender<(u64, bool)>,
    },

    AppendEntries {
        term: u64,
        leader_id: NodeId,
        prev_log_index: u64,
        prev_log_term: u64,
        leader_commit: u64,
        entries: Vec<Entry>,
        resp: oneshot::Sender<(u64, bool)>,
    },

    VoteReceived {
        node_id: NodeId,
        term: u64,
        granted: bool,
    },

    AppendEntriesResp {
        node_id: NodeId,
        prev_log_index: u64,
        prev_log_term: u64,
        resp: tonic::Result<AppendEntriesResp>,
    },

    AppendStream {
        stream_id: String,
        events: Vec<Bytes>,
        resp: oneshot::Sender<Option<u64>>,
    },

    ReadStream {
        stream_id: String,
        resp: oneshot::Sender<Option<Vec<RecordedEvent>>>,
    },

    Tick,
}

pub struct AppendEntriesResp {
    pub term: u64,
    pub success: bool,
}

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

pub struct Persistent {
    pub id: NodeId,
    pub entries: Entries,
    pub term: u64,
    pub commit_index: u64,
    pub last_applied: u64,
}

impl Persistent {
    pub fn load() -> Self {
        Self {
            id: Default::default(),
            entries: Default::default(),
            term: 0,
            commit_index: 0,
            last_applied: 0,
        }
    }
}

pub struct Volatile {
    pub id: NodeId,
    pub seeds: Vec<Seed>,
    pub status: Status,
    pub voted_for: Option<NodeId>,
    pub next_index: HashMap<NodeId, u64>,
    pub match_index: HashMap<NodeId, u64>,
    pub election_timeout: Instant,
    pub election_timeout_range: ElectionTimeoutRange,
    pub tally: HashSet<NodeId>,
    pub heartbeat_timeout: Instant,
}

impl Volatile {
    pub fn new(id: NodeId, seeds: Vec<Seed>) -> Self {
        Self {
            id,
            seeds,
            status: Status::Follower,
            voted_for: None,
            next_index: Default::default(),
            match_index: Default::default(),
            election_timeout: Instant::now(),
            election_timeout_range: Default::default(),
            tally: Default::default(),
            heartbeat_timeout: Instant::now(),
        }
    }

    pub fn cluster_size(&self) -> usize {
        1 + self.seeds.len()
    }

    pub fn have_we_reached_quorum(&self) -> bool {
        1 + self.tally.len() > self.cluster_size() / 2
    }

    pub fn reset(&mut self) {
        self.voted_for = None;
        self.tally.clear();
        self.match_index.clear();
        self.next_index.clear();
    }

    pub fn election_timeout(&self) -> bool {
        self.election_timeout.elapsed() >= self.election_timeout_range.duration
    }

    pub fn node(&self, node_id: &NodeId) -> &Seed {
        for seed in &self.seeds {
            if &seed.id == node_id {
                return seed;
            }
        }

        panic!("Unknown node {}:{}", node_id.host, node_id.port)
    }
}

#[derive(Clone)]
pub struct Node {
    sender: mpsc::Sender<Msg>,
}

impl Node {
    pub fn new(sender: mpsc::Sender<Msg>) -> Self {
        Self { sender }
    }

    pub async fn request_vote(
        &self,
        term: u64,
        candidate_id: NodeId,
        last_log_index: u64,
        last_log_term: u64,
    ) -> (u64, bool) {
        let (resp, recv) = oneshot::channel();
        if self
            .sender
            .send(Msg::RequestVote {
                term,
                candidate_id,
                last_log_index,
                last_log_term,
                resp,
            })
            .is_err()
        {
            panic!("Node is down");
        }

        recv.await.unwrap()
    }

    pub async fn append_entries(
        &self,
        term: u64,
        leader_id: NodeId,
        prev_log_index: u64,
        prev_log_term: u64,
        leader_commit: u64,
        entries: Vec<Entry>,
    ) -> (u64, bool) {
        let (resp, recv) = oneshot::channel();
        if self
            .sender
            .send(Msg::AppendEntries {
                term,
                leader_id,
                prev_log_index,
                prev_log_term,
                leader_commit,
                entries,
                resp,
            })
            .is_err()
        {
            panic!("Node is down");
        }

        recv.await.unwrap()
    }

    pub async fn append_stream(&self, stream_id: String, events: Vec<Bytes>) -> Option<u64> {
        let (resp, recv) = oneshot::channel();
        if self
            .sender
            .send(Msg::AppendStream {
                stream_id,
                events,
                resp,
            })
            .is_err()
        {
            panic!("Node is down");
        }

        recv.await.unwrap()
    }

    pub async fn read_stream(&self, stream_id: String) -> Option<Vec<RecordedEvent>> {
        let (resp, recv) = oneshot::channel();
        if self
            .sender
            .send(Msg::ReadStream { stream_id, resp })
            .is_err()
        {
            panic!("Node is down");
        }

        recv.await.unwrap()
    }
}

pub fn start(
    persistent: Persistent,
    node_id: NodeId,
    seeds: Vec<Seed>,
    recv: Receiver<Msg>,
) -> JoinHandle<()> {
    thread::spawn(move || state_machine(persistent, node_id, seeds, recv))
}

fn state_machine(
    mut persistent: Persistent,
    node_id: NodeId,
    seeds: Vec<Seed>,
    mailbox: Receiver<Msg>,
) {
    let mut volatile = Volatile::new(node_id, seeds);

    loop {
        match mailbox.recv().unwrap() {
            Msg::RequestVote {
                term,
                candidate_id,
                last_log_index,
                last_log_term,
                resp,
            } => {
                let result = on_request_vote(
                    &mut persistent,
                    &mut volatile,
                    term,
                    candidate_id,
                    last_log_index,
                    last_log_term,
                );

                let _ = resp.send(result);
            }

            Msg::AppendEntries {
                term,
                leader_id,
                prev_log_index,
                prev_log_term,
                leader_commit,
                entries,
                resp,
            } => {
                let result = on_append_entries(
                    &mut persistent,
                    &mut volatile,
                    term,
                    leader_id,
                    prev_log_index,
                    prev_log_term,
                    leader_commit,
                    entries,
                );

                let _ = resp.send(result);
            }

            Msg::VoteReceived {
                node_id,
                term,
                granted,
            } => {
                on_vote_received(&mut persistent, &mut volatile, node_id, term, granted);
            }

            Msg::Tick => {
                on_tick(&mut persistent, &mut volatile);
            }
            Msg::AppendEntriesResp {
                node_id,
                prev_log_index,
                prev_log_term,
                resp,
            } => on_append_entries_resp(
                &mut persistent,
                &mut volatile,
                node_id,
                prev_log_index,
                prev_log_term,
                resp,
            ),
            Msg::AppendStream {
                stream_id,
                events,
                resp,
            } => {
                if volatile.status != Status::Leader {
                    let _ = resp.send(None);
                    continue;
                }

                on_append_stream(&mut persistent, stream_id, resp);
            }

            Msg::ReadStream { stream_id, resp } => {
                if volatile.status != Status::Leader {
                    let _ = resp.send(None);
                    continue;
                }

                on_read_stream(&mut persistent, stream_id, resp);
            }
        }
    }
}

fn on_read_stream(
    persistent: &mut Persistent,
    stream_id: String,
    resp: oneshot::Sender<Option<Vec<RecordedEvent>>>,
) {
    todo!()
}

fn on_append_stream(
    persistent: &mut Persistent,
    stream_id: String,
    resp: oneshot::Sender<Option<u64>>,
) {
    todo!()
}

pub fn on_vote_received(
    persistent: &mut Persistent,
    volatile: &mut Volatile,
    node_id: NodeId,
    term: u64,
    granted: bool,
) {
    // Probably out-of-order message.
    if persistent.term < term {
        return;
    }

    if persistent.term > term {
        volatile.status = Status::Follower;
        volatile.election_timeout_range.pick_timeout_value();
        volatile.election_timeout = Instant::now();
        volatile.voted_for = None;
        volatile.next_index.clear();
        volatile.match_index.clear();
        return;
    }

    if granted {
        volatile.tally.insert(node_id);

        if volatile.have_we_reached_quorum() {
            switch_to_leader(persistent, volatile);
            return;
        }
    }
}

pub fn on_append_entries(
    persistent: &mut Persistent,
    volatile: &mut Volatile,
    term: u64,
    leader_id: NodeId,
    prev_log_index: u64,
    prev_log_term: u64,
    leader_commit: u64,
    entries: Vec<Entry>,
) -> (u64, bool) {
    if persistent.term > term {
        return (persistent.term, false);
    }

    if persistent.term < term {
        volatile.voted_for = None;
        persistent.term = term;
    }

    // I'm expecting  it to be safe resetting the election timeout in this case.
    // Even if the replication request doesn't have a shared point of reference, the
    // term is a valid value and it seems we have an healthy active leader just
    // figuring out.
    volatile.election_timeout = Instant::now();
    volatile.status = Status::Follower;

    // Current node doesn't have that point of reference from this index position.
    if !persistent
        .entries
        .contains_log(prev_log_index, prev_log_term)
    {
        return (persistent.term, false);
    }

    // Means it's a heartbeat message.
    if entries.is_empty() {
        return (persistent.term, true);
    }

    if persistent.entries.last_index() > prev_log_index && persistent.entries.last_term() != term {
        persistent
            .entries
            .remove_uncommitted_entries_from(prev_log_index, prev_log_term);
    }

    if leader_commit > persistent.commit_index {
        persistent.commit_index = min(leader_commit, entries.last().unwrap().index);
    }

    persistent.entries.append(entries);

    (persistent.term, true)
}

pub fn on_request_vote(
    persistent: &mut Persistent,
    volatile: &mut Volatile,
    term: u64,
    candidate_id: NodeId,
    last_log_index: u64,
    last_log_term: u64,
) -> (u64, bool) {
    if persistent.term > term {
        return (0, false);
    }

    if let Some(id) = volatile.voted_for.clone() {
        return (
            persistent.term,
            id == candidate_id && persistent.entries.last_index() <= last_log_index,
        );
    } else {
        if persistent.entries.last_index() <= last_log_index
            && persistent.entries.last_term() <= last_log_term
        {
            volatile.voted_for = Some(candidate_id);
            volatile.status = Status::Follower;
            persistent.term = term;

            return (term, true);
        }

        (0, false)
    }
}

pub fn on_append_entries_resp(
    persistent: &mut Persistent,
    volatile: &mut Volatile,
    node_id: NodeId,
    prev_log_index: u64,
    prev_log_term: u64,
    resp: tonic::Result<AppendEntriesResp>,
) {
    if volatile.status != Status::Leader {
        return;
    }

    if let Ok(resp) = resp {
        if !resp.success {
            // In this case we decrease what we thought was the next entry index of the follower
            // node. From there we find the previous entry to this index and pass it as a point of
            // reference to maintain log consistency.
            let next_index = volatile.next_index.get_mut(&node_id).unwrap();
            *next_index -= 1;

            let (prev_log_index, prev_log_term) =
                persistent.entries.get_previous_entry(*next_index);
            let entries = persistent
                .entries
                .read_entries_from(prev_log_index + 1, 500);
            let seed = volatile.node(&node_id);

            seed.send_append_entries(
                persistent.term,
                volatile.id.clone(),
                prev_log_index,
                prev_log_term,
                persistent.commit_index,
                entries,
            );

            return;
        }
    } else {
        // In that case we just keep retrying because it means the follower was not reachable.
        let seed = volatile.node(&node_id);
        let entries = persistent
            .entries
            .read_entries_from(prev_log_index + 1, 500);

        seed.send_append_entries(
            persistent.term,
            volatile.id.clone(),
            prev_log_index,
            prev_log_term,
            persistent.commit_index,
            entries,
        );
    }
}

fn find_known_reference_point(
    persistent: &mut Persistent,
    volatile: &mut Volatile,
    node_id: NodeId,
) -> (u64, u64) {
    let prev_log_index = *volatile.match_index.entry(node_id).or_insert(0);
    let prev_log_term = persistent.entries.entry_term(prev_log_index);

    (prev_log_index, prev_log_term)
}

fn switch_to_leader(persistent: &mut Persistent, volatile: &mut Volatile) {
    volatile.status = Status::Leader;
    volatile.reset();

    let last_log_index = persistent.entries.last_index();
    // Send heartbeat request to assert dominance.
    for seed in volatile.seeds.clone() {
        volatile
            .next_index
            .entry(seed.id.clone())
            .or_insert(last_log_index + 1);

        let (prev_log_index, prev_log_term) =
            find_known_reference_point(persistent, volatile, seed.id.clone());

        let entries = persistent
            .entries
            .read_entries_from(prev_log_index + 1, 500);

        seed.send_append_entries(
            persistent.term,
            volatile.id.clone(),
            prev_log_index,
            prev_log_term,
            persistent.commit_index,
            entries,
        );
    }
}

fn switch_to_candidate(persistent: &mut Persistent, volatile: &mut Volatile) {
    volatile.status = Status::Candidate;
    persistent.term += 1;
    volatile.voted_for = Some(persistent.id.clone());

    let last_log_index = persistent.entries.last_index();
    let last_log_term = persistent.entries.last_term();

    for seed in &volatile.seeds {
        seed.request_vote(
            persistent.term,
            volatile.id.clone(),
            last_log_index,
            last_log_term,
        );
    }
}

pub fn on_tick(persistent: &mut Persistent, volatile: &mut Volatile) {
    // Means we are running in single node.
    if volatile.seeds.is_empty() {
        return;
    }

    if volatile.heartbeat_timeout.elapsed() >= HEARTBEAT_DELAY && volatile.status == Status::Leader
    {
        for seed in volatile.seeds.clone() {
            let (prev_log_index, prev_log_term) =
                find_known_reference_point(persistent, volatile, seed.id.clone());

            let entries = persistent
                .entries
                .read_entries_from(prev_log_index + 1, 500);

            seed.send_append_entries(
                persistent.term,
                volatile.id.clone(),
                prev_log_index,
                prev_log_term,
                persistent.commit_index,
                entries,
            );
        }

        volatile.heartbeat_timeout = Instant::now();
        return;
    }

    if volatile.election_timeout() && volatile.status != Status::Leader {
        switch_to_candidate(persistent, volatile);
        return;
    }
}
