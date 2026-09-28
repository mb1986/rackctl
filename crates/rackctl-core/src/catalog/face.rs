//! Faces: the pictures a model's front panel is drawn from.

use kdl::{KdlEntry, KdlNode};
use miette::SourceSpan;
use unicode_width::UnicodeWidthChar;

use super::legend::{Legend, Part, PartKind};
use super::model::Mount;
use crate::kdl_reader::{NodeReader, Problem};

/// The widest a strip face may be, in columns.
pub const STRIP_WIDTH: usize = 5;

/// The faces of a model, at most one of each kind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Faces {
    /// The face drawn with two rows per unit.
    pub normal: Option<Face>,
    /// The face drawn with one row per unit.
    pub compact: Option<Face>,
    /// The face of a vertical strip beside the rack.
    pub strip: Option<Face>,
}

impl Faces {
    /// Returns whether the model has any face.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.normal.is_none() && self.compact.is_none() && self.strip.is_none()
    }

    /// Returns the faces the model has.
    pub fn iter(&self) -> impl Iterator<Item = &Face> {
        [&self.normal, &self.compact, &self.strip].into_iter().flatten()
    }

    fn iter_mut(&mut self) -> impl Iterator<Item = &mut Face> {
        [&mut self.normal, &mut self.compact, &mut self.strip].into_iter().flatten()
    }

    const fn slot(&mut self, kind: FaceKind) -> &mut Option<Face> {
        match kind {
            FaceKind::Normal => &mut self.normal,
            FaceKind::Compact => &mut self.compact,
            FaceKind::Strip => &mut self.strip,
        }
    }
}

/// The kinds of faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FaceKind {
    /// Two rows per unit.
    Normal,
    /// One row per unit, written `face compact=#true`.
    Compact,
    /// A vertical strip beside the rack, written `face strip=#true`.
    Strip,
}

impl FaceKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Compact => "compact",
            Self::Strip => "strip",
        }
    }
}

/// One face of a model: its picture, cut into cells and elements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Face {
    /// Which face this is.
    pub kind: FaceKind,
    /// Location of the picture in the model file.
    pub span: SourceSpan,
    rows: Vec<String>,
    /// Where each row starts in the model file, when it is written there as it is.
    row_offsets: Vec<Option<usize>>,
    /// The cells of each row, from the top. Rows may differ in length.
    cells: Vec<Vec<Cell>>,
    elements: Vec<Element>,
}

/// One cell of a face: a character drawn as it is, or part of an element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cell {
    /// A character drawn as it is.
    Literal(char),
    /// Part of the element at this position in [`Face::elements`].
    Element(usize),
}

/// A live part of a face, drawn from a legend key: an LED, a numbered element such as a bay
/// or a port, a text field, or blank space. It covers a rectangle of cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    /// The legend key the element is drawn with.
    pub key: char,
    /// What the element shows.
    pub part: Part,
    /// The top row of the rectangle.
    pub row: usize,
    /// The left column of the rectangle.
    pub column: usize,
    /// The width of the rectangle, in columns.
    pub width: usize,
    /// The height of the rectangle, in rows.
    pub height: usize,
    /// Location of the element's top row in the model file.
    pub span: SourceSpan,
}

impl Element {
    /// Extends the element's rectangle to cover the cell at `row` and `column`.
    fn cover(&mut self, row: usize, column: usize) {
        let right = (self.column + self.width).max(column + 1);
        let bottom = (self.row + self.height).max(row + 1);
        self.column = self.column.min(column);
        self.row = self.row.min(row);
        self.width = right - self.column;
        self.height = bottom - self.row;
    }
}

impl Face {
    /// Returns the rows of the picture as written, from the top.
    pub fn rows(&self) -> impl Iterator<Item = &str> {
        self.rows.iter().map(String::as_str)
    }

    /// Returns the cells of each row, from the top.
    pub fn cells(&self) -> impl Iterator<Item = &[Cell]> {
        self.cells.iter().map(Vec::as_slice)
    }

    /// Returns the cell at `row` and `column`, if the picture has one there.
    #[must_use]
    pub fn cell(&self, row: usize, column: usize) -> Option<Cell> {
        self.cells.get(row)?.get(column).copied()
    }

    /// Returns the elements of the face, in reading order of their top-left cells.
    #[must_use]
    pub fn elements(&self) -> &[Element] {
        &self.elements
    }

