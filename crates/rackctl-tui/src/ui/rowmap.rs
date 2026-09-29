//! The rack as screen rows.

use std::ops::Range;

use rackctl_core::rack::UnitRange;

/// Which of a unit's rows shows its number.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LabelRow {
    #[default]
    Top,
    Bottom,
}

/// One screen row of the rack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    /// The unit number shown on this row.
    pub label: Option<u16>,
    pub kind: RowKind,
}

/// What a screen row shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// A device's row, counted from its top.
    Device { index: usize, row: u16 },
    /// An empty unit's row; `line_below` when a device starts right below it.
    Empty { line_below: bool },
}

/// The rack's screen rows, from the top of the rack down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowMap {
    rows: Vec<Row>,
    units: u16,
    rows_per_unit: u16,
}

impl RowMap {
    /// Maps a rack of `units` with `devices` in its slots, given by the caller's index and
    /// the units they cover.
    #[must_use]
    pub fn new(
        units: u16,
        devices: &[(usize, UnitRange)],
        rows_per_unit: u16,
        label: LabelRow,
    ) -> Self {
        let per_unit = rows_per_unit.max(1);
        // The device in each unit, with the device's top unit.
        let mut owners = vec![None; usize::from(units) + 1];
        for &(index, range) in devices {
            for u in range.lowest()..=range.highest().min(units) {
                owners[usize::from(u)] = Some((index, range.highest()));
            }
        }
        let mut rows = Vec::with_capacity(usize::from(units) * usize::from(per_unit));
        for u in (1..=units).rev() {
            for r in 0..per_unit {
                let last = r + 1 == per_unit;
                let labelled = match label {
                    LabelRow::Top => r == 0,
                    LabelRow::Bottom => last,
                };
                let kind = match owners[usize::from(u)] {
                    Some((index, top)) => RowKind::Device { index, row: (top - u) * per_unit + r },
                    None => {
                        RowKind::Empty { line_below: last && owners[usize::from(u - 1)].is_some() }
                    }
                };
                rows.push(Row { label: labelled.then_some(u), kind });
            }
        }
        Self { rows, units, rows_per_unit: per_unit }
    }

    /// Returns the rows from the top of the rack down.
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Returns the rows that `units` cover, clipped to the rack.
    #[must_use]
    pub fn span(&self, units: UnitRange) -> Range<usize> {
        if units.lowest() > self.units {
            return 0..0;
        }
        let per_unit = usize::from(self.rows_per_unit);
        let top = usize::from(self.units - units.highest().min(self.units));
        let bottom = usize::from(self.units - units.lowest().max(1)) + 1;
        top * per_unit..bottom * per_unit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes each row as its label and `index:row` for a device, `.` for an empty row or
    /// `_` for one with a line below.
    fn sketch(map: &RowMap) -> Vec<String> {
        map.rows()
            .iter()
            .map(|row| {
                let label = row.label.map_or_else(String::new, |u| u.to_string());
                let kind = match row.kind {
                    RowKind::Device { index, row } => format!("{index}:{row}"),
                    RowKind::Empty { line_below: false } => ".".to_owned(),
                    RowKind::Empty { line_below: true } => "_".to_owned(),
                };
                format!("{label:>1} {kind}")
            })
            .collect()
    }

    /// A 4-unit rack: a 1U device at the top, an empty unit, and a 2U device at the bottom.
    fn rack(rows_per_unit: u16, label: LabelRow) -> RowMap {
        let devices = [(0, UnitRange::new(4, 4)), (1, UnitRange::new(1, 2))];
        RowMap::new(4, &devices, rows_per_unit, label)
    }

    #[test]
    fn maps_devices_and_empty_units_from_the_top() {
        let rows = ["4 0:0", "  0:1", "3 .", "  _", "2 1:0", "  1:1", "1 1:2", "  1:3"];
        assert_eq!(sketch(&rack(2, LabelRow::Top)), rows);
    }

    #[test]
    fn puts_labels_on_the_bottom_row() {
        let rows = ["  0:0", "4 0:1", "  .", "3 _", "  1:0", "2 1:1", "  1:2", "1 1:3"];
        assert_eq!(sketch(&rack(2, LabelRow::Bottom)), rows);
    }

    #[test]
    fn maps_one_row_per_unit() {
        assert_eq!(sketch(&rack(1, LabelRow::Bottom)), ["4 0:0", "3 _", "2 1:0", "1 1:1"]);
    }

    #[test]
    fn draws_no_line_between_stacked_devices() {
        let devices = [(0, UnitRange::new(2, 2)), (1, UnitRange::new(1, 1))];
        let map = RowMap::new(3, &devices, 2, LabelRow::Top);
        assert_eq!(sketch(&map), ["3 .", "  _", "2 0:0", "  0:1", "1 1:0", "  1:1"]);
    }

    #[test]
    fn finds_the_rows_of_units() {
        let map = rack(2, LabelRow::Top);
        assert_eq!(map.span(UnitRange::new(1, 4)), 0..8);
        assert_eq!(map.span(UnitRange::new(2, 3)), 2..6);
        assert_eq!(map.span(UnitRange::new(3, 9)), 0..4);
        assert_eq!(map.span(UnitRange::new(5, 6)), 0..0);
    }
}
