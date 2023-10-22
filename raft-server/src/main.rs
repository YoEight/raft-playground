mod entry;
mod id;
mod machine;
mod options;
mod seed;

use crate::machine::{Node, Persistent};
use crate::seed::Seed;
use clap::Parser;
use raft_common::server::RaftServer;
use raft_common::{EntriesReq, EntriesResp, NodeId, VoteReq, VoteResp};
use std::sync::mpsc;
use tonic::transport::Server;
use tonic::{transport, Request, Response, Status};

#[derive(Clone)]
struct RaftImpl {
    node: Node,
}

impl RaftImpl {
    fn new(node: Node) -> Self {
        Self { node }
    }
}

#[tonic::async_trait]
impl raft_common::server::Raft for RaftImpl {
    async fn request_vote(&self, request: Request<VoteReq>) -> Result<Response<VoteResp>, Status> {
        let vote = request.into_inner();
        let candidate_id = if let Some(id) = vote.candidate_id {
            id
        } else {
            return Err(Status::invalid_argument("Candidate id is not provided"));
        };

        let (term, vote_granted) = self
            .node
            .request_vote(
                vote.term,
                candidate_id,
                vote.last_log_index,
                vote.last_log_term,
            )
            .await;

        Ok(Response::new(VoteResp { term, vote_granted }))
    }

    async fn append_entries(
        &self,
        request: Request<EntriesReq>,
    ) -> Result<Response<EntriesResp>, Status> {
        let request = request.into_inner();

        let leader_id = if let Some(id) = request.leader_id {
            id
        } else {
            return Err(Status::invalid_argument("Candidate id not provided"));
        };

        let (term, success) = self
            .node
            .append_entries(
                request.term,
                leader_id,
                request.prev_log_index,
                request.prev_log_term,
                request.leader_commit,
                request.entries,
            )
            .await;

        Ok(Response::new(EntriesResp { term, success }))
    }
}

#[tokio::main]
async fn main() -> Result<(), transport::Error> {
    let opts = options::Options::parse();
    let addr = format!("127.0.0.1:{}", opts.port).parse().unwrap();

    println!("Listening on {}", addr);

    let persistent = Persistent::load();
    let mut seeds = Vec::new();
    let (sender, mailbox) = mpsc::channel();

    let node_id = NodeId {
        host: "127.0.0.1".to_string(),
        port: opts.port as u32,
    };

    for seed_port in opts.seeds {
        let node_id = NodeId {
            host: "127.0.0.1".to_string(),
            port: seed_port as u32,
        };

        seeds.push(Seed::new(node_id, sender.clone()));
    }

    let handle = machine::start(persistent, node_id, seeds, mailbox);
    let node = Node::new(sender);

    Server::builder()
        .add_service(RaftServer::new(RaftImpl::new(node)))
        .serve(addr)
        .await?;

    Ok(())
}