    /// Returns the location of `count` characters of row `row`, starting at column `column`,
    /// in the model file. Falls back to the whole picture when the row cannot be located.
    #[must_use]
    pub fn span_at(&self, row: usize, column: usize, count: usize) -> SourceSpan {
        let (Some(text), Some(Some(start))) = (self.rows.get(row), self.row_offsets.get(row))
        else {
            return self.span;
        };
        let byte = |column: usize| text.char_indices().nth(column).map_or(text.len(), |(i, _)| i);
        let (from, to) = (byte(column), byte(column + count));
        SourceSpan::from(start + from..start + to)
    }

    /// Returns the location of a whole row in the model file.
    #[must_use]
    pub fn row_span(&self, row: usize) -> SourceSpan {
        let count = self.rows.get(row).map_or(0, |text| text.chars().count());
        self.span_at(row, 0, count)
    }

    /// Cuts the picture into cells and elements. A character that is a legend key starts an
    /// element, `_` extends the element on its left and `|` the element above it, and any
    /// other character is drawn as it is.
    fn cut(&mut self, legend: &Legend, problems: &mut Vec<Problem>) {
        let mut cells: Vec<Vec<Cell>> = Vec::with_capacity(self.rows.len());
        let mut elements: Vec<Element> = Vec::new();
        for (row, text) in self.rows.iter().enumerate() {
            let mut line: Vec<Cell> = Vec::with_capacity(text.len());
            for (column, character) in text.chars().enumerate() {
                let span = self.span_at(row, column, 1);
                problems.extend(width_problem(character, span));
                let cell = match character {
                    '_' => match line.last() {
                        Some(&Cell::Element(index)) if extends_along(elements[index].part) => {
                            elements[index].cover(row, column);
                            Cell::Element(index)
                        }
                        _ => {
                            problems.push(nothing_to_continue('_', span));
                            Cell::Literal('_')
                        }
                    },
                    '|' => {
                        let above = row.checked_sub(1).and_then(|up| cells.get(up)?.get(column));
                        match above {
                            Some(&Cell::Element(index)) if extends_down(elements[index].part) => {
                                elements[index].cover(row, column);
                                Cell::Element(index)
                            }
                            _ => {
                                problems.push(nothing_to_continue('|', span));
                                Cell::Literal('|')
                            }
                        }
                    }
                    key => {
                        let Some(entry) = legend.get(key) else {
                            line.push(Cell::Literal(key));
                            continue;
                        };
                        // A run of one text key is a single text field.
                        let run = match line.last() {
                            Some(&Cell::Element(index))
                                if entry.part.kind() == PartKind::Text
                                    && elements[index].key == key =>
                            {
                                Some(index)
                            }
                            _ => None,
                        };
                        if let Some(index) = run {
                            elements[index].cover(row, column);
                            Cell::Element(index)
                        } else {
                            let part = entry.part;
                            elements.push(Element {
                                key,
                                part,
                                row,
                                column,
                                width: 1,
                                height: 1,
                                span,
                            });
                            Cell::Element(elements.len() - 1)
                        }
                    }
                };
                line.push(cell);
            }
            cells.push(line);
        }

        let mut counts = vec![0; elements.len()];
        for cell in cells.iter().flatten() {
            if let Cell::Element(index) = cell {
                counts[*index] += 1;
            }
        }
        for (element, count) in elements.iter_mut().zip(counts) {
            element.span = self.span_at(element.row, element.column, element.width);
            if count != element.width * element.height {
                problems.push(
                    Problem::new("this element is not a rectangle", element.span).with_help(
                        "`_` extends an element along its row and `|` down its column; every \
                         row of the element must be as wide as its top row",
                    ),
                );
            }
        }
        self.cells = cells;
        self.elements = elements;
    }
}

/// Parts that `_` can extend along a row: LEDs, numbered elements and text fields.
const fn extends_along(part: Part) -> bool {
    matches!(part.kind(), PartKind::Led | PartKind::List | PartKind::Text)
}

/// Parts that `|` can extend down a column: LEDs and numbered elements.
const fn extends_down(part: Part) -> bool {
    matches!(part.kind(), PartKind::Led | PartKind::List)
}

/// Reports a `_` or `|` that has no element to continue.
fn nothing_to_continue(character: char, span: SourceSpan) -> Problem {
    let (message, help) = if character == '_' {
        (
            "`_` has nothing on its left to continue",
            "`_` extends the LED, numbered element or text on its left, as in `b__` for a bay \
             three columns wide",
        )
    } else {
        (
            "`|` has nothing above it to continue",
            "`|` extends the LED or numbered element above it, as in a `b` over a `|` for a bay \
             two rows tall",
        )
    };
    Problem::new(message, span).with_help(help)
}

