mod entry;
mod env;
mod id;
mod machine;
mod options;
mod state;
mod ticking;
mod vote_listener;

use crate::machine::{Node, Persistent};
use crate::state::{NodeState, State};
use clap::Parser;
use raft_common::server::RaftServer;
use raft_common::{EntriesReq, EntriesResp, VoteReq, VoteResp};
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
    let node = machine::start(persistent);

    Server::builder()
        .add_service(RaftServer::new(RaftImpl::new(node)))
        .serve(addr)
        .await?;

    Ok(())
}
