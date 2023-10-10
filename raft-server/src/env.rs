use hyper::client::HttpConnector;
use raft_common::client::RaftClient;
use tonic::transport::Channel;
use uuid::Uuid;

pub type HyperClient = hyper::Client<HttpConnector, tonic::body::BoxBody>;

#[derive(Debug)]
pub struct Seed {
    pub host: String,
    pub port: u16,
    pub client: Option<RaftClient<HyperClient>>,
}
