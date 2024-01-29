use clap::Parser;
use raft_server::{options, Node};
use tracing_appender::rolling;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let opts = options::Options::parse();
    let logs = rolling::daily("./logs", format!("node_{}", opts.port));
    tracing_subscriber::fmt()
        .with_file(true)
        .with_ansi(false)
        .with_writer(logs)
        .init();

    let mut node = Node::new(tokio::runtime::Handle::current(), opts)?;
    node.start();

    node.wait_for_completion();
    Ok(())
}
