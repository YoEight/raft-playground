use crate::entry::{Entries, RecordedEvent};
use crate::seed::Seed;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use eyre::bail;
use raft_common::{Entry, NodeId};
use rand::{thread_rng, Rng};
use std::cmp::min;
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::sync::mpsc::Receiver;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tracing::info;

const HEARTBEAT_DELAY: Duration = Duration::from_millis(30);

#[derive(Debug)]
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
        batch_end_log_index: u64,
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

    Status {
        resp: oneshot::Sender<StatusResp>,
    },
}

#[derive(Debug)]
pub struct AppendEntriesResp {
    pub term: u64,
    pub success: bool,
}

#[derive(Debug)]
pub struct StatusResp {
    pub id: NodeId,
    pub status: Status,
    pub leader_id: Option<NodeId>,
    pub term: u64,
    pub log_index: u64,
    pub global: u64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Status {
    Follower,
    Candidate,
    Leader,
}

impl Display for Status {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::Follower => write!(f, "follower"),
            Status::Candidate => write!(f, "candidate"),
            Status::Leader => write!(f, "leader"),
        }
    }
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
    pub leader: Option<NodeId>,
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
        // If the seed list is empty it means we are in a single node configuration.
        let status = if seeds.is_empty() {
            Status::Leader
        } else {
            Status::Follower
        };

        Self {
            id,
            seeds,
            status,
            leader: None,
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

#[derive(Clone, Debug)]
pub struct NodeClient {
    sender: mpsc::Sender<Msg>,
}

impl NodeClient {
    pub fn new(sender: mpsc::Sender<Msg>) -> Self {
        Self { sender }
    }

    pub async fn request_vote(
        &self,
        term: u64,
        candidate_id: NodeId,
        last_log_index: u64,
        last_log_term: u64,
    ) -> eyre::Result<(u64, bool)> {
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
            bail!("Node is down");
        }

        if let Ok(res) = recv.await {
            Ok(res)
        } else {
            bail!("Unexpected error when asked to vote")
        }
    }

    pub async fn append_entries(
        &self,
        term: u64,
        leader_id: NodeId,
        prev_log_index: u64,
        prev_log_term: u64,
        leader_commit: u64,
        entries: Vec<Entry>,
    ) -> eyre::Result<(u64, bool)> {
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
            bail!("Node is down");
        }

        if let Ok(tuple) = recv.await {
            Ok(tuple)
        } else {
            bail!("Unexpected when asked to replicate entries")
        }
    }

    pub async fn append_stream(
        &self,
        stream_id: String,
        events: Vec<Bytes>,
    ) -> eyre::Result<Option<u64>> {
        let (resp, recv) = oneshot::channel();
        if self
            .sender
            .send(Msg::AppendStream {
                stream_id: stream_id.clone(),
                events,
                resp,
            })
            .is_err()
        {
            bail!("Node is down");
        }

        if let Ok(resp) = recv.await {
            Ok(resp)
        } else {
            bail!("Unexpected error when appending stream '{}'", stream_id)
        }
    }

    pub async fn read_stream(&self, stream_id: String) -> eyre::Result<Option<Vec<RecordedEvent>>> {
        let (resp, recv) = oneshot::channel();
        if self
            .sender
            .send(Msg::ReadStream {
                stream_id: stream_id.clone(),
                resp,
            })
            .is_err()
        {
            bail!("Node is down");
        }

        if let Ok(batch) = recv.await {
            Ok(batch)
        } else {
            bail!("Unexpected error when looking for stream '{}'", stream_id)
        }
    }

    pub async fn status(&self) -> eyre::Result<StatusResp> {
        let (resp, recv) = oneshot::channel();

        if self.sender.send(Msg::Status { resp }).is_err() {
            bail!("Node is down");
        }

        if let Ok(status) = recv.await {
            Ok(status)
        } else {
            bail!("Unexpected error when querying the status")
        }
    }

    pub fn vote_received(&self, node_id: NodeId, term: u64, granted: bool) {
        let _ = self.sender.send(Msg::VoteReceived {
            node_id,
            term,
            granted,
        });
    }

    pub fn append_entries_response_received(
        &self,
        node_id: NodeId,
        batch_end_log_index: u64,
        resp: Result<AppendEntriesResp, tonic::Status>,
    ) {
        let _ = self.sender.send(Msg::AppendEntriesResp {
            node_id,
            batch_end_log_index,
            resp,
        });
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
    let mut volatile = Volatile::new(node_id.clone(), seeds);
    let mut tick_tracker = Instant::now();

    loop {
        let elapsed = tick_tracker.elapsed();
        if tick_tracker.elapsed() >= HEARTBEAT_DELAY {
            info!(
                "node_{}:{} [term={}] Tick tracker elapsed {:?}",
                node_id.host, node_id.port, persistent.term, elapsed
            );
            on_tick(&mut persistent, &mut volatile);
            tick_tracker = Instant::now();
        }

        let msg = match mailbox.recv_timeout(HEARTBEAT_DELAY) {
            Ok(msg) => msg,
            Err(e) => match e {
                RecvTimeoutError::Disconnected => return,
                RecvTimeoutError::Timeout => {
                    info!(
                        "node_{}:{} [term={}] Tick tracker elapsed {:?}",
                        node_id.host,
                        node_id.port,
                        persistent.term,
                        tick_tracker.elapsed()
                    );
                    on_tick(&mut persistent, &mut volatile);
                    tick_tracker = Instant::now();
                    continue;
                }
            },
        };

        info!(
            "node_{}:{} [term={}] received {:?}",
            node_id.host, node_id.port, persistent.term, msg
        );

        match msg {
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

            Msg::AppendEntriesResp {
                node_id,
                batch_end_log_index,
                resp,
            } => on_append_entries_resp(&mut volatile, node_id, batch_end_log_index, resp),
            Msg::AppendStream {
                stream_id,
                events,
                resp,
            } => {
                if volatile.status != Status::Leader {
                    let _ = resp.send(None);
                    continue;
                }

                on_append_stream(&mut persistent, stream_id, events, resp);
            }

            Msg::ReadStream { stream_id, resp } => {
                if volatile.status != Status::Leader {
                    let _ = resp.send(None);
                    continue;
                }

                on_read_stream(&mut persistent, stream_id, resp);
            }

            Msg::Status { resp } => {
                on_status(&persistent, &volatile, resp);
            }
        }
    }
}

