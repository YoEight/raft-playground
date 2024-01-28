use futures::stream::BoxStream;
use raft_common::{
    AppendReq, AppendResp, EntriesReq, EntriesResp, ReadResp, StatusResp, VoteReq, VoteResp,
};
use tonic::{Request, Response, Status};

use crate::machine::NodeClient;

#[derive(Clone)]
pub struct RaftImpl {
    node: NodeClient,
}

impl RaftImpl {
    pub fn new(node: NodeClient) -> Self {
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
pub struct ApiImpl {
    node: NodeClient,
}

impl ApiImpl {
    pub fn new(node: NodeClient) -> Self {
        Self { node }
    }
}

#[tonic::async_trait]
impl raft_common::server::Api for ApiImpl {
    async fn append(&self, request: Request<AppendReq>) -> Result<Response<AppendResp>, Status> {
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

    async fn status(&self, _: Request<()>) -> Result<Response<StatusResp>, Status> {
        let snapshot = self.node.status().await;

        Ok(Response::new(StatusResp {
            host: snapshot.id.host,
            port: snapshot.id.port,
            term: snapshot.term,
            status: snapshot.status.to_string(),
            log_index: snapshot.log_index,
            global: snapshot.global,
            leader_host: snapshot
                .leader_id
                .as_ref()
                .map(|id| id.host.clone())
                .unwrap_or_default(),
            leader_port: snapshot
                .leader_id
                .as_ref()
                .map(|id| id.port)
                .unwrap_or_default(),
        }))
    }
}
