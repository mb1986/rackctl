//! `rackctl catalog show`: draws a model's face in a slice of rack.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, ValueEnum};
use rackctl_core::catalog::{Catalog, ModelError};
use rackctl_tui::ui::art::{FaceLayout, FaceView, Panel, Sample, sample_looks};
use rackctl_tui::ui::slice::Slice;
use rackctl_tui::ui::text;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::CONFIG_ERROR;
use crate::paths::{self, Locations};

/// The width of a face between its ears.
const FACE_WIDTH: usize = 48;
/// The lowest unit of the drawn device.
const UNIT: u16 = 10;

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
    let looks = sample_looks(model, face, sample);
    let look = |index: usize| looks[index];
    let layout = FaceLayout::new(face, FACE_WIDTH);
    let view = FaceView {
        model,
        face,
        layout: &layout,
        name: &args.name,
        amps: "4.1A",
        look: &look,
        numbers: args.numbers,
    };
    let slice = Slice { panel: Panel { face: view }, unit: UNIT, rows_per_unit: 2 };
    let mut buf = Buffer::empty(Rect::new(0, 0, slice.width(), slice.height()));
    slice.render(buf.area, &mut buf);

    let mut out = anstream::stdout();
    let lines = if args.plain { text::plain(&buf) } else { text::ansi(&buf) };
    for line in lines {
        writeln!(out, "{line}")?;
    }
    Ok(ExitCode::SUCCESS)
}
