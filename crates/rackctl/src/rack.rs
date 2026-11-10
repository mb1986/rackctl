//! `rackctl rack`: draws the front view of each rack, or of one.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anstyle::Style;
use clap::Args;
use rackctl_core::rack::Rack;
use rackctl_tui::ui::rack::front_view;
use rackctl_tui::ui::text;

use crate::paths::Locations;
use crate::style::HEADING;
use crate::{CONFIG_ERROR, load_setup};

#[derive(Debug, Args)]
pub struct RackArgs {
    /// The rack to draw [default: every rack]
    name: Option<String>,
    /// Draw without colours
    #[arg(long)]
    plain: bool,
}

/// Runs `rack`. `config` is the configuration named with `-c`, if any.
pub fn run(
    args: &RackArgs,
    config: Option<PathBuf>,
    locations: &Locations,
) -> io::Result<ExitCode> {
    let mut err = io::stderr();
    let Some(setup) = load_setup(config, locations, &mut err)? else {
        return Ok(ExitCode::from(CONFIG_ERROR));
    };
    let layout = &setup.config.layout;
    let racks: Vec<&Rack> = match &args.name {
        None => layout.racks().iter().collect(),
        Some(name) => {
            let Some(rack) = layout.rack(name) else {
                let names: Vec<_> = layout.racks().iter().map(|rack| rack.name.as_str()).collect();
                writeln!(
                    err,
                    "rackctl: unknown rack `{name}`; the racks are {}",
                    names.join(", ")
                )?;
                return Ok(ExitCode::FAILURE);
            };
            vec![rack]
        }
    };

    let mut out = anstream::stdout();
    let heading = if args.plain { Style::new() } else { HEADING };
    for (index, rack) in racks.iter().enumerate() {
        // A rack drawn alone needs no heading.
        if racks.len() > 1 {
            if index > 0 {
                writeln!(out)?;
            }
            writeln!(out, "{heading}rack \"{}\", {}U{heading:#}", rack.name, rack.units)?;
        }
        let buf = front_view(rack, &setup.catalog);
        let lines = if args.plain { text::plain(&buf) } else { text::ansi(&buf) };
        for line in lines {
            writeln!(out, "{line}")?;
        }
    }
    Ok(ExitCode::SUCCESS)
}
