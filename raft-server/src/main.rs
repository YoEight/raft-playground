use clap::Parser;
use raft_server::{options, Node};
use tonic::transport;

#[tokio::main]
async fn main() -> Result<(), transport::Error> {
    let node = Node::new(options::Options::parse());
    if let Err(e) = node.start().await {
        panic!("Unexpected error: {}", e);
    }

    Ok(())
}
