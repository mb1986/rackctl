//! The rack: its devices in their slots, with rails and unit numbers.

use rackctl_core::catalog::{Catalog, Face, Model};
use rackctl_core::rack::{self, Device, Placement, Rack, UnitRange};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Widget;

use crate::ui::art::{FaceLayout, FaceView, Look, Panel, SAMPLE_AMPS, Sample, sample_looks};
use crate::ui::rowmap::{LabelRow, RowKind, RowMap};
use crate::ui::theme::{RACK, UNDERLINE};

/// Columns for a unit number and its space, on each side.
const LABEL: u16 = 3;

/// A device ready to draw.
struct DeviceArt<'a> {
    device: &'a Device,
    model: &'a Model,
    face: &'a Face,
    layout: FaceLayout,
    looks: Vec<Look>,
    units: UnitRange,
}

/// The front devices of a rack, ready to draw.
pub struct RackArt<'a> {
    units: u16,
    width: usize,
    devices: Vec<DeviceArt<'a>>,
}

impl<'a> RackArt<'a> {
    /// Prepares the front devices of `rack` with faces `width` columns wide and the sample
    /// status. Devices without a valid model or a normal face are left out.
    #[must_use]
    pub fn new(rack: &'a Rack, catalog: &'a Catalog, width: usize) -> Self {
        let devices = rack
            .devices
            .iter()
            .filter(|device| {
                matches!(device.placement, Placement::Slot { .. })
                    && device.face == rack::Face::Front
            })
            .filter_map(|device| {
                let model = catalog.model(&device.model).ok()?;
                let face = model.faces.normal.as_ref()?;
                Some(DeviceArt {
                    device,
                    model,
                    face,
                    layout: FaceLayout::new(face, width),
                    looks: sample_looks(model, face, Sample::Normal),
                    units: device.units(model, rack.units),
                })
            })
            .collect();
        Self { units: u16::from(rack.units), width, devices }
    }

    /// Maps the rack to screen rows.
    #[must_use]
    pub fn row_map(&self, rows_per_unit: u16, label: LabelRow) -> RowMap {
        let devices: Vec<(usize, UnitRange)> =
            self.devices.iter().enumerate().map(|(index, device)| (index, device.units)).collect();
        RowMap::new(self.units, &devices, rows_per_unit, label)
    }
}

/// The rows of a row map, with unit numbers and rails on each side.
#[derive(Clone, Copy)]
pub struct RackView<'a> {
    pub art: &'a RackArt<'a>,
    pub map: &'a RowMap,
}

impl RackView<'_> {
    /// Returns the view's width: a device, its rails and unit numbers.
    #[must_use]
    pub fn width(&self) -> u16 {
        self.panel_width() + 2 * (LABEL + 1)
    }

    /// Returns the view's height: one line per row.
    #[must_use]
    pub fn height(&self) -> u16 {
        u16::try_from(self.map.rows().len()).unwrap_or(u16::MAX)
    }

    /// Returns the width of a device between the rails, ears included.
    fn panel_width(&self) -> u16 {
        u16::try_from(self.art.width + 2).unwrap_or(u16::MAX)
    }
}

impl Widget for RackView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(Rect { width: self.width(), height: self.height(), ..area });
        let rack = Style::new().fg(RACK);
        let inside = Rect { x: area.x + LABEL + 1, width: self.panel_width(), ..area };
        for (row, y) in self.map.rows().iter().zip(area.top()..area.bottom()) {
            let label = row.label.map_or_else(String::new, |unit| unit.to_string());
            buf.set_string(area.x, y, format!("{label:>2} "), rack);
            buf.set_string(inside.x - 1, y, "┊", rack);
            buf.set_string(inside.right(), y, "┊", rack);
            buf.set_string(inside.right() + 1, y, format!(" {label:>2}"), rack);
            match row.kind {
                RowKind::Empty { line_below } => {
                    let holes =
                        format!("·┊{:1$}┊·", "", usize::from(inside.width.saturating_sub(4)));
                    buf.set_string(inside.x, y, holes, rack);
                    if line_below {
                        let line = Style::new()
                            .add_modifier(Modifier::UNDERLINED)
                            .underline_color(UNDERLINE);
                        buf.set_style(Rect { y, height: 1, ..inside }, line);
                    }
                }
                RowKind::Device { index, row: 0 } => {
                    let device = &self.art.devices[index];
                    let panel = Panel {
                        face: FaceView {
                            model: device.model,
                            face: device.face,
                            layout: &device.layout,
                            name: &device.device.id,
                            amps: SAMPLE_AMPS,
                            looks: &device.looks,
                            numbers: false,
                        },
                    };
                    panel.render(Rect { y, height: panel.height(), ..inside }, buf);
                }
                RowKind::Device { .. } => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use indoc::indoc;

    use super::*;
    use crate::ui::text::plain;

    /// Models for the test racks, as file path and contents.
    const MODELS: &[(&str, &str)] = &[
        (
            "x/box.kdl",
            indoc! {r##"
                model { name "Box"; kind "server"
                  face #"""
                    p nnn

                    """#
                  legend { p power; n name } }"##},
        ),
        (
            "x/tall.kdl",
            indoc! {r##"
                model { name "Tall"; kind "server"; height 2
                  face #"""
                    p nnn



                    """#
                  legend { p power; n name } }"##},
        ),
    ];

    /// Draws a 4-unit rack of `devices`, with faces 6 columns wide.
    fn draw(devices: &str) -> Buffer {
        let dir = tempfile::tempdir().expect("temporary directory");
        fs::create_dir(dir.path().join("x")).expect("model directory");
        for (path, text) in MODELS {
            fs::write(dir.path().join(path), text).expect("model file");
        }
        let catalog = Catalog::open(&[dir.path()]).expect("readable catalog");
        let rack = Rack::parse(&format!("rack \"r\" units=4 {{\n{devices}\n}}")).expect("rack");
        let art = RackArt::new(&rack, &catalog, 6);
        let map = art.row_map(2, LabelRow::Top);
        let view = RackView { art: &art, map: &map };
        let mut buf = Buffer::empty(Rect::new(0, 0, view.width(), view.height()));
        view.render(buf.area, &mut buf);
        buf
    }

    const DEVICES: &str = r#"
        device "a" model="x/box" u=4
        device "b" model="x/tall" u=1
        device "c" model="x/box" u=3 face="rear""#;

    #[test]
    fn draws_devices_and_empty_units() {
        let rows = [
            " 4 ┊┓● a   ┏┊  4",
            "   ┊┛      ┗┊   ",
            " 3 ┊·┊    ┊·┊  3",
            "   ┊·┊    ┊·┊   ",
            " 2 ┊┓● b   ┏┊  2",
            "   ┊┃      ┃┊   ",
            " 1 ┊┃      ┃┊  1",
            "   ┊┛      ┗┊   ",
        ];
        assert_eq!(plain(&draw(DEVICES)), rows);
    }

    #[test]
    fn underlines_the_empty_row_above_a_device() {
        let buf = draw(DEVICES);
        let underlined = |x, y| buf[(x, y)].modifier.contains(Modifier::UNDERLINED);
        assert!((4..12).all(|x| underlined(x, 3)));
        assert!(!underlined(3, 3) && !underlined(12, 3));
        assert!(!underlined(4, 2));
    }
}
