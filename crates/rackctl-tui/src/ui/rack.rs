//! The rack: its devices in their slots, with rails and unit numbers, and strips beside it.

use rackctl_core::catalog::{Catalog, Face, Kind, Model, STRIP_WIDTH};
use rackctl_core::rack::{self, Device, Placement, Rack, Side, UnitRange};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Widget;

use crate::ui::art::{
    BlankPanel, FaceLayout, FaceView, Look, Panel, SAMPLE_AMPS, Sample, sample_looks,
};
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
    /// `None` for a model without a face for the device's place.
    face: Option<FaceArt<'a>>,
    units: UnitRange,
}

/// A face laid out, with the look of each of its elements.
struct FaceArt<'a> {
    face: &'a Face,
    layout: FaceLayout,
    looks: Vec<Look>,
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
        Self { name, model, face: Some(FaceArt { face, layout, looks }), units }
    }

    /// Prepares a device with the sample status, or without a face when there is none.
    fn sample(
        device: &'a Device,
        model: &'a Model,
        face: Option<(&'a Face, FaceLayout)>,
        units: UnitRange,
    ) -> Self {
        let face = face.map(|(face, layout)| FaceArt {
            face,
            layout,
            looks: sample_looks(model, face, Sample::Normal),
        });
        Self { name: &device.id, model, face, units }
    }

    /// Draws the device over `area`: its face in a frame, or the frame alone.
    pub(crate) fn render(&self, numbers: bool, strip: bool, area: Rect, buf: &mut Buffer) {
        let Some(face) = &self.face else {
            let named = !matches!(self.model.kind, Kind::Blank | Kind::Shelf);
            let name = named.then_some(self.name);
            BlankPanel { ears: self.model.ears, strip, name }.render(area, buf);
            return;
        };
        let face = FaceView {
            model: self.model,
            face: face.face,
            layout: &face.layout,
            name: self.name,
            amps: SAMPLE_AMPS,
            looks: &face.looks,
            numbers,
        };
        Panel { face }.render(area, buf);
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
    /// status. Devices without a valid model are left out, and those without a face for their
    /// place are drawn as an empty frame.
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
                let face =
                    model.faces.normal.as_ref().map(|face| (face, FaceLayout::new(face, width)));
                let units = device.units(model, rack.units);
                Some(DeviceArt::sample(device, model, face, units))
            })
            .collect();
        let mut art = Self::from_devices(u16::from(rack.units), width, devices, label);

        for device in &rack.devices {
            let Placement::Strip { side, .. } = device.placement else { continue };
            let Ok(model) = catalog.model(&device.model) else { continue };
            let units = device.units(model, rack.units);
            // The frame takes the first and last row.
            let height = art.map.span(units).len().saturating_sub(2);
            let face = model.faces.strip.as_ref();
            let face = face.map(|face| (face, FaceLayout::stretched(face, STRIP_WIDTH, height)));
            let strip = DeviceArt::sample(device, model, face, units);
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
        let width = u16::try_from(STRIP_WIDTH + 2).unwrap_or(u16::MAX);
        strip.render(self.numbers, true, Rect { x, y, width, height }, buf);
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
                    let device = &self.art.devices[index];
                    let rows = self.art.map.span(device.units).len();
                    let height = u16::try_from(rows).unwrap_or(u16::MAX);
                    device.render(self.numbers, false, Rect { y, height, ..inside }, buf);
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
    use crate::ui::theme::PANEL;

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
        ("x/plain.kdl", r#"model { name "Plain"; kind "server" }"#),
        ("x/blank.kdl", r#"model { name "Blank"; kind "blank"; ears "screws" }"#),
        ("x/bare.kdl", r#"model { name "Bare"; kind "pdu"; mount "side"; height 2 }"#),
    ];

    /// Builds the view of a 4-unit rack of `devices`, with faces 6 columns wide, for `use_view`.
    fn with_view<T>(devices: &str, label: LabelRow, use_view: impl FnOnce(RackView) -> T) -> T {
        let dir = tempfile::tempdir().expect("temporary directory");
        fs::create_dir(dir.path().join("x")).expect("model directory");
        for (path, text) in MODELS {
            fs::write(dir.path().join(path), text).expect("model file");
        }
        let catalog = Catalog::open(&[dir.path()]).expect("readable catalog");
        let rack = Rack::parse(&format!("rack \"r\" units=4 {{\n{devices}\n}}")).expect("rack");
        let art = RackArt::new(&rack, &catalog, 6, label);
        use_view(RackView { art: &art, numbers: false, top: 0 })
    }

    /// Draws the whole of a 4-unit rack of `devices`.
    fn draw(devices: &str) -> Buffer {
        draw_labelled(devices, LabelRow::Top)
    }

    /// Draws the whole of a 4-unit rack of `devices`, with unit numbers on `label` rows.
    #[expect(clippy::redundant_closure_for_method_calls, reason = "the method has one lifetime")]
    fn draw_labelled(devices: &str, label: LabelRow) -> Buffer {
        with_view(devices, label, |view| view.canvas())
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
    fn puts_unit_numbers_on_the_bottom_row() {
        let rows = [
            "   ┊┓● a   ┏┊   ",
            " 4 ┊┛      ┗┊  4",
            "   ┊·┊    ┊·┊   ",
            " 3 ┊·┊    ┊·┊  3",
            "   ┊┓● b   ┏┊   ",
            " 2 ┊┃      ┃┊  2",
            "   ┊┃      ┃┊   ",
            " 1 ┊┛      ┗┊  1",
        ];
        assert_eq!(plain(&draw_labelled(DEVICES, LabelRow::Bottom)), rows);
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
        with_view(&devices, LabelRow::Top, |view| {
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

    #[test]
    fn draws_devices_without_a_face_as_empty_frames() {
        let devices = r#"
            device "srv" model="x/plain" u=4
            device "cover" model="x/blank" u=2
            device "pdu" model="x/bare" mount="left" u=3"#;
        let rows = [
            "🭽▔▔▔▔▔🭾  4 ┊┓ srv  ┏┊  4",
            "▏pdu  ▕    ┊┛      ┗┊   ",
            "▏     ▕  3 ┊·┊    ┊·┊  3",
            "🭼▁▁▁▁▁🭿    ┊·┊    ┊·┊   ",
            "         2 ┊⊕      ⊕┊  2",
            "           ┊⊕      ⊕┊   ",
            "         1 ┊·┊    ┊·┊  1",
            "           ┊·┊    ┊·┊   ",
        ];
        let buf = draw(devices);
        assert_eq!(plain(&buf), rows);
        // The panels are tinted like any device, so they don't read as free space.
        assert!([(16, 0), (16, 5), (3, 2)].iter().all(|&at| buf[at].bg == PANEL));
    }
}
