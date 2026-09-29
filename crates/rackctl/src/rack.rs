//! `rackctl rack`: draws the rack's front view.

use std::io::{self, Write};
use std::iter;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use rackctl_core::config;
use rackctl_tui::ui::rack::front_view;
use rackctl_tui::ui::text;

use crate::paths::{self, Locations};
use crate::{CONFIG_ERROR, find_rack_file, open_catalog, write_reports};

#[derive(Debug, Args)]
pub struct RackArgs {
    /// Draw without colours
    #[arg(long)]
    plain: bool,
}

/// Runs `rack`. `config` is the rack file named with `-c`, if any.
pub fn run(
    args: &RackArgs,
    config: Option<PathBuf>,
    locations: &Locations,
) -> io::Result<ExitCode> {
    let mut err = io::stderr();
    let Some(rack_file) = find_rack_file(config, locations, &mut err)? else {
        return Ok(ExitCode::from(CONFIG_ERROR));
    };
    let Some(catalog) = open_catalog(Some(&rack_file), locations, &mut err)? else {
        return Ok(ExitCode::from(CONFIG_ERROR));
    };
    let rack = match config::load_rack(&rack_file, &catalog) {
        Ok(rack) => rack,
        Err(error) => {
            write_reports(&mut err, iter::once(&error).chain(catalog.invalid_models()))?;
            return Ok(ExitCode::from(CONFIG_ERROR));
        }
    };
    if let Err(error) = config::load_wiring(&paths::wiring_file(&rack_file), &rack, &catalog) {
        write_reports(&mut err, iter::once(&error))?;
        return Ok(ExitCode::from(CONFIG_ERROR));
    }

    let buf = front_view(&rack, &catalog);
    let mut out = anstream::stdout();
    let lines = if args.plain { text::plain(&buf) } else { text::ansi(&buf) };
    for line in lines {
        writeln!(out, "{line}")?;
    }
    Ok(ExitCode::SUCCESS)
}