fn on_status(persistent: &Persistent, volatile: &Volatile, resp: oneshot::Sender<StatusResp>) {
    let _ = resp.send(StatusResp {
        id: volatile.id.clone(),
        status: volatile.status,
        leader_id: volatile.leader.clone(),
        term: persistent.term,
        log_index: persistent.commit_index,
        global: 0,
    });
}

fn on_read_stream(
    persistent: &mut Persistent,
    stream_id: String,
    resp: oneshot::Sender<Option<Vec<RecordedEvent>>>,
) {
    let mut global = 0;
    let mut revision = 0;
    let stream_id_bytes = stream_id.as_bytes();
    let mut batch = Vec::new();

    for entry in persistent.entries.entries() {
        let mut payload = entry.payload.clone();
        let str_len = payload.get_u32_le();
        let str_bytes = payload.copy_to_bytes(str_len as usize);

        if str_bytes == stream_id_bytes {
            batch.push(RecordedEvent {
                stream_id: stream_id.clone(),
                global,
                revision,
                payload,
            });

            revision += 1;
        } else {
            global += 1;
        }
    }

    let _ = resp.send(Some(batch));
}

fn on_append_stream(
    persistent: &mut Persistent,
    stream_id: String,
    events: Vec<Bytes>,
    resp: oneshot::Sender<Option<u64>>,
) {
    let stream_id_len = stream_id.chars().count() as u32;
    let mut index = persistent.entries.last_index();
    let term = persistent.term;
    let mut buffer = BytesMut::new();
    for event in events {
        buffer.put_u32_le(stream_id_len);
        buffer.put(stream_id.as_bytes());
        buffer.put(event);
        index += 1;

        persistent.entries.add(Entry {
            index,
            term,
            payload: buffer.split().freeze(),
        });
    }

    let _ = resp.send(Some(index));
}

