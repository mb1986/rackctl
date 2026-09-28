//! The layout of a face at a given width.

use rackctl_core::catalog::{Cell, Face, Part};

/// A face laid out at a width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceLayout {
    width: usize,
    /// The picture column each screen column shows, row by row; `None` is blank.
    rows: Vec<Vec<Option<usize>>>,
}

impl FaceLayout {
    /// Lays out `face` at `width` columns, stretching its fills and cutting longer rows.
    #[must_use]
    pub fn new(face: &Face, width: usize) -> Self {
        let is_fill = |cell: &Cell| matches!(cell, Cell::Element(index) if face.elements()[*index].part == Part::Fill);
        let rows = face
            .cells()
            .map(|cells| {
                let fills = cells.iter().filter(|cell| is_fill(cell)).count();
                let spare = width.saturating_sub(cells.len() - fills);
                let mut columns = Vec::with_capacity(width.max(cells.len()));
                let mut fill = 0;
                for (column, cell) in cells.iter().enumerate() {
                    if is_fill(cell) {
                        // The leftmost fills take the remainder.
                        let extra = usize::from(fill < spare % fills);
                        columns.extend(std::iter::repeat_n(None, spare / fills + extra));
                        fill += 1;
                    } else {
                        columns.push(Some(column));
                    }
                }
                columns.resize(width, None);
                columns
            })
            .collect();
        Self { width, rows }
    }

    /// Returns the width the face is laid out at.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Returns the picture column each screen column shows, row by row; `None` is blank.
    pub fn rows(&self) -> impl Iterator<Item = &[Option<usize>]> {
        self.rows.iter().map(Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use indoc::formatdoc;
    use rackctl_core::catalog::Model;

    use super::*;

    /// Lays out a 1U face and draws the picture character each column shows.
    fn laid_out(rows: [&str; 2], width: usize) -> Vec<String> {
        let [top, bottom] = rows;
        let used = |key| top.contains(key) || bottom.contains(key);
        let legend = [('~', "~ fill"), ('.', ". space")]
            .into_iter()
            .filter_map(|(key, entry)| used(key).then_some(entry))
            .collect::<Vec<_>>()
            .join("; ");
        let text = formatdoc! {r##"
            model {{ name "X"; kind "server"
            face #"""
            {top}
            {bottom}
            """#
            legend {{ {legend} }} }}"##};
        let model = Model::parse("x/y", &text).expect("valid model");
        let face = model.faces.normal.expect("normal face");
        let layout = FaceLayout::new(&face, width);
        let pictures: Vec<Vec<char>> = face.rows().map(|row| row.chars().collect()).collect();
        layout
            .rows()
            .zip(&pictures)
            .map(|(columns, picture)| {
                columns.iter().map(|column| column.map_or(' ', |at| picture[at])).collect()
            })
            .collect()
    }

    #[test]
    fn shares_the_spare_columns_between_the_fills() {
        assert_eq!(laid_out(["x~y~z", "a.b~"], 10), ["x    y   z", "a.b       "]);
    }

    #[test]
    fn pads_a_row_without_fills_and_cuts_one_too_wide() {
        assert_eq!(laid_out(["abc", "abcdefgh"], 5), ["abc  ", "abcde"]);
    }

    #[test]
    fn gives_fills_nothing_when_the_row_is_full() {
        assert_eq!(laid_out(["ab~cd", "~"], 4), ["abcd", "    "]);
    }
}
