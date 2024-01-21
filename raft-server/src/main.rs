use clap::Parser;
use raft_server::{options, Node};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let node = Node::new(options::Options::parse())?;
    node.start().await??;

    Ok(())
}
