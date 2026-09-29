//! The rack: its devices in their slots, with rails and unit numbers, and strips beside it.

use rackctl_core::catalog::{Catalog, Face, Model, STRIP_WIDTH};
use rackctl_core::rack::{self, Device, Placement, Rack, Side, UnitRange};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Widget;

use crate::ui::art::{FaceLayout, FaceView, Look, Panel, SAMPLE_AMPS, Sample, sample_looks};
use crate::ui::rowmap::{LabelRow, RowKind, RowMap};
use crate::ui::theme::{RACK, UNDERLINE};

/// The default width of a face between its ears.
pub const FACE_WIDTH: usize = 48;

/// Columns for a unit number and its space, on each side.
const LABEL: u16 = 3;

/// Draws the front view of `rack` at the default size, with the sample status.
#[must_use]
pub fn front_view(rack: &Rack, catalog: &Catalog) -> Buffer {
    let art = RackArt::new(rack, catalog, FACE_WIDTH, 2, LabelRow::default());
    let view = RackView { art: &art, numbers: false };
    let mut buf = Buffer::empty(Rect::new(0, 0, view.width(), view.height()));
    view.render(buf.area, &mut buf);
    buf
}

/// A device ready to draw.
pub(crate) struct DeviceArt<'a> {
    name: &'a str,
    model: &'a Model,
    face: &'a Face,
    layout: FaceLayout,
    looks: Vec<Look>,
    units: UnitRange,
}

impl<'a> DeviceArt<'a> {
    /// Prepares the device `name` covering `units`, its `face` laid out as `layout`.
    pub(crate) const fn new(
        name: &'a str,
        model: &'a Model,
        face: &'a Face,
        layout: FaceLayout,
        looks: Vec<Look>,
        units: UnitRange,
    ) -> Self {
        Self { name, model, face, layout, looks, units }
    }

    /// Prepares a device with the sample status.
    fn sample(
        device: &'a Device,
        model: &'a Model,
        face: &'a Face,
        layout: FaceLayout,
        units: UnitRange,
    ) -> Self {
        let looks = sample_looks(model, face, Sample::Normal);
        Self::new(&device.id, model, face, layout, looks, units)
    }

    fn panel(&self, numbers: bool) -> Panel<'_> {
        Panel {
            face: FaceView {
                model: self.model,
                face: self.face,
                layout: &self.layout,
                name: self.name,
                amps: SAMPLE_AMPS,
                looks: &self.looks,
                numbers,
            },
        }
    }
}

/// A rack's front view, ready to draw: its devices and strips, and its screen rows.
pub struct RackArt<'a> {
    width: usize,
    map: RowMap,
    devices: Vec<DeviceArt<'a>>,
    /// Strips on each side as seen from the front, nearest to the rack first.
    left: Vec<DeviceArt<'a>>,
    right: Vec<DeviceArt<'a>>,
}

impl<'a> RackArt<'a> {
    /// Prepares the front view of `rack` with faces `width` columns wide and the sample
    /// status. Devices without a valid model or a face for their place are left out.
    #[must_use]
    pub fn new(
        rack: &'a Rack,
        catalog: &'a Catalog,
        width: usize,
        rows_per_unit: u16,
        label: LabelRow,
    ) -> Self {
        let devices: Vec<DeviceArt> = rack
            .devices
            .iter()
            .filter(|device| {
                matches!(device.placement, Placement::Slot { .. })
                    && device.face == rack::Face::Front
            })
            .filter_map(|device| {
                let model = catalog.model(&device.model).ok()?;
                let face = model.faces.normal.as_ref()?;
                let units = device.units(model, rack.units);
                Some(DeviceArt::sample(device, model, face, FaceLayout::new(face, width), units))
            })
            .collect();
        let mut art =
            Self::from_devices(u16::from(rack.units), width, devices, rows_per_unit, label);

        for device in &rack.devices {
            let Placement::Strip { side, .. } = device.placement else { continue };
            let Ok(model) = catalog.model(&device.model) else { continue };
            let Some(face) = &model.faces.strip else { continue };
            let units = device.units(model, rack.units);
            // The frame takes the first and last row.
            let height = art.map.span(units).len().saturating_sub(2);
            let layout = FaceLayout::stretched(face, STRIP_WIDTH, height);
            let strip = DeviceArt::sample(device, model, face, layout, units);
            match seen_from_front(side, device.face) {
                Side::Left => art.left.push(strip),
                Side::Right => art.right.push(strip),
            }
        }
        art
    }

    /// Prepares a rack of `units` with `devices` in its slots and no strips.
    pub(crate) fn from_devices(
        units: u16,
        width: usize,
        devices: Vec<DeviceArt<'a>>,
        rows_per_unit: u16,
        label: LabelRow,
    ) -> Self {
        let slots: Vec<(usize, UnitRange)> =
            devices.iter().enumerate().map(|(index, device)| (index, device.units)).collect();
        let map = RowMap::new(units, &slots, rows_per_unit, label);
        Self { width, map, devices, left: Vec::new(), right: Vec::new() }
    }

