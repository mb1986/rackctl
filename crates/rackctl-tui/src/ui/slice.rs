//! A device shown in a slice of rack.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Widget;

use crate::ui::art::Panel;
use crate::ui::theme::{RACK, UNDERLINE};

/// Columns for a unit number and its space, on each side.
const LABEL: u16 = 3;

/// A device between an empty unit above and below it, with unit numbers and rails.
#[derive(Clone, Copy)]
pub struct Slice<'a> {
    pub panel: Panel<'a>,
    /// The device's lowest unit.
    pub unit: u16,
    /// Rows per unit: 2, or 1 in compact mode.
    pub rows_per_unit: u16,
}

impl Slice<'_> {
    /// Returns the slice's width: the panel, its rails and unit numbers.
    #[must_use]
    pub fn width(&self) -> u16 {
        self.panel.width() + 2 * (LABEL + 1)
    }

    /// Returns the slice's height: the panel and an empty unit on each side.
    #[must_use]
    pub fn height(&self) -> u16 {
        self.panel.height() + 2 * self.rows_per_unit
    }
}

impl Widget for Slice<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(Rect { width: self.width(), height: self.height(), ..area });
        let per_unit = self.rows_per_unit.max(1);
        let top = self.unit + self.panel.height() / per_unit;
        let rack = Style::new().fg(RACK);
        let inside = Rect { x: area.x + LABEL + 1, width: self.panel.width(), ..area };
        for (row, y) in (area.top()..area.bottom()).enumerate() {
            let row = u16::try_from(row).unwrap_or(u16::MAX);
            let unit = top - row / per_unit;
            let label = if row % per_unit == 0 { format!("{unit:>2}") } else { String::new() };
            buf.set_string(area.x, y, format!("{label:>2} "), rack);
            buf.set_string(inside.x - 1, y, "┊", rack);
            buf.set_string(inside.right(), y, "┊", rack);
            buf.set_string(inside.right() + 1, y, format!(" {label:>2}"), rack);
            if unit == top || unit + 1 == self.unit {
                let holes = format!("·┊{:1$}┊·", "", usize::from(inside.width.saturating_sub(4)));
                buf.set_string(inside.x, y, holes, rack);
            }
        }
        // The line above the device, on the last row of the unit above it.
        let above = Style::new().add_modifier(Modifier::UNDERLINED).underline_color(UNDERLINE);
        buf.set_style(Rect { y: area.y + per_unit - 1, height: 1, ..inside }, above);
        self.panel
            .render(Rect { y: area.y + per_unit, height: self.panel.height(), ..inside }, buf);
    }
}
