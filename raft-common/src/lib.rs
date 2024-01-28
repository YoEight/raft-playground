mod proto {
    tonic::include_proto!("consensus");
    tonic::include_proto!("api");
}

pub mod client {
    pub use super::proto::api_client::*;
    pub use super::proto::raft_client::*;
}

pub mod server {
    pub use super::proto::api_server::*;
    pub use super::proto::raft_server::*;
}

pub use proto::{
    AppendReq, AppendResp, EntriesReq, EntriesResp, Entry, NodeId, ReadReq, ReadResp, StatusResp,
    VoteReq, VoteResp,
};
