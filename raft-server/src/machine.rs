use crate::entry::Entries;
use crate::state::{ElectionTimeoutRange, Status};
use raft_common::Entry;
use std::cmp::min;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::Instant;
use tokio::sync::oneshot;
use uuid::Uuid;

pub enum Msg {
    RequestVote {
        term: u64,
        candidate_id: Uuid,
        last_log_index: u64,
        last_log_term: u64,
        resp: oneshot::Sender<(u64, bool)>,
    },

    AppendEntries {
        term: u64,
        leader_id: Uuid,
        prev_log_index: u64,
        prev_log_term: u64,
        leader_commit: u64,
        entries: Vec<Entry>,
        resp: oneshot::Sender<(u64, bool)>,
    },

    VotedReceived {
        node_id: Uuid,
        term: u64,
        granted: bool,
    },

    Tick,
}

pub struct Persistent {
    pub id: Uuid,
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
    pub status: Status,
    pub voted_for: Option<Uuid>,
    pub next_index: HashMap<u16, u64>,
    pub match_index: HashMap<u16, u64>,
    pub election_timeout: Instant,
    pub election_timeout_range: ElectionTimeoutRange,
    pub tally: HashSet<Uuid>,
}

impl Default for Volatile {
    fn default() -> Self {
        Self {
            status: Status::Follower,
            voted_for: None,
            next_index: Default::default(),
            match_index: Default::default(),
            election_timeout: Instant::now(),
            election_timeout_range: Default::default(),
            tally: Default::default(),
        }
    }
}

#[derive(Clone)]
pub struct Node {
    sender: mpsc::Sender<Msg>,
}

impl Node {
    pub async fn request_vote(
        &self,
        term: u64,
        candidate_id: Uuid,
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
        leader_id: Uuid,
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

    pub async fn tick(&self) {
        if self.sender.send(Msg::Tick).is_err() {
            panic!("Node is down");
        }
    }
}

pub fn start(persistent: Persistent) -> Node {
    let (sender, recv) = mpsc::channel();

    thread::spawn(move || state_machine(persistent, recv));

    Node { sender }
}

fn state_machine(mut persistent: Persistent, mailbox: Receiver<Msg>) {
    let cluster_size = 3usize;
    let mut volatile = Volatile::default();

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

            Msg::VotedReceived {
                node_id,
                term,
                granted,
            } => {
                on_vote_received(
                    &mut persistent,
                    &mut volatile,
                    cluster_size,
                    node_id,
                    term,
                    granted,
                );
            }

            Msg::Tick => {
                on_tick(&mut persistent, &mut volatile);
            }
        }
    }
}

fn on_vote_received(
    persistent: &mut Persistent,
    volatile: &mut Volatile,
    cluster_size: usize,
    node_id: Uuid,
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

        // Means we reached quorum, we can move to Leader status.
        if volatile.tally.len() > cluster_size / 2 {
            // state.switch_to_leader();
            return;
        }
    }
}

fn on_append_entries(
    persistent: &mut Persistent,
    volatile: &mut Volatile,
    term: u64,
    leader_id: Uuid,
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

fn on_request_vote(
    persistent: &mut Persistent,
    volatile: &mut Volatile,
    term: u64,
    candidate_id: Uuid,
    last_log_index: u64,
    last_log_term: u64,
) -> (u64, bool) {
    if persistent.term > term {
        return (0, false);
    }

    if let Some(id) = volatile.voted_for {
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

fn on_tick(persistent: &mut Persistent, volatile: &mut Volatile) {}