pub fn on_vote_received(
    persistent: &mut Persistent,
    volatile: &mut Volatile,
    node_id: NodeId,
    term: u64,
    granted: bool,
) {
    // Probably out-of-order message.
    if persistent.term > term || volatile.status == Status::Leader {
        return;
    }

    if persistent.term < term {
        let prev_term = persistent.term;
        persistent.term = term;
        volatile.status = Status::Follower;
        volatile.leader = Some(node_id.clone());
        volatile.election_timeout_range.pick_timeout_value();
        volatile.election_timeout = Instant::now();
        volatile.voted_for = None;
        volatile.next_index.clear();
        volatile.match_index.clear();

        info!(
            "Node_{}:{} [term={}]switched to follower because received an higher [term={}] from vote for {}:{}",
            volatile.id.host, volatile.id.port, prev_term, term, node_id.host, node_id.port,
        );
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

    let prev_term = persistent.term;

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
    volatile.leader = Some(leader_id);

    info!(
        "Node_{}:{} [term={}] is a follower because received a valid replication request -> [term={}]",
        volatile.id.host, volatile.id.port, prev_term, term
    );
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
        return (persistent.term, false);
    }

    if persistent.term < term || volatile.voted_for.is_none() {
        persistent.term = term;
        let mut granted = false;

        if persistent.entries.last_index() <= last_log_index
            && persistent.entries.last_term() <= last_log_term
        {
            volatile.voted_for = Some(candidate_id);
            volatile.status = Status::Follower;
            info!(
                "Node_{}:{} switch to follower because received a valid vote request",
                volatile.id.host, volatile.id.port
            );

            granted = true;
        }

        return (persistent.term, granted);
    }

    // When current term and term are equal, we check that the candidate matches and we have common history.
    (
        persistent.term,
        volatile.voted_for == Some(candidate_id)
            && persistent.entries.last_index() <= last_log_index
            && persistent.entries.last_term() <= last_log_term,
    )
}

pub fn on_append_entries_resp(
    volatile: &mut Volatile,
    node_id: NodeId,
    batch_end_log_index: u64,
    resp: tonic::Result<AppendEntriesResp>,
) {
    if volatile.status != Status::Leader {
        return;
    }

    if let Ok(resp) = resp {
        if resp.success {
            // We node that node successfully replicated to that point.
            *volatile.match_index.get_mut(&node_id).unwrap() = batch_end_log_index;
        } else {
            // In this case we decrease what we thought was the next entry index of the follower
            // node. From there we find the previous entry to this index and pass it as a point of
            // reference to maintain log consistency.
            let next_index = volatile.next_index.get_mut(&node_id).unwrap();
            *next_index = next_index.saturating_sub(1);
        }

        return;
    }

    // In that case, the next tick will retry at the same next_match position of the log
    // because it appears the node is not reachable.
}

fn switch_to_leader(persistent: &mut Persistent, volatile: &mut Volatile) {
    volatile.status = Status::Leader;
    volatile.leader = Some(volatile.id.clone());
    volatile.reset();

    info!(
        "Node_{}:{} [term={}] switched to leader",
        volatile.id.host, volatile.id.port, persistent.term
    );

    // Send heartbeat request to assert dominance.
    send_append_entries(persistent, volatile);
}

fn switch_to_candidate(persistent: &mut Persistent, volatile: &mut Volatile) {
    let prev_term = persistent.term;
    volatile.status = Status::Candidate;
    volatile.leader = None;
    persistent.term += 1;
    volatile.voted_for = Some(persistent.id.clone());
    volatile.election_timeout_range.pick_timeout_value();

    info!(
        "Node_{}:{} [term={}] switched to candidate with election timeout {:?} -> term {}",
        volatile.id.host,
        volatile.id.port,
        prev_term,
        volatile.election_timeout_range.duration,
        persistent.term,
    );

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

pub fn send_append_entries(persistent: &mut Persistent, volatile: &mut Volatile) {
    let last_log_index = persistent.entries.last_index();

    for seed in volatile.seeds.clone() {
        let next_index = volatile
            .next_index
            .entry(seed.id.clone())
            .or_insert(last_log_index + 1);

        let (prev_log_index, prev_log_term) = persistent.entries.get_previous_entry(*next_index);

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
}

pub fn on_tick(persistent: &mut Persistent, volatile: &mut Volatile) {
    // Means we are running in single node.
    if volatile.seeds.is_empty() {
        return;
    }

    // Note: I think the make reason we keep having cluster stability issues is because of the heartbeat delay that
    // might not be short enough (based on the election timeout) causing followers to switch to candidate too often.
    if volatile.status == Status::Leader {
        info!(
            "node_{}:{} Actual heartbeat delay: {:?}",
            volatile.id.host,
            volatile.id.port,
            volatile.heartbeat_timeout.elapsed()
        );

        send_append_entries(persistent, volatile);
    } else if volatile.election_timeout() {
        switch_to_candidate(persistent, volatile);
    }
}
