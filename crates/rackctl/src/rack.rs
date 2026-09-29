//! `rackctl rack`: draws the rack's front view.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use rackctl_tui::ui::rack::front_view;
use rackctl_tui::ui::text;

use crate::paths::Locations;
use crate::{CONFIG_ERROR, load_setup};

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
    let Some(setup) = load_setup(config, locations, &mut io::stderr())? else {
        return Ok(ExitCode::from(CONFIG_ERROR));
    };

    let buf = front_view(&setup.rack, &setup.catalog);
    let mut out = anstream::stdout();
    let lines = if args.plain { text::plain(&buf) } else { text::ansi(&buf) };
    for line in lines {
        writeln!(out, "{line}")?;
    }
    Ok(ExitCode::SUCCESS)
}
