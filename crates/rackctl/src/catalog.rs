//! `rackctl catalog show`: draws a model's face in a slice of rack.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, ValueEnum};
use rackctl_core::catalog::{Catalog, ModelError};
use rackctl_tui::ui::art::Sample;
use rackctl_tui::ui::preview::preview;
use rackctl_tui::ui::text;

use crate::CONFIG_ERROR;
use crate::paths::{self, Locations};

#[derive(Debug, Args)]
pub struct ShowArgs {
    /// The model, such as `dell/r630-sff8`
    id: String,
    /// Show each element's number instead of its glyph
    #[arg(long)]
    numbers: bool,
    /// The sample status to draw
    #[arg(long, value_enum, default_value_t = SampleState::Normal)]
    state: SampleState,
    /// The device name for `name` fields
    #[arg(long, default_value = "srv01")]
    name: String,
    /// Draw without colours
    #[arg(long)]
    plain: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SampleState {
    Normal,
    Off,
}

/// Runs `catalog show`. `config` is the rack file named with `-c`, whose user catalog is used.
pub fn show(args: &ShowArgs, config: Option<PathBuf>, locations: &Locations) -> ExitCode {
    draw(args, config, locations).unwrap_or_else(|error| {
        if error.kind() != io::ErrorKind::BrokenPipe {
            let _ = writeln!(io::stderr(), "rackctl: cannot write the output: {error}");
        }
        ExitCode::FAILURE
    })
}

fn draw(args: &ShowArgs, config: Option<PathBuf>, locations: &Locations) -> io::Result<ExitCode> {
    let mut err = io::stderr();
    let mut catalog = Catalog::builtin();
    if let Some(rack_file) = config.or_else(|| locations.rack_file()) {
        let user_dir = paths::user_catalog(&rack_file);
        if let Err(error) = catalog.add_dirs(&[&user_dir]) {
            writeln!(err, "rackctl: cannot read {}: {error}", locations.display(&user_dir))?;
            return Ok(ExitCode::from(CONFIG_ERROR));
        }
    }
    let model = match catalog.model(&args.id) {
        Ok(model) => model,
        Err(ModelError::Unknown { id }) => {
            writeln!(err, "rackctl: unknown model `{id}`")?;
            return Ok(ExitCode::FAILURE);
        }
        Err(ModelError::Invalid(error)) => {
            for report in error.reports() {
                write!(err, "{report:?}")?;
            }
            return Ok(ExitCode::from(CONFIG_ERROR));
        }
    };
    let Some(face) = &model.faces.normal else {
        let reason = if model.faces.strip.is_some() {
            "has a strip face, which cannot be drawn yet"
        } else {
            "has no face"
        };
        writeln!(err, "rackctl: `{}` {reason}", args.id)?;
        return Ok(ExitCode::FAILURE);
    };

    let sample = match args.state {
        SampleState::Normal => Sample::Normal,
        SampleState::Off => Sample::Off,
    };
    let buf = preview(model, face, sample, args.numbers, &args.name);
    let mut out = anstream::stdout();
    let lines = if args.plain { text::plain(&buf) } else { text::ansi(&buf) };
    for line in lines {
        writeln!(out, "{line}")?;
    }
    Ok(ExitCode::SUCCESS)
}
