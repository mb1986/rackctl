//! The layout of a face at a given width.

use std::iter;

use rackctl_core::catalog::{Cell, Face, Part};

/// A face laid out at a width, and for a strip, at a height.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceLayout {
    width: usize,
    /// For each picture row, the picture column each screen column shows; `None` is blank.
    columns: Vec<Vec<Option<usize>>>,
    /// The picture row each screen row shows; `None` is blank.
    rows: Vec<Option<usize>>,
}

impl FaceLayout {
    /// Lays out `face` at `width` columns, stretching its fills and cutting longer rows.
    #[must_use]
    pub fn new(face: &Face, width: usize) -> Self {
        let columns: Vec<Vec<Option<usize>>> = face
            .cells()
            .map(|cells| {
                let fills: Vec<bool> = cells.iter().map(|&cell| is_fill(face, cell)).collect();
                stretch(&fills, width)
            })
            .collect();
        let rows = (0..columns.len()).map(Some).collect();
        Self { width, columns, rows }
    }

    /// Lays out `face` at `width` columns and `height` rows. Rows holding only fills share the
    /// spare rows, the first ones taking the remainder, and a taller face is cut.
    #[must_use]
    pub fn stretched(face: &Face, width: usize, height: usize) -> Self {
        let blank = |cell: &Cell| *cell == Cell::Literal(' ');
        let fill_rows: Vec<bool> = face
            .cells()
            .map(|cells| {
                let start = cells.iter().position(|cell| !blank(cell)).unwrap_or(cells.len());
                let end = cells.iter().rposition(|cell| !blank(cell)).map_or(start, |end| end + 1);
                start < end && cells[start..end].iter().all(|&cell| is_fill(face, cell))
            })
            .collect();
        Self { rows: stretch(&fill_rows, height), ..Self::new(face, width) }
    }

    /// Returns the width the face is laid out at.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Returns the height the face is laid out at.
    #[must_use]
    pub const fn height(&self) -> usize {
        self.rows.len()
    }

    /// Returns, for each screen row, the picture row it shows with the picture column of each
    /// screen column, or `None` for a blank row.
    pub fn rows(&self) -> impl Iterator<Item = Option<(usize, &[Option<usize>])>> {
        self.rows.iter().map(|row| row.map(|row| (row, self.columns[row].as_slice())))
    }
}

/// Returns whether `cell` is part of a fill.
fn is_fill(face: &Face, cell: Cell) -> bool {
    matches!(cell, Cell::Element(index) if face.elements()[index].part == Part::Fill)
}

/// Lays out a line of items at `length` positions and returns the item each position shows,
/// `None` for a blank. Fills share the spare positions, the first ones taking the remainder,
/// and a longer line is cut.
fn stretch(fills: &[bool], length: usize) -> Vec<Option<usize>> {
    let count = fills.iter().filter(|&&fill| fill).count();
    let spare = length.saturating_sub(fills.len() - count);
    let mut line = Vec::with_capacity(length.max(fills.len()));
    let mut fill = 0;
    for (at, &is_fill) in fills.iter().enumerate() {
        if is_fill {
            let extra = usize::from(fill < spare % count);
            line.extend(iter::repeat_n(None, spare / count + extra));
            fill += 1;
        } else {
            line.push(Some(at));
        }
    }
    line.resize(length, None);
    line
}

#[cfg(test)]
mod tests {
    use indoc::formatdoc;
    use rackctl_core::catalog::Model;

    use super::*;

    /// Returns the face of a model with the picture `rows`: a 1U server's, or a strip's.
    fn face(rows: &[&str], strip: bool) -> Face {
        let picture = rows.join("\n");
        let used = |key| picture.contains(key);
        let legend = [('~', "~ fill"), ('.', ". space")]
            .into_iter()
            .filter_map(|(key, entry)| used(key).then_some(entry))
            .collect::<Vec<_>>()
            .join("; ");
        let (kind, face) =
            if strip { (r#"pdu"; mount "side"#, "face strip=#true") } else { ("server", "face") };
        let text = formatdoc! {r##"
            model {{ name "X"; kind "{kind}"
            {face} #"""
            {picture}
            """#
            legend {{ {legend} }} }}"##};
        let faces = Model::parse("x/y", &text).expect("valid model").faces;
        faces.strip.or(faces.normal).expect("a face")
    }

    /// Draws the picture character each screen column shows.
    fn draw(face: &Face, layout: &FaceLayout) -> Vec<String> {
        let pictures: Vec<Vec<char>> = face.rows().map(|row| row.chars().collect()).collect();
        let blank = " ".repeat(layout.width());
        layout
            .rows()
            .map(|row| {
                row.map_or_else(
                    || blank.clone(),
                    |(row, columns)| {
                        columns
                            .iter()
                            .map(|column| column.map_or(' ', |at| pictures[row][at]))
                            .collect()
                    },
                )
            })
            .collect()
    }

    fn laid_out(rows: [&str; 2], width: usize) -> Vec<String> {
        let face = face(&rows, false);
        draw(&face, &FaceLayout::new(&face, width))
    }

    fn stretched(rows: &[&str], height: usize) -> Vec<String> {
        let face = face(rows, true);
        draw(&face, &FaceLayout::stretched(&face, 1, height))
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

    #[test]
    fn shares_the_spare_rows_between_the_fill_rows() {
        assert_eq!(
            stretched(&["a", "~", "b", "~", "c"], 8),
            ["a", " ", " ", " ", "b", " ", " ", "c"]
        );
    }

    #[test]
    fn pads_a_strip_without_fill_rows_and_cuts_one_too_tall() {
        assert_eq!(stretched(&["a", "b", "c"], 4), ["a", "b", "c", " "]);
        assert_eq!(stretched(&["a", "b", "c"], 2), ["a", "b"]);
        // Fill rows get nothing before the other rows are cut.
        assert_eq!(stretched(&["p", "~", "o", "o", "o", "~", "t"], 4), ["p", "o", "o", "o"]);
    }
}
