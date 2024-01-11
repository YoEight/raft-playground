use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Commands {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Exit the application.
    Exit,
    /// Exit the application.
    Quit,
    Spawn(Spawn),
    Stop(Stop),
    Start(Start),
    SendEvent(SendEvent),
    /// Ping a server node.
    Ping(Ping),
}

#[derive(Args, Debug, Clone)]
/// Spawns a cluster of nodes.
pub struct Spawn {
    #[arg(long)]
    pub count: usize,
}

#[derive(Args, Debug, Clone)]
/// Stops a node.
pub struct Stop {
    #[arg(long)]
    pub node: usize,
}

#[derive(Args, Debug, Clone)]
/// Starts a node.
pub struct Start {
    #[arg(long)]
    pub node: usize,

    #[arg(long = "extern")]
    pub external: bool,

    #[arg(long)]
    pub port: Option<usize>,
}

#[derive(Args, Debug, Clone)]
/// Sends an event to a node.
pub struct SendEvent {
    #[arg(long)]
    pub node: usize,
}

#[derive(Args, Debug, Clone)]
pub struct Ping {
    #[command(subcommand)]
    pub command: PingCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum PingCommand {
    External(PingExternal),
}

#[derive(Args, Debug, Clone)]
pub struct PingExternal {
    #[arg(long)]
    pub host: String,

    #[arg(long)]
    pub port: u16,
}
