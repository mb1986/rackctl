//! Numbering: the order in which a face's numbered elements get their numbers.

use super::face::{Face, Faces};
use super::legend::{Direction, Legend, LegendEntry};
use crate::kdl_reader::{Problem, Spanned};

/// Where an element sits in the grid of its legend key: its row, and its column, which is
/// its place along the row.
#[derive(Debug, Clone, Copy)]
struct Position {
    /// The element's position in [`Face::elements`].
    index: usize,
    row: usize,
    column: usize,
}

/// Checks that each `layout=` matches every face that shows its key.
pub fn check_layouts(faces: &Faces, legend: &Legend, problems: &mut Vec<Problem>) {
    for face in faces.iter() {
        for entry in legend.entries().filter(|entry| entry.numbering.layout.is_some()) {
            ordered(face, entry, problems);
        }
    }
}

/// Returns the elements of `entry`'s key on `face`, as positions in [`Face::elements`], in
/// the order they are numbered. A `layout=` that does not match the face is reported, and
/// the grid of the picture is used instead.
pub fn ordered(face: &Face, entry: &LegendEntry, problems: &mut Vec<Problem>) -> Vec<usize> {
    let mut positions = picture_grid(face, entry.key);
    if let Some(layout) = &entry.numbering.layout
        && let Some(placed) = layout_grid(face, entry.key, layout, &positions, problems)
    {
        positions = placed;
    }
    let (primary, secondary) = (entry.numbering.order.primary, entry.numbering.order.secondary);
    let up = primary == Direction::Up || secondary == Some(Direction::Up);
    let left = primary == Direction::Left || secondary == Some(Direction::Left);
    let along = |a: usize, b: usize, reverse: bool| if reverse { b.cmp(&a) } else { a.cmp(&b) };
    positions.sort_by(|a, b| {
        let (rows, columns) = (along(a.row, b.row, up), along(a.column, b.column, left));
        if primary.is_horizontal() { rows.then(columns) } else { columns.then(rows) }
    });
    positions.into_iter().map(|position| position.index).collect()
}

/// Places the elements of `key` as the picture shows them: in their picture rows, where
/// the k-th element of a row is in column k, whatever lies between them.
fn picture_grid(face: &Face, key: char) -> Vec<Position> {
    let mut positions: Vec<Position> = Vec::new();
    // The elements are in reading order, so each row's elements come together.
    for (index, element) in face.elements().iter().enumerate() {
        if element.key != key {
            continue;
        }
        let column = match positions.last() {
            Some(last) if last.row == element.row => last.column + 1,
            _ => 0,
        };
        positions.push(Position { index, row: element.row, column });
    }
    positions
}

/// Places the elements of `key` on the grid of `layout`: the n-th row of the layout holds
/// the n-th picture row of these elements, and each `x` one of them, in order. Returns
/// `None` after reporting a layout that does not match the picture.
fn layout_grid(
    face: &Face,
    key: char,
    layout: &Spanned<Vec<Vec<bool>>>,
    picture: &[Position],
    problems: &mut Vec<Problem>,
) -> Option<Vec<Position>> {
    // A face without these elements has nothing for the layout to describe.
    if picture.is_empty() {
        return None;
    }
    let face_name = face.kind.name();
    let rows: Vec<&[Position]> = picture.chunk_by(|a, b| a.row == b.row).collect();
    if rows.len() != layout.value.len() {
        problems.push(Problem::new(
            format!(
                "`layout` has {}, but the {face_name} face has `{key}` elements on {}",
                plural(layout.value.len(), "row"),
                plural(rows.len(), "row"),
            ),
            layout.span,
        ));
        return None;
    }
    let mut placed = Vec::with_capacity(picture.len());
    for (row, (elements, slots)) in rows.iter().zip(&layout.value).enumerate() {
        let columns: Vec<usize> =
            slots.iter().enumerate().filter(|(_, slot)| **slot).map(|(column, _)| column).collect();
        if columns.len() != elements.len() {
            problems.push(Problem::new(
                format!(
                    "row {} of `layout` has {} `x`, but the {face_name} face has {} there",
                    row + 1,
                    columns.len(),
                    plural(elements.len(), &format!("`{key}` element")),
                ),
                layout.span,
            ));
            return None;
        }
        placed.extend(elements.iter().zip(columns).map(|(element, column)| Position {
            index: element.index,
            row,
            column,
        }));
    }
    Some(placed)
}