/// Reports a character that is not exactly one column wide.
fn width_problem(character: char, span: SourceSpan) -> Option<Problem> {
    let shown = character.escape_debug();
    match character.width() {
        Some(1) => None,
        None => Some(Problem::new(
            format!("a picture cannot contain control characters such as `{shown}`"),
            span,
        )),
        Some(width) => Some(Problem::new(
            format!("`{shown}` is {width} columns wide; each character of a picture must be one"),
            span,
        )),
    }
}

/// Reads a `face` node into `faces`.
pub fn read_face(node: &KdlNode, faces: &mut Faces, problems: &mut Vec<Problem>) {
    let mut reader = NodeReader::new(node, problems);
    let picture = reader.arg_str(0, "picture");
    let compact = reader.opt_bool("compact").unwrap_or(false);
    let strip = reader.opt_bool("strip").unwrap_or(false);
    reader.finish();

    let kind = match (compact, strip) {
        (false, false) => FaceKind::Normal,
        (true, false) => FaceKind::Compact,
        (false, true) => FaceKind::Strip,
        (true, true) => {
            problems
                .push(Problem::new("a face is either compact or a strip, not both", node.span()));
            return;
        }
    };
    let (Some(picture), Some(entry)) = (picture, node.entry(0)) else { return };
    let span = entry.span();
    let rows: Vec<String> = picture.split('\n').map(str::to_owned).collect();
    let row_offsets = locate_rows(entry, &rows);

    let slot = faces.slot(kind);
    if let Some(first) = slot {
        problems.push(
            Problem::new(format!("the model has more than one {} face", kind.name()), span)
                .with_label("given again here")
                .with_label_at(first.span, "first given here"),
        );
        return;
    }
    *slot = Some(Face { kind, span, rows, row_offsets, cells: Vec::new(), elements: Vec::new() });
}

/// Checks the faces against the model: strips only on side-mounted models, and the number
/// and width of the rows. A face of the wrong kind for the model is reported and dropped, so
/// that it is not checked any further.
pub fn check_faces(
    faces: &mut Faces,
    mount: Mount,
    height: Option<u8>,
    problems: &mut Vec<Problem>,
) {
    for kind in [FaceKind::Normal, FaceKind::Compact, FaceKind::Strip] {
        let slot = faces.slot(kind);
        let Some(face) = slot else { continue };
        let wrong_kind = match (face.kind, mount) {
            (FaceKind::Strip, Mount::Rack) => Some(
                Problem::new("a strip face needs a side-mounted model", face.span)
                    .with_help("add `mount \"side\"` to the model, or remove `strip=#true`"),
            ),
            (FaceKind::Normal | FaceKind::Compact, Mount::Side) => Some(
                Problem::new(
                    format!(
                        "a side-mounted model has only a strip face, not a {} one",
                        face.kind.name()
                    ),
                    face.span,
                )
                .with_help("add `strip=#true` to the face"),
            ),
            _ => None,
        };
        if let Some(problem) = wrong_kind {
            problems.push(problem);
            *slot = None;
            continue;
        }
        let height = usize::from(height.unwrap_or(1));
        let expected = match face.kind {
            FaceKind::Normal => Some(height * 2),
            FaceKind::Compact => Some(height),
            FaceKind::Strip => None,
        };
        if let Some(expected) = expected.filter(|&expected| expected != face.rows.len()) {
            let rows = |count: usize| {
                if count == 1 { "1 row".to_owned() } else { format!("{count} rows") }
            };
            problems.push(Problem::new(
                format!(
                    "the {} face has {}; a {height}U model needs {}",
                    face.kind.name(),
                    rows(face.rows.len()),
                    rows(expected)
                ),
                face.span,
            ));
        }
        if face.kind == FaceKind::Strip {
            for (row, text) in face.rows().enumerate() {
                let width = text.chars().count();
                if width > STRIP_WIDTH {
                    problems.push(Problem::new(
                        format!("a strip face is at most {STRIP_WIDTH} columns wide; this row has {width}"),
                        face.row_span(row),
                    ));
                }
            }
        }
    }
}