    /// Returns the rack's screen rows.
    #[must_use]
    pub const fn map(&self) -> &RowMap {
        &self.map
    }
}

/// Returns the side a strip is on as seen from the front: a rear strip is mirrored.
const fn seen_from_front(side: Side, face: rack::Face) -> Side {
    match (side, face) {
        (side, rack::Face::Front) => side,
        (Side::Left, rack::Face::Rear) => Side::Right,
        (Side::Right, rack::Face::Rear) => Side::Left,
    }
}

/// The rack's rows with unit numbers and rails on each side, and its strips beside them.
#[derive(Clone, Copy)]
pub struct RackView<'a> {
    pub art: &'a RackArt<'a>,
    /// Whether numbered elements show their numbers instead of their glyphs.
    pub numbers: bool,
}

impl RackView<'_> {
    /// Returns the view's width: a device, its rails and unit numbers, and the strips.
    #[must_use]
    pub fn width(&self) -> u16 {
        self.rack_width() + strip_columns(self.art.left.len() + self.art.right.len())
    }

    /// Returns the view's height: one line per row.
    #[must_use]
    pub fn height(&self) -> u16 {
        u16::try_from(self.art.map.rows().len()).unwrap_or(u16::MAX)
    }

    /// Returns the width of the rack: a device, its rails and unit numbers.
    fn rack_width(self) -> u16 {
        self.panel_width() + 2 * (LABEL + 1)
    }

    /// Returns the width of a device between the rails, ears included.
    fn panel_width(self) -> u16 {
        u16::try_from(self.art.width + 2).unwrap_or(u16::MAX)
    }

    /// Draws a strip at column `x`, over the rows of its units.
    fn render_strip(self, strip: &DeviceArt<'_>, x: u16, area: Rect, buf: &mut Buffer) {
        let rows = self.art.map.span(strip.units);
        let panel = strip.panel(self.numbers);
        let y = area.y + u16::try_from(rows.start).unwrap_or(u16::MAX);
        panel.render(Rect { x, y, width: panel.width(), height: panel.height() }, buf);
    }
}

/// Returns the columns of `count` strips, each with the gap next to it.
fn strip_columns(count: usize) -> u16 {
    u16::try_from((STRIP_WIDTH + 3) * count).unwrap_or(u16::MAX)
}

impl Widget for RackView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(Rect { width: self.width(), height: self.height(), ..area });
        let rack_x = area.x + strip_columns(self.art.left.len());
        for (index, strip) in self.art.left.iter().enumerate() {
            self.render_strip(strip, rack_x - strip_columns(index + 1), area, buf);
        }
        let after = rack_x + self.rack_width() + 1;
        for (index, strip) in self.art.right.iter().enumerate() {
            self.render_strip(strip, after + strip_columns(index), area, buf);
        }

        let rack = Style::new().fg(RACK);
        let inside = Rect { x: rack_x + LABEL + 1, width: self.panel_width(), ..area };
        for (row, y) in self.art.map.rows().iter().zip(area.top()..area.bottom()) {
            let label = row.label.map_or_else(String::new, |unit| unit.to_string());
            buf.set_string(rack_x, y, format!("{label:>2} "), rack);
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
                    let panel = self.art.devices[index].panel(self.numbers);
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
        (
            "x/strip.kdl",
            indoc! {r##"
                model { name "Strip"; kind "pdu"; mount "side"; height 2
                  face strip=#true #"""
                    p
                    ~
                    """#
                  legend { p power; ~ fill } }"##},
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
        let art = RackArt::new(&rack, &catalog, 6, 2, LabelRow::Top);
        let view = RackView { art: &art, numbers: false };
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

    #[test]
    fn draws_strips_beside_the_rack() {
        let strips = r#"
            device "s1" model="x/strip" mount="left"
            device "s2" model="x/strip" mount="right" u=3 face="rear"
            device "s3" model="x/strip" mount="right""#;
        let rows = [
            "🭽▔▔▔▔▔🭾          4 ┊┓● a   ┏┊  4        ",
            "▏●    ▕            ┊┛      ┗┊           ",
            "▏     ▕          3 ┊·┊    ┊·┊  3        ",
            "🭼▁▁▁▁▁🭿            ┊·┊    ┊·┊           ",
            "        🭽▔▔▔▔▔🭾  2 ┊┓● b   ┏┊  2 🭽▔▔▔▔▔🭾",
            "        ▏●    ▕    ┊┃      ┃┊    ▏●    ▕",
            "        ▏     ▕  1 ┊┃      ┃┊  1 ▏     ▕",
            "        🭼▁▁▁▁▁🭿    ┊┛      ┗┊    🭼▁▁▁▁▁🭿",
        ];
        assert_eq!(plain(&draw(&format!("{DEVICES}{strips}"))), rows);
    }
}
