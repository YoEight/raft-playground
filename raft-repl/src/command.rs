use clap::{Args, Parser, Subcommand, ValueEnum};

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
    Restart(Restart),
    AppendToStream(AppendToStream),
    ReadStream(ReadStream),
    /// Ping a server node.
    Ping(Ping),

    /// Read status from a node.
    Status(StatusNode),
}

#[derive(Args, Debug, Clone)]
/// Spawns a cluster of nodes.
pub struct Spawn {
    #[arg(long)]
    pub count: usize,

    #[arg(long, default_value = "managed")]
    pub r#type: ProcType,
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

    #[arg(long, default_value = "managed")]
    pub r#type: ProcType,
}

#[derive(Args, Debug, Clone)]
/// Restarts a node
pub struct Restart {
    #[arg(long)]
    pub node: usize,

    #[arg(long, default_value = "managed")]
    pub r#type: ProcType,
}

#[derive(Args, Debug, Clone)]
/// Append a random an event to a random stream to a node.
pub struct AppendToStream {
    /// Node index
    pub node: usize,

    #[arg(long)]
    pub stream: Option<String>,
}

#[derive(Args, Debug, Clone)]
/// Read a stream from a node.
pub struct ReadStream {
    /// Node index
    pub node: usize,

    #[arg(long)]
    pub stream: String,
}

#[derive(Args, Debug, Clone)]
pub struct Ping {
    #[command(subcommand)]
    pub command: PingCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum PingCommand {
    Node(PingNode),
    External(PingExternal),
}

#[derive(Args, Debug, Clone)]
pub struct PingNode {
    /// Node index
    pub node: usize,
}

#[derive(Args, Debug, Clone)]
pub struct PingExternal {
    #[arg(long)]
    pub host: String,

    #[arg(long)]
    pub port: u16,
}

#[derive(Args, Debug, Clone)]
pub struct StatusNode {
    /// Node index
    pub node: usize,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcType {
    Managed,
    Binary,
    External,
}

impl Default for ProcType {
    fn default() -> Self {
        ProcType::Managed
    }
}