/// Cuts the faces into cells and elements, telling live characters from literal ones with
/// the legend.
pub fn cut_faces(faces: &mut Faces, legend: &Legend, problems: &mut Vec<Problem>) {
    for face in faces.iter_mut() {
        face.cut(legend, problems);
    }
}

/// Finds where each row of a picture starts in the model file, so that problems can point at
/// single characters. The rows appear in the picture's text as written in the file, in
/// order, so each one is searched for after the one before. A row that is empty, or cannot
/// be found as written, is not located.
fn locate_rows(entry: &KdlEntry, rows: &[String]) -> Vec<Option<usize>> {
    let Some(written) = entry.format().map(|format| format.value_repr.as_str()) else {
        return vec![None; rows.len()];
    };
    let mut from = 0;
    rows.iter()
        .map(|row| {
            if row.is_empty() {
                return None;
            }
            let found = from + written.get(from..)?.find(row.as_str())?;
            from = found + row.len();
            Some(entry.span().offset() + found)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Model;

    fn model(text: &str) -> Model {
        Model::parse("x/y", text).expect("valid model")
    }

    fn messages(text: &str) -> Vec<String> {
        Model::parse("x/y", text)
            .expect_err("the model has problems")
            .iter()
            .map(|problem| problem.message().to_owned())
            .collect()
    }

    #[test]
    fn reads_each_kind_of_face() {
        let text = r##"model {
            name "X"; kind "server"; height 1
            face #"""
              p nnnn
              bbbbbb
              """#
            face compact=#true "p nnnn bbbbbb"
            legend { p power; n name; b bay }
        }"##;
        let faces = model(text).faces;
        let normal = faces.normal.expect("normal face");
        assert_eq!(normal.rows().collect::<Vec<_>>(), ["p nnnn", "bbbbbb"]);
        assert_eq!(faces.compact.expect("compact face").kind, FaceKind::Compact);
        assert!(faces.strip.is_none());
    }

    #[test]
    fn locates_picture_characters_in_the_file() {
        let text = r##"model {
            name "X"; kind "server"; height 1
            face #"""
              p nnnn
              bbbbbb
              """#
            face compact=#true "p nnnn bbbbbb"
            legend { p power; n name; b bay }
        }"##;
        let faces = model(text).faces;
        let normal = faces.normal.expect("normal face");
        let span = normal.span_at(1, 2, 3);
        assert_eq!(&text[span.offset()..span.offset() + span.len()], "bbb");
        let compact = faces.compact.expect("compact face");
        let span = compact.span_at(0, 2, 4);
        assert_eq!(&text[span.offset()..span.offset() + span.len()], "nnnn");
    }

    #[test]
    fn falls_back_to_the_whole_picture_for_rows_written_with_escapes() {
        let text =
            r#"model { name "X"; kind "server"; face "p\u{62}\nbb"; legend { p power; b bay } }"#;
        let normal = model(text).faces.normal.expect("normal face");
        assert_eq!(normal.rows().collect::<Vec<_>>(), ["pb", "bb"]);
        assert_eq!(normal.span_at(0, 0, 1), normal.span);
        let span = normal.span_at(1, 0, 2);
        assert_eq!(&text[span.offset()..span.offset() + span.len()], "bb");
    }

    #[test]
    fn reads_a_strip_on_a_side_mounted_model() {
        let text = r##"model {
            name "Strip"; kind "pdu"; mount "side"
            face strip=#true #"""
              p
              ~
              o
              """#
            legend { p power; o outlet; ~ fill }
        }"##;
        assert!(model(text).faces.strip.is_some());
    }

    #[test]
    fn reports_faces_that_do_not_fit_the_model() {
        assert_eq!(
            messages(
                r#"model {
                    name "X"; kind "server"; height 1
                    face "p"
                    face "b"
                    face compact=#true strip=#true "x"
                    face strip=#true "p"
                    face compact=#true "p"
                    legend { p power; b bay }
                }"#
            ),
            [
                "the model has more than one normal face",
                "a face is either compact or a strip, not both",
                "the normal face has 1 row; a 1U model needs 2 rows",
                "a strip face needs a side-mounted model",
            ]
        );
    }

    #[test]
    fn reports_normal_faces_on_side_mounted_models_and_wide_strips() {
        let text = r#"model {
            name "Strip"; kind "pdu"; mount "side"
            face "p"
            face strip=#true "pppppp"
            legend { p power }
        }"#;
        let problems = Model::parse("x/y", text).expect_err("problems");
        let found: Vec<_> = problems.iter().map(Problem::message).collect();
        assert_eq!(
            found,
            [
                "a side-mounted model has only a strip face, not a normal one",
                "a strip face is at most 5 columns wide; this row has 6",
            ]
        );
        let span = problems[1].span();
        assert_eq!(&text[span.offset()..span.offset() + span.len()], "pppppp");
    }

    #[test]
    fn reports_a_picture_that_is_not_a_string() {
        assert_eq!(
            messages(r#"model { name "X"; kind "server"; face 5 compact="yes" }"#),
            [
                "`picture` must be a string, found the number 5",
                "`compact` must be #true or #false, found the string \"yes\"",
            ]
        );
    }

    /// The model text of a 1U server whose normal face is `rows`, described by `legend`.
    fn server(rows: [&str; 2], legend: &str) -> String {
        let [top, bottom] = rows;
        format!(
            "model {{ name \"X\"; kind \"server\"\nface #\"\"\"\n{top}\n{bottom}\n\"\"\"#\n\
             legend {{ {legend} }} }}"
        )
    }

    fn normal_face(text: &str) -> Face {
        model(text).faces.normal.expect("normal face")
    }

    /// Returns each element as its key, rectangle (row, column, width, height) and the text
    /// its top row covers in the model file.
    fn elements(text: &str) -> Vec<(char, [usize; 4], String)> {
        normal_face(text)
            .elements()
            .iter()
            .map(|e| {
                let written = &text[e.span.offset()..e.span.offset() + e.span.len()];
                (e.key, [e.row, e.column, e.width, e.height], written.to_owned())
            })
            .collect()
    }

    #[test]
    fn cuts_the_picture_into_elements_and_literals() {
        let text = server(["p N bbx", "~~ tttt"], "p power; b bay; t short; ~ fill");
        let face = normal_face(&text);
        assert_eq!(face.cell(0, 2), Some(Cell::Literal('N')));
        assert_eq!(face.cell(0, 6), Some(Cell::Literal('x')));
        assert_eq!(face.cell(1, 2), Some(Cell::Literal(' ')));
        assert_eq!(face.cell(1, 7), None);
        assert_eq!(
            elements(&text),
            [
                ('p', [0, 0, 1, 1], "p".to_owned()),
                ('b', [0, 4, 1, 1], "b".to_owned()),
                ('b', [0, 5, 1, 1], "b".to_owned()),
                ('~', [1, 0, 1, 1], "~".to_owned()),
                ('~', [1, 1, 1, 1], "~".to_owned()),
                ('t', [1, 3, 4, 1], "tttt".to_owned()),
            ]
        );
        assert_eq!(face.cell(1, 5), Some(Cell::Element(5)));
    }

    #[test]
    fn extends_elements_with_underscores_and_bars() {
        let text = server(["b__ c_ tt_", "||| ||"], "b bay; c psu; t short");
        assert_eq!(
            elements(&text),
            [
                ('b', [0, 0, 3, 2], "b__".to_owned()),
                ('c', [0, 4, 2, 2], "c_".to_owned()),
                ('t', [0, 7, 3, 1], "tt_".to_owned()),
            ]
        );
    }

    #[test]
    fn reports_underscores_and_bars_with_nothing_to_continue() {
        let text = server(["_b ~_ tt", "|     |"], "b bay; t short; ~ fill");
        assert_eq!(
            messages(&text),
            [
                "`_` has nothing on its left to continue",
                "`_` has nothing on its left to continue",
                "`|` has nothing above it to continue",
                "`|` has nothing above it to continue",
            ]
        );
    }

    #[test]
    fn reports_elements_that_are_not_rectangles() {
        let text = server(["b_ c", "|  |_"], "b bay; c bay");
        let problems = Model::parse("x/y", &text).expect_err("not rectangles");
        let found: Vec<_> = problems
            .iter()
            .map(|p| (p.message(), &text[p.span().offset()..p.span().offset() + p.span().len()]))
            .collect();
        assert_eq!(
            found,
            [("this element is not a rectangle", "b_"), ("this element is not a rectangle", "c")]
        );
    }

    #[test]
    fn reports_characters_that_are_not_one_column_wide() {
        let text = r#"model { name "X"; kind "server"; face "中b\n\tb"; legend { b bay } }"#;
        assert_eq!(
            messages(text),
            [
                "`中` is 2 columns wide; each character of a picture must be one",
                "a picture cannot contain control characters such as `\\t`",
            ]
        );
    }
}
