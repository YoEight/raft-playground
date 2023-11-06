use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Commands {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    Exit,
    Quit,
    Spawn(Spawn),
}

#[derive(Args, Debug, Clone)]
pub struct Spawn {
    #[arg(long)]
    pub count: usize,
}
