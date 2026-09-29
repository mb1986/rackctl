//! The rackctl command-line tool.
//!
//! Each subcommand runs once, prints its result and exits. Without a subcommand, rackctl
//! prints its help.

mod catalog;
mod check;
mod paths;
mod style;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anstream::{AutoStream, ColorChoice};
use clap::{CommandFactory, Parser, Subcommand};
use rackctl_core::catalog::Catalog;

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
    let result = match cli.command {
        Some(Command::Check) => check::run(cli.config, &locations),
        Some(Command::Catalog { command: CatalogCommand::Show(args) }) => {
            catalog::show(&args, cli.config, &locations)
        }
        None => Cli::command().print_help().map(|()| ExitCode::SUCCESS),
    };
    exit_code(result)
}

/// Returns a command's exit code. A closed pipe, as in `rackctl check | head -1`, is not an
/// error.
fn exit_code(result: io::Result<ExitCode>) -> ExitCode {
    result.unwrap_or_else(|error| {
        if error.kind() != io::ErrorKind::BrokenPipe {
            let _ = writeln!(io::stderr(), "rackctl: cannot write the output: {error}");
        }
        ExitCode::FAILURE
    })
}

/// Opens the built-in catalog with the user's models next to `rack_file`, if one is given.
/// Returns `None` after reporting a user catalog that cannot be read.
fn open_catalog(
    rack_file: Option<&Path>,
    locations: &Locations,
    err: &mut impl Write,
) -> io::Result<Option<Catalog>> {
    let mut catalog = Catalog::builtin();
    if let Some(rack_file) = rack_file {
        let user_dir = paths::user_catalog(rack_file);
        if let Err(error) = catalog.add_dirs(&[&user_dir]) {
            writeln!(err, "rackctl: cannot read {}: {error}", locations.display(&user_dir))?;
            return Ok(None);
        }
    }
    Ok(Some(catalog))
}

/// Makes error reports use colour exactly when the rest of the output does: not when
/// standard error is redirected, or when `NO_COLOR` is set.
fn set_report_style() {
    let color = AutoStream::choice(&io::stderr()) != ColorChoice::Never;
    let _ = miette::set_hook(Box::new(move |_| {
        Box::new(miette::MietteHandlerOpts::new().color(color).build())
    }));
}
