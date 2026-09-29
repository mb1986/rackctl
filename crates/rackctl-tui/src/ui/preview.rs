//! A model's face with a sample status, in a slice of rack.

use rackctl_core::catalog::{Face, Model};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::ui::art::{FaceLayout, FaceView, Panel, Sample, sample_looks};
use crate::ui::slice::Slice;

/// The width of a face between its ears.
const FACE_WIDTH: usize = 48;
/// The lowest unit of the drawn device.
const UNIT: u16 = 10;

/// Draws `face` of `model` with a sample status, as the device `name`, in a slice of rack.
#[must_use]
pub fn preview(model: &Model, face: &Face, sample: Sample, numbers: bool, name: &str) -> Buffer {
    let looks = sample_looks(model, face, sample);
    let look = |index: usize| looks[index];
    let layout = FaceLayout::new(face, FACE_WIDTH);
    let view = FaceView { model, face, layout: &layout, name, amps: "4.1A", look: &look, numbers };
    let slice = Slice { panel: Panel { face: view }, unit: UNIT, rows_per_unit: 2 };
    let mut buf = Buffer::empty(Rect::new(0, 0, slice.width(), slice.height()));
    slice.render(buf.area, &mut buf);
    buf
}
