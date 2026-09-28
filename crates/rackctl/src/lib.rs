//! The rackctl command-line tool.
//!
//! Each subcommand runs once, prints its result and exits. Without a subcommand, rackctl
//! prints its help.

mod catalog;
mod check;
mod paths;
mod style;

use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use anstream::{AutoStream, ColorChoice};
use clap::{CommandFactory, Parser, Subcommand};

use crate::paths::Locations;

/// Exit code for a configuration that cannot be found, read or accepted.
const CONFIG_ERROR: u8 = 2;

#[derive(Debug, Parser)]
#[command(name = "rackctl", version, about)]
struct Cli {
    #[arg(
        short,
        long,
        global = true,
        value_name = "FILE",
        help = "The rack file [default: $RACKCTL_CONFIG, or ~/.config/rackctl/rack.kdl]"
    )]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check the configuration and the catalog, and summarize the rack
    Check,
    /// Work with the device catalog
    Catalog {
        #[command(subcommand)]
        command: CatalogCommand,
    },
}

#[derive(Debug, Subcommand)]
enum CatalogCommand {
    /// Draw a model's face with a sample status
    Show(catalog::ShowArgs),
}

/// Runs rackctl with the arguments of the process and returns its exit code.
#[must_use]
pub fn run() -> ExitCode {
    let cli = Cli::parse();
    set_report_style();
    let locations = Locations::from_env();
    match cli.command {
        Some(Command::Check) => check::run(cli.config, &locations),
        Some(Command::Catalog { command: CatalogCommand::Show(args) }) => {
            catalog::show(&args, cli.config, &locations)
        }
        None => {
            let _ = Cli::command().print_help();
            ExitCode::SUCCESS
        }
    }
}

/// Makes error reports use colour exactly when the rest of the output does: not when
/// standard error is redirected, or when `NO_COLOR` is set.
fn set_report_style() {
    let color = AutoStream::choice(&io::stderr()) != ColorChoice::Never;
    let _ = miette::set_hook(Box::new(move |_| {
        Box::new(miette::MietteHandlerOpts::new().color(color).build())
    }));
}
