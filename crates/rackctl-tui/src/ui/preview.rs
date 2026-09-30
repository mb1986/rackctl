//! A model's face with a sample status, in a slice of rack.

use rackctl_core::catalog::{Face, FaceKind, Model, STRIP_WIDTH};
use rackctl_core::rack::UnitRange;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Widget;

use crate::ui::art::{FaceLayout, Sample, sample_looks};
use crate::ui::rack::{DeviceArt, FACE_WIDTH, RackArt, RackView};
use crate::ui::rowmap::LabelRow;
use crate::ui::theme::RACK;

/// The lowest unit of the drawn device.
const UNIT: u16 = 10;
/// The units a strip spans when its model gives no height.
const STRIP_UNITS: u16 = 36;

/// Draws `face` of `model` with a sample status, as the device `name`: a rack device in a
/// slice of rack, or a strip over its span with unit numbers.
#[must_use]
pub fn preview(model: &Model, face: &Face, sample: Sample, numbers: bool, name: &str) -> Buffer {
    let looks = sample_looks(model, face, sample);
    if face.kind != FaceKind::Strip {
        let units = UnitRange::new(UNIT, UNIT + model.height.map_or(1, u16::from) - 1);
        let layout = FaceLayout::new(face, FACE_WIDTH);
        let device = DeviceArt::new(name, model, face, layout, looks, units);
        let top = units.highest() + 1;
        let art = RackArt::from_devices(top, FACE_WIDTH, vec![device], LabelRow::Top);
        let view = RackView { art: &art, numbers, top: 0 };
        // The device with an empty unit above and below it.
        let rows = art.map().span(UnitRange::new(UNIT - 1, top)).len();
        let height = u16::try_from(rows).unwrap_or(u16::MAX);
        let mut buf = Buffer::empty(Rect::new(0, 0, view.width(), height));
        view.render(buf.area, &mut buf);
        return buf;
    }
    let units = model.height.map_or(STRIP_UNITS, u16::from);
    let rows = units * 2;
    // The frame takes the first and last row.
    let layout = FaceLayout::stretched(face, STRIP_WIDTH, usize::from(rows - 2));
    let strip = DeviceArt::new(name, model, face, layout, looks, UnitRange::new(1, units));
    let width = u16::try_from(STRIP_WIDTH + 2).unwrap_or(u16::MAX);
    let mut buf = Buffer::empty(Rect::new(0, 0, width + 3, rows));
    for row in (0..rows).step_by(2) {
        buf.set_string(0, row, format!("{:>2} ", units - row / 2), Style::new().fg(RACK));
    }
    strip.render(numbers, true, Rect { x: 3, width, ..buf.area }, &mut buf);
    buf
}
