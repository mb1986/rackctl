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

/// Screen rows per rack unit; compact mode, with one, comes with compact faces.
const ROWS_PER_UNIT: u16 = 2;

/// Draws the front view of `rack` at the default size, with the sample status.
#[must_use]
pub fn front_view(rack: &Rack, catalog: &Catalog) -> Buffer {
    let art = RackArt::new(rack, catalog, FACE_WIDTH, LabelRow::default());
    RackView { art: &art, numbers: false, top: 0 }.canvas()
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
    pub fn new(rack: &'a Rack, catalog: &'a Catalog, width: usize, label: LabelRow) -> Self {
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
        let mut art = Self::from_devices(u16::from(rack.units), width, devices, label);

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
        label: LabelRow,
    ) -> Self {
        let slots: Vec<(usize, UnitRange)> =
            devices.iter().enumerate().map(|(index, device)| (index, device.units)).collect();
        let map = RowMap::new(units, &slots, ROWS_PER_UNIT, label);
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

/// The rack's rows with unit numbers and rails on each side, and its strips beside them,
/// shown from row `top` down.
#[derive(Clone, Copy)]
pub struct RackView<'a> {
    pub art: &'a RackArt<'a>,
    /// Whether numbered elements show their numbers instead of their glyphs.
    pub numbers: bool,
    /// The first row shown.
    pub top: u16,
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

    /// Draws the whole rack, whatever the size of the screen.
    #[must_use]
    pub fn canvas(self) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, self.width(), self.height()));
        self.draw(&mut buf);
        buf
    }

    /// Draws a strip at column `x`, over the rows of its units.
    fn draw_strip(self, strip: &DeviceArt<'_>, x: u16, buf: &mut Buffer) {
        let rows = self.art.map.span(strip.units);
        let y = u16::try_from(rows.start).unwrap_or(u16::MAX);
        let height = u16::try_from(rows.len()).unwrap_or(u16::MAX);
        let panel = strip.panel(self.numbers);
        panel.render(Rect { x, y, width: panel.width(), height }, buf);
    }

    /// Draws the whole rack into `buf`, a buffer of the view's size.
    fn draw(self, buf: &mut Buffer) {
        let area = buf.area;
        let rack_x = strip_columns(self.art.left.len());
        for (index, strip) in self.art.left.iter().enumerate() {
            self.draw_strip(strip, rack_x - strip_columns(index + 1), buf);
        }
        let after = rack_x + self.rack_width() + 1;
        for (index, strip) in self.art.right.iter().enumerate() {
            self.draw_strip(strip, after + strip_columns(index), buf);
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

/// Returns the columns of `count` strips, each with the gap next to it.
fn strip_columns(count: usize) -> u16 {
    u16::try_from((STRIP_WIDTH + 3) * count).unwrap_or(u16::MAX)
}

impl Widget for RackView<'_> {
    /// Shows the rows from `top` that fit in `area`.
    fn render(self, area: Rect, buf: &mut Buffer) {
        let canvas = self.canvas();
        let rows = canvas.area.height.saturating_sub(self.top).min(area.height);
        let columns = canvas.area.width.min(area.width);
        for y in 0..rows {
            for x in 0..columns {
                if let Some(cell) = buf.cell_mut((area.x + x, area.y + y)) {
                    *cell = canvas[(x, self.top + y)].clone();
                }
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

    /// Builds the view of a 4-unit rack of `devices`, with faces 6 columns wide, for `use_view`.
    fn with_view<T>(devices: &str, use_view: impl FnOnce(RackView) -> T) -> T {
        let dir = tempfile::tempdir().expect("temporary directory");
        fs::create_dir(dir.path().join("x")).expect("model directory");
        for (path, text) in MODELS {
            fs::write(dir.path().join(path), text).expect("model file");
        }
        let catalog = Catalog::open(&[dir.path()]).expect("readable catalog");
        let rack = Rack::parse(&format!("rack \"r\" units=4 {{\n{devices}\n}}")).expect("rack");
        let art = RackArt::new(&rack, &catalog, 6, LabelRow::Top);
        use_view(RackView { art: &art, numbers: false, top: 0 })
    }

    /// Draws the whole of a 4-unit rack of `devices`.
    #[expect(clippy::redundant_closure_for_method_calls, reason = "the method has one lifetime")]
    fn draw(devices: &str) -> Buffer {
        with_view(devices, |view| view.canvas())
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

    #[test]
    fn shows_the_rows_from_top_that_fit_the_area() {
        let devices = format!("{DEVICES}\ndevice \"s\" model=\"x/strip\" mount=\"left\" u=3");
        with_view(&devices, |view| {
            let canvas = view.canvas();
            for (top, area) in [
                (2, Rect::new(2, 1, 60, 4)),
                (0, Rect::new(0, 0, 10, 3)),
                (9, Rect::new(0, 0, 60, 4)),
            ] {
                let mut buf = Buffer::empty(Rect::new(0, 0, 50, 7));
                RackView { top, ..view }.render(area, &mut buf);
                for (x, y) in buf.area.positions().map(|position| (position.x, position.y)) {
                    let inside = area.contains((x, y).into())
                        && canvas.area.contains((x - area.x, y - area.y + top).into());
                    let expected = if inside {
                        canvas[(x - area.x, y - area.y + top)].clone()
                    } else {
                        ratatui::buffer::Cell::default()
                    };
                    assert_eq!(buf[(x, y)], expected, "top {top}, area {area:?}, cell ({x}, {y})");
                }
            }
        });
    }
}
