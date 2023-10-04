use bytes::Bytes;
use clap::Parser;
use raft_common::server::RaftServer;
use raft_common::{EntriesReq, EntriesResp, VoteReq, VoteResp};
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Server;
use tonic::{transport, Request, Response, Status};

#[derive(Default)]
struct State {
    entries: Vec<Bytes>,
    term: u64,
    writer: u64,
}

#[derive(Clone)]
struct RaftImpl {
    state: Arc<Mutex<State>>,
}

impl RaftImpl {
    fn new(state: Arc<Mutex<State>>) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl raft_common::server::Raft for RaftImpl {
    async fn request_vote(&self, request: Request<VoteReq>) -> Result<Response<VoteResp>, Status> {
        Err(Status::unimplemented(
            "resquest_vote isn't implemented yet!",
        ))
    }

    async fn append_entries(
        &self,
        request: Request<EntriesReq>,
    ) -> Result<Response<EntriesResp>, Status> {
        let request = request.into_inner();

        if request.entries.is_empty() {
            let state = self.state.lock().await;

            return Ok(Response::new(EntriesResp {
                term: state.term,
                success: true,
            }));
        }

        Err(Status::unimplemented(
            "append_entries isn't implemented yet!",
        ))
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
        .add_service(RaftServer::new(RaftImpl::new(state)))
        .serve(addr)
        .await?;

    Ok(())
}
