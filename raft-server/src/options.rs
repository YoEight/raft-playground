use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Options {
    #[arg(long)]
    pub port: u16,

    #[arg(long = "seed")]
    pub seeds: Vec<u16>,
}
