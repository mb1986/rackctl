//! The rackctl command-line tool.
//!
//! Each subcommand runs once, prints its result and exits. Without a subcommand, rackctl
//! prints its help.

mod catalog;
mod check;
mod paths;
mod rack;
mod style;
mod trace;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anstream::{AutoStream, ColorChoice};
use clap::{CommandFactory, Parser, Subcommand};
use rackctl_core::catalog::Catalog;
use rackctl_core::config::{self, Config};
use rackctl_core::kdl_reader::FileError;

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
        value_name = "PATH",
        help = "The configuration directory or file [default: $RACKCTL_CONFIG, or ~/.config/rackctl]"
    )]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check the configuration and the catalog, and summarize each rack
    Check,
    /// Draw the front view of each rack, or of one, with a sample status
    Rack(rack::RackArgs),
    /// Follow the cables from an endpoint, through patch panels
    Trace(trace::TraceArgs),
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
        Some(Command::Rack(args)) => rack::run(&args, cli.config, &locations),
        Some(Command::Trace(args)) => trace::run(&args, cli.config, &locations),
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

/// Returns the configuration's directory or file: `config`, or the default one. Returns
/// `None` after reporting that there is no default.
fn find_config(
    config: Option<PathBuf>,
    locations: &Locations,
    err: &mut impl Write,
) -> io::Result<Option<PathBuf>> {
    let path = config.or_else(|| locations.config_path());
    if path.is_none() {
        writeln!(
            err,
            "rackctl: cannot find the configuration because $HOME is not set; \
             use -c PATH or set $RACKCTL_CONFIG"
        )?;
    }
    Ok(path)
}

/// A valid configuration with the catalog its models come from.
struct Setup {
    catalog: Catalog,
    config: Config,
}

/// Loads the configuration named with `-c`, or the default one. Returns `None` after
/// reporting its problems.
fn load_setup(
    config: Option<PathBuf>,
    locations: &Locations,
    err: &mut impl Write,
) -> io::Result<Option<Setup>> {
    let Some(path) = find_config(config, locations, err)? else { return Ok(None) };
    let Some(catalog) = open_catalog(Some(&path), locations, err)? else {
        return Ok(None);
    };
    match config::load(&path, &catalog) {
        Ok(config) => Ok(Some(Setup { catalog, config })),
        Err(errors) => {
            write_reports(err, errors.iter().chain(catalog.invalid_models()))?;
            Ok(None)
        }
    }
}

/// Writes the problems of `files`, each report after a blank line.
fn write_reports<'a>(
    err: &mut impl Write,
    files: impl IntoIterator<Item = &'a FileError>,
) -> io::Result<()> {
    for report in files.into_iter().flat_map(FileError::reports) {
        write!(err, "\n{report:?}")?;
    }
    Ok(())
}

/// Opens the built-in catalog with the user's models of the configuration at `config`, if
/// one is given. Returns `None` after reporting a user catalog that cannot be read.
fn open_catalog(
    config: Option<&Path>,
    locations: &Locations,
    err: &mut impl Write,
) -> io::Result<Option<Catalog>> {
    let mut catalog = Catalog::builtin();
    if let Some(config) = config {
        let user_dir = paths::user_catalog(config);
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
