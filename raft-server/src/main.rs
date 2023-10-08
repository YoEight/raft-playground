mod state;

use crate::state::{NodeState, State};
use bytes::Bytes;
use clap::Parser;
use raft_common::server::RaftServer;
use raft_common::{EntriesReq, EntriesResp, VoteReq, VoteResp};
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Server;
use tonic::{transport, Request, Response, Status};

#[derive(Clone)]
struct RaftImpl {
    state: NodeState,
}

impl RaftImpl {
    fn new(state: NodeState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl raft_common::server::Raft for RaftImpl {
    async fn request_vote(&self, request: Request<VoteReq>) -> Result<Response<VoteResp>, Status> {
        let vote = request.into_inner();
        let candidate_id = match vote.candidate_id.parse() {
            Err(e) => {
                return Err(Status::invalid_argument(format!(
                    "Invalid candidate_id format: {}",
                    e
                )))
            }

            Ok(id) => id,
        };

        let (term, vote_granted) = self
            .state
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

        if request.entries.is_empty() {
            let term = self.state.current_term().await;

            return Ok(Response::new(EntriesResp {
                term,
                success: true,
            }));
        }

        let leader_id = match request.leader_id.parse() {
            Err(e) => {
                return Err(Status::invalid_argument(format!(
                    "Invalid candidate_id format: {}",
                    e
                )))
            }

            Ok(id) => id,
        };

        let (term, success) = self
            .state
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

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Options {
    #[arg(long)]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<(), transport::Error> {
    let opts = Options::parse();
    let addr = format!("127.0.0.1:{}", opts.port).parse().unwrap();

    println!("Listening on {}", addr);

    let state = Arc::new(Mutex::new(State::default()));

    Server::builder()
        .add_service(RaftServer::new(RaftImpl::new(NodeState::default())))
        .serve(addr)
        .await?;

    Ok(())
}