/// Writes a count with its noun, such as `1 row` or `3 rows`.
fn plural(count: usize, noun: &str) -> String {
    if count == 1 { format!("1 {noun}") } else { format!("{count} {noun}s") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Model;
    use crate::testing::pointed;

    /// The text of a 1U server whose normal face is `rows`, with `b` described by `entry`.
    fn server(rows: [&str; 2], entry: &str) -> String {
        let [top, bottom] = rows;
        format!(
            "model {{ name \"X\"; kind \"server\"\nface #\"\"\"\n{top}\n{bottom}\n\"\"\"#\n\
             legend {{ {entry}; c bay }} }}"
        )
    }

    /// Returns the row and column of each `b` element, in numbering order.
    fn order(rows: [&str; 2], entry: &str) -> Vec<(usize, usize)> {
        let model = Model::parse("x/y", &server(rows, entry)).expect("valid model");
        let face = model.faces.normal.expect("normal face");
        let entry = model.legend.get('b').expect("legend entry");
        let mut problems = Vec::new();
        let order = ordered(&face, entry, &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        order
            .iter()
            .map(|&index| (face.elements()[index].row, face.elements()[index].column))
            .collect()
    }

    #[test]
    fn numbers_along_rows_or_down_columns_from_any_corner() {
        let grid = ["b b bc", "b b bc"];
        let by = |direction: &str| order(grid, &format!("b bay order=\"{direction}\""));
        assert_eq!(by("right"), [(0, 0), (0, 2), (0, 4), (1, 0), (1, 2), (1, 4)]);
        assert_eq!(by("left"), [(0, 4), (0, 2), (0, 0), (1, 4), (1, 2), (1, 0)]);
        assert_eq!(by("right-up"), [(1, 0), (1, 2), (1, 4), (0, 0), (0, 2), (0, 4)]);
        assert_eq!(by("left-up"), [(1, 4), (1, 2), (1, 0), (0, 4), (0, 2), (0, 0)]);
        assert_eq!(by("down"), [(0, 0), (1, 0), (0, 2), (1, 2), (0, 4), (1, 4)]);
        assert_eq!(by("up"), [(1, 0), (0, 0), (1, 2), (0, 2), (1, 4), (0, 4)]);
        assert_eq!(by("down-left"), [(0, 4), (1, 4), (0, 2), (1, 2), (0, 0), (1, 0)]);
        assert_eq!(by("up-left"), [(1, 4), (0, 4), (1, 2), (0, 2), (1, 0), (0, 0)]);
        assert_eq!(order(grid, "b bay"), by("right"));
    }

    #[test]
    fn counts_columns_by_the_elements_of_the_key() {
        // The second row is shifted and has a `c` bay first, but its first `b` is still in
        // the first column.
        let order = order(["b b bc", " c b b b"], r#"b bay order="down""#);
        assert_eq!(order, [(0, 0), (1, 3), (0, 2), (1, 5), (0, 4), (1, 7)]);
    }

    #[test]
    fn numbers_on_the_grid_a_layout_gives() {
        // The R630's 3 + 5 bays: the top row starts in the third column.
        let rows = ["  b b bc", "b b b b b"];
        assert_eq!(
            order(rows, r#"b bay order="down" layout="--xxx,xxxxx""#),
            [(1, 0), (1, 2), (0, 2), (1, 4), (0, 4), (1, 6), (0, 6), (1, 8)]
        );
        assert_eq!(
            order(rows, r#"b bay order="down""#),
            [(0, 2), (1, 0), (0, 4), (1, 2), (0, 6), (1, 4), (1, 6), (1, 8)]
        );
    }

    #[test]
    fn reports_a_layout_that_does_not_match_the_picture() {
        let text = server(["b bc", "b b"], r#"b bay layout="xx""#);
        assert_eq!(
            pointed(&text, &Model::parse("x/y", &text).expect_err("wrong rows")),
            [(
                "`layout` has 1 row, but the normal face has `b` elements on 2 rows",
                r#"layout="xx""#
            )]
        );
        let text = server(["b bc", "b b"], r#"b bay layout="xx,x""#);
        assert_eq!(
            pointed(&text, &Model::parse("x/y", &text).expect_err("wrong elements")),
            [(
                "row 2 of `layout` has 1 `x`, but the normal face has 2 `b` elements there",
                r#"layout="xx,x""#
            )]
        );
    }
}
