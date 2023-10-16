mod proto {
    tonic::include_proto!("consensus");
}

pub mod client {
    pub use super::proto::raft_client::*;
}

pub mod server {
    pub use super::proto::raft_server::*;
}

pub use proto::{EntriesReq, EntriesResp, Entry, VoteReq, VoteResp};
