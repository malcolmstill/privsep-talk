mod cli;
mod controller;
mod engine;
mod msg;
mod parser;
mod proc;

use clap::Parser;
use cli::{Cli, Subsystem};
use std::io::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.subsystem {
        None => controller::controller().await,
        Some(Subsystem::Parser) => parser::parser().await,
        Some(Subsystem::Engine) => engine::engine().await,
    }
}
