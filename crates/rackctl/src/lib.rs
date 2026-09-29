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
use std::iter;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anstream::{AutoStream, ColorChoice};
use clap::{CommandFactory, Parser, Subcommand};
use rackctl_core::catalog::Catalog;
use rackctl_core::config;
use rackctl_core::kdl_reader::FileError;
use rackctl_core::rack::Rack;
use rackctl_core::wiring::Cabling;

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
    /// Draw the rack's front view with a sample status
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

/// Returns the rack file: `config`, or the default one. Returns `None` after reporting that
/// there is no default.
fn find_rack_file(
    config: Option<PathBuf>,
    locations: &Locations,
    err: &mut impl Write,
) -> io::Result<Option<PathBuf>> {
    let rack_file = config.or_else(|| locations.rack_file());
    if rack_file.is_none() {
        writeln!(
            err,
            "rackctl: cannot find the configuration because $HOME is not set; \
             use -c FILE or set $RACKCTL_CONFIG"
        )?;
    }
    Ok(rack_file)
}

/// A valid configuration: the catalog, the rack and its wiring.
struct Setup {
    catalog: Catalog,
    rack: Rack,
    wiring_file: PathBuf,
    /// The cables, or `None` without a wiring file.
    cabling: Option<Cabling>,
}

/// Loads the configuration named with `-c`, or the default one. Returns `None` after
/// reporting its problems.
fn load_setup(
    config: Option<PathBuf>,
    locations: &Locations,
    err: &mut impl Write,
) -> io::Result<Option<Setup>> {
    let Some(rack_file) = find_rack_file(config, locations, err)? else { return Ok(None) };
    let Some(catalog) = open_catalog(Some(&rack_file), locations, err)? else {
        return Ok(None);
    };
    let rack = match config::load_rack(&rack_file, &catalog) {
        Ok(rack) => rack,
        Err(error) => {
            write_reports(err, iter::once(&error).chain(catalog.invalid_models()))?;
            return Ok(None);
        }
    };
    let wiring_file = paths::wiring_file(&rack_file);
    let cabling = match config::load_wiring(&wiring_file, &rack, &catalog) {
        Ok(cabling) => cabling,
        Err(error) => {
            write_reports(err, iter::once(&error))?;
            return Ok(None);
        }
    };
    Ok(Some(Setup { catalog, rack, wiring_file, cabling }))
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
