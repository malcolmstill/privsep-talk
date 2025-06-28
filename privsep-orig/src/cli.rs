use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    version,
    about,
    long_about = None, 
    subcommand_value_name = "SUBSYSTEM",
    subcommand_help_heading = "Subsystems",
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Subsystem
    #[command(subcommand)]
    pub subsystem: Option<Subsystem>,
}

#[derive(Subcommand)]
pub enum Subsystem {
    /// Parser subsystem
    Parser,
    /// Engine subsystem
    Engine,
}
