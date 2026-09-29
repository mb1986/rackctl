//! A model's face with a sample status, in a slice of rack.

use rackctl_core::catalog::{Face, FaceKind, Model, STRIP_WIDTH};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Widget;

use crate::ui::art::{FaceLayout, FaceView, Panel, SAMPLE_AMPS, Sample, sample_looks};
use crate::ui::slice::Slice;
use crate::ui::theme::RACK;

/// The width of a face between its ears.
const FACE_WIDTH: usize = 48;
/// The lowest unit of the drawn device.
const UNIT: u16 = 10;
/// The units a strip spans when its model gives no height.
const STRIP_UNITS: u16 = 36;

/// Draws `face` of `model` with a sample status, as the device `name`: a rack device in a
/// slice of rack, or a strip over its span with unit numbers.
#[must_use]
pub fn preview(model: &Model, face: &Face, sample: Sample, numbers: bool, name: &str) -> Buffer {
    let looks = sample_looks(model, face, sample);
    let view =
        |layout| FaceView { model, face, layout, name, amps: SAMPLE_AMPS, looks: &looks, numbers };
    if face.kind != FaceKind::Strip {
        let layout = FaceLayout::new(face, FACE_WIDTH);
        let slice = Slice { panel: Panel { face: view(&layout) }, unit: UNIT, rows_per_unit: 2 };
        let mut buf = Buffer::empty(Rect::new(0, 0, slice.width(), slice.height()));
        slice.render(buf.area, &mut buf);
        return buf;
    }
    let units = model.height.map_or(STRIP_UNITS, u16::from);
    // The frame takes the first and last row.
    let layout = FaceLayout::stretched(face, STRIP_WIDTH, usize::from(units * 2 - 2));
    let panel = Panel { face: view(&layout) };
    let mut buf = Buffer::empty(Rect::new(0, 0, panel.width() + 3, panel.height()));
    for row in (0..panel.height()).step_by(2) {
        buf.set_string(0, row, format!("{:>2} ", units - row / 2), Style::new().fg(RACK));
    }
    panel.render(Rect { x: 3, ..buf.area }, &mut buf);
    buf
}
