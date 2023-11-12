mod entry;
mod id;
mod machine;
mod options;
mod seed;

use crate::machine::{Msg, Node, Persistent};
use crate::seed::Seed;
use clap::Parser;
use futures::stream::BoxStream;
use raft_common::server::{ApiServer, RaftServer};
use raft_common::{AppendResp, EntriesReq, EntriesResp, NodeId, ReadResp, VoteReq, VoteResp};
use std::sync::mpsc;
use std::time::Duration;
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

#[derive(Clone)]
struct ApiImpl {
    node: Node,
}

impl ApiImpl {
    pub fn new(node: Node) -> Self {
        Self { node }
    }
}

#[tonic::async_trait]
impl raft_common::server::Api for ApiImpl {
    async fn append(
        &self,
        request: Request<raft_common::AppendReq>,
    ) -> Result<Response<raft_common::AppendResp>, Status> {
        let request = request.into_inner();
        if let Some(next_position) = self
            .node
            .append_stream(request.stream_id, request.events)
            .await
        {
            Ok(Response::new(AppendResp {
                position: next_position,
            }))
        } else {
            Err(Status::failed_precondition("not-leader"))
        }
    }

    type ReadStream = BoxStream<'static, Result<ReadResp, Status>>;

    async fn read(
        &self,
        request: Request<raft_common::ReadReq>,
    ) -> Result<Response<Self::ReadStream>, Status> {
        let request = request.into_inner();
        if let Some(events) = self.node.read_stream(request.stream_id).await {
            let streaming = async_stream::stream! {
                for event in events {
                    yield Ok(ReadResp {
                        stream_id: event.stream_id,
                        global: event.global,
                        revision: event.revision,
                        payload: event.payload,
                    });
                }
            };

            Ok(Response::new(Box::pin(streaming)))
        } else {
            Err(Status::failed_precondition("not-leader"))
        }
    }

    async fn ping(&self, _request: Request<()>) -> Result<Response<()>, Status> {
        Ok(Response::new(()))
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
    let mut ticking = tokio::time::interval(Duration::from_millis(5));
    let tick_send = sender.clone();

    tokio::spawn(async move {
        loop {
            ticking.tick().await;

            if tick_send.send(Msg::Tick).is_err() {
                break;
            }
        }
    });

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
        .add_service(RaftServer::new(RaftImpl::new(node.clone())))
        .add_service(ApiServer::new(ApiImpl::new(node)))
        .serve(addr)
        .await?;

    let _ = handle.join();
    Ok(())
}
