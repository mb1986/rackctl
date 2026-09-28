//! Faces: the pictures a model's front panel is drawn from.

use kdl::{KdlEntry, KdlNode};
use miette::SourceSpan;
use unicode_width::UnicodeWidthChar;

use super::legend::{Legend, Part, PartKind};
use super::model::Mount;
use super::numbering::Numbers;
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
    /// Returns the faces the model has.
    pub fn iter(&self) -> impl Iterator<Item = &Face> {
        [&self.normal, &self.compact, &self.strip].into_iter().flatten()
    }

    pub(super) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Face> {
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
    /// Returns the name used in messages, such as `normal`.
    #[must_use]
    pub const fn name(self) -> &'static str {
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
    /// The elements that are not rectangles, already reported.
    irregular: Vec<usize>,
    /// The number table of each part the face numbers without problems.
    numbers: Vec<Numbers>,
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
    /// For a number field, the numbered element whose number it shows: the one it touches
    /// on its left or right.
    pub number_of: Option<usize>,
    /// For a numbered element, such as a bay or a port, its number.
    pub number: Option<u16>,
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

    /// Returns the number table of a numbered part, such as [`Part::Port`] for all the ports,
    /// RJ45 and SFP alike. A part the face does not show has none.
    #[must_use]
    pub fn numbers(&self, part: Part) -> Option<&Numbers> {
        self.numbers.iter().find(|numbers| numbers.part() == part)
    }

    /// Gives the element at `index` in [`Face::elements`] its number.
    pub(super) fn set_number(&mut self, index: usize, number: u16) {
        if let Some(element) = self.elements.get_mut(index) {
            element.number = Some(number);
        }
    }

    /// Keeps the number table of a part.
    pub(super) fn add_numbers(&mut self, numbers: Numbers) {
        self.numbers.push(numbers);
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

    /// Returns the location of the bytes `from..to` of row `row` in the model file, or of the
    /// whole picture when the row cannot be located.
    fn bytes_span(&self, row: usize, from: usize, to: usize) -> SourceSpan {
        match self.row_offsets.get(row) {
            Some(&Some(start)) => SourceSpan::from(start + from..start + to),
            _ => self.span,
        }
    }

    /// Cuts the picture into cells and elements. A character that is a legend key starts an
    /// element; `_` extends the LED, numbered element or text on its left, and `|` the LED
    /// or numbered element above it; any other character is drawn as it is.
    fn cut(&mut self, legend: &Legend, problems: &mut Vec<Problem>) {
        // The byte offset of each character of each row, and of the row's end.
        let offsets: Vec<Vec<usize>> = self
            .rows
            .iter()
            .map(|text| text.char_indices().map(|(byte, _)| byte).chain([text.len()]).collect())
            .collect();
        let mut cells: Vec<Vec<Cell>> = Vec::with_capacity(self.rows.len());
        let mut elements: Vec<Element> = Vec::new();
        for (row, text) in self.rows.iter().enumerate() {
            let mut line: Vec<Cell> = Vec::with_capacity(text.len());
            for (column, character) in text.chars().enumerate() {
                let span = self.bytes_span(row, offsets[row][column], offsets[row][column + 1]);
                problems.extend(width_problem(character, span));
                let above = row.checked_sub(1).and_then(|up| cells.get(up)?.get(column)).copied();
                let cell = match character {
                    '_' => match line.last() {
                        Some(&Cell::Element(index)) if extends_along(elements[index].part) => {
                            elements[index].cover(row, column);
                            Cell::Element(index)
                        }
                        // A run of stray `_` is reported once.
                        Some(Cell::Literal('_')) => Cell::Literal('_'),
                        _ => {
                            problems.push(nothing_to_continue('_', span));
                            Cell::Literal('_')
                        }
                    },
                    '|' => match above {
                        Some(Cell::Element(index)) if extends_down(elements[index].part) => {
                            elements[index].cover(row, column);
                            Cell::Element(index)
                        }
                        // Stray `|` next to or under another one are reported once.
                        _ if above == Some(Cell::Literal('|'))
                            || line.last() == Some(&Cell::Literal('|')) =>
                        {
                            Cell::Literal('|')
                        }
                        _ => {
                            problems.push(nothing_to_continue('|', span));
                            Cell::Literal('|')
                        }
                    },
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
                                number_of: None,
                                number: None,
                            });
                            Cell::Element(elements.len() - 1)
                        }
                    }
                };
                line.push(cell);
            }
            cells.push(line);
        }

        let irregular = self.check_rectangles(&offsets, &cells, &mut elements, problems);
        link_number_fields(&cells, &mut elements, legend, problems);
        self.cells = cells;
        self.elements = elements;
        self.irregular = irregular;
    }

    /// Sets each element's location to its top row, reports the elements that are not
    /// rectangles and returns their indices.
    fn check_rectangles(
        &self,
        offsets: &[Vec<usize>],
        cells: &[Vec<Cell>],
        elements: &mut [Element],
        problems: &mut Vec<Problem>,
    ) -> Vec<usize> {
        let mut counts = vec![0; elements.len()];
        for cell in cells.iter().flatten() {
            if let Cell::Element(index) = cell {
                counts[*index] += 1;
            }
        }
        let mut irregular = Vec::new();
        for (index, (element, count)) in elements.iter_mut().zip(counts).enumerate() {
            let row = &offsets[element.row];
            let end =
                row.get(element.column + element.width).copied().unwrap_or(row[row.len() - 1]);
            element.span = self.bytes_span(element.row, row[element.column], end);
            if count != element.width * element.height {
                irregular.push(index);
                problems.push(
                    Problem::new("this element is not a rectangle", element.span).with_help(
                        "`_` extends an element along its row and `|` down its column; every \
                         row of the element must be as wide as its top row",
                    ),
                );
            }
        }
        irregular
    }
}

/// Links each number field to the numbered element it touches on its left or right, and
/// checks that it has room for a number besides its gap.
fn link_number_fields(
    cells: &[Vec<Cell>],
    elements: &mut [Element],
    legend: &Legend,
    problems: &mut Vec<Problem>,
) {
    for index in 0..elements.len() {
        let field = &elements[index];
        if field.part != Part::Number {
            continue;
        }
        let numbered_at = |column: Option<usize>| match cells[field.row].get(column?) {
            Some(&Cell::Element(other)) if elements[other].part.kind() == PartKind::List => {
                Some(other)
            }
            _ => None,
        };
        let left = numbered_at(field.column.checked_sub(1));
        let right = numbered_at(Some(field.column + field.width));
        let gap = legend.get(field.key).map_or(0, |entry| entry.gap);
        let problem = match (left, right) {
            (Some(_), Some(_)) => Some("this number field touches numbered elements on both sides"),
            (None, None) => Some("a number field must touch a bay, port or other numbered element"),
            _ => None,
        };
        let span = field.span;
        if let Some(message) = problem {
            problems.push(Problem::new(message, span).with_help(
                "put it directly on the left or right of one numbered element, as in `###o`",
            ));
            continue;
        }
        if field.width <= gap {
            problems.push(Problem::new(
                format!("this number field has no room for a number besides its `gap={gap}`"),
                span,
            ));
        }
        elements[index].number_of = left.xor(right);
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
            format!("control characters such as `{shown}` cannot be drawn"),
            span,
        )),
        Some(width) => Some(Problem::new(
            format!("`{shown}` is {width} columns wide; every character must be one column"),
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
    *slot = Some(Face {
        kind,
        span,
        rows,
        row_offsets,
        cells: Vec::new(),
        elements: Vec::new(),
        irregular: Vec::new(),
        numbers: Vec::new(),
    });
}

/// Checks the faces against the model: strips only on side-mounted models, and the number
/// and width of the rows. A face of the wrong kind for the model is reported and dropped, so
/// that it is not checked any further. `height` is `None` for an invalid height, and then the
/// number of rows is not checked.
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
                .with_help(if face.kind == FaceKind::Compact {
                    "replace `compact=#true` with `strip=#true`"
                } else {
                    "add `strip=#true` to the face"
                }),
            ),
            _ => None,
        };
        if let Some(problem) = wrong_kind {
            problems.push(problem);
            *slot = None;
            continue;
        }
        let expected = match (face.kind, height.map(usize::from)) {
            (FaceKind::Normal, Some(height)) => Some((height, height * 2)),
            (FaceKind::Compact, Some(height)) => Some((height, height)),
            _ => None,
        };
        if let Some((height, expected)) = expected.filter(|&(_, rows)| rows != face.rows.len()) {
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

/// Checks that every glyph fits the elements drawn with it: each of its characters is one
/// column wide, and it is one character, which fills the element, or one per cell.
pub fn check_glyphs(faces: &Faces, legend: &Legend, problems: &mut Vec<Problem>) {
    // Elements that are not rectangles have no size to compare with; they are reported.
    let elements: Vec<&Element> = faces
        .iter()
        .flat_map(|face| {
            face.elements.iter().enumerate().filter(|(index, _)| !face.irregular.contains(index))
        })
        .map(|(_, element)| element)
        .collect();
    for entry in legend.entries() {
        for glyph in entry.glyphs() {
            // The legend reports control characters and this reports wide ones; a glyph with
            // either is not size checked.
            if glyph.value.chars().any(|c| c.width() != Some(1)) {
                problems.extend(
                    glyph
                        .value
                        .chars()
                        .filter(|c| c.width().is_some())
                        .find_map(|c| width_problem(c, glyph.span)),
                );
                continue;
            }
            let length = glyph.value.chars().count();
            let misfit = elements.iter().find(|element| {
                element.key == entry.key && length > 1 && element.width * element.height != length
            });
            if let Some(element) = misfit {
                let cells = element.width * element.height;
                let cells = if cells == 1 { "1 cell".to_owned() } else { format!("{cells} cells") };
                problems.push(
                    Problem::new(
                        format!(
                            "this glyph has {length} characters, but a `{}` element covers {cells}",
                            entry.key
                        ),
                        glyph.span,
                    )
                    .with_label_at(element.span, format!("covers {cells}"))
                    .with_help(
                        "a glyph is one character, which fills the element, or one per cell",
                    ),
                );
            }
        }
    }
}

/// Reports legend keys that no face uses.
pub fn check_unused_keys(faces: &Faces, legend: &Legend, problems: &mut Vec<Problem>) {
    for entry in legend.entries() {
        if !faces.iter().flat_map(Face::elements).any(|element| element.key == entry.key) {
            problems.push(
                Problem::new(
                    format!("legend key `{}` is not used by any face", entry.key),
                    entry.span,
                )
                .with_help("use it in a picture, or remove it from the legend"),
            );
        }
    }
}

/// Finds where each non-empty row of a picture starts in the model file. Rows from the first
/// one not written as it is, such as one with an escape, are not located.
fn locate_rows(entry: &KdlEntry, rows: &[String]) -> Vec<Option<usize>> {
    let mut starts = vec![None; rows.len()];
    let Some(written) = entry.format().map(|format| format.value_repr.as_str()) else {
        return starts;
    };
    if rows.is_empty() {
        return starts;
    }
    // The entry's span may start with a type annotation, but it ends where the value does.
    let span = entry.span();
    let offset = span.offset() + span.len() - written.len();
    let lines = kdl_lines(written);
    // A multi-line picture ends with a line holding only the indent and the closing quotes.
    let Some(&(_, last)) = lines.last().filter(|_| lines.len() > 1) else {
        let row = rows[0].as_str();
        let at = written.find('"').map(|at| at + 1).filter(|_| !row.is_empty());
        starts[0] = at.filter(|&at| written[at..].starts_with(row)).map(|at| offset + at);
        return starts;
    };
    let indent = last.len() - last.trim_start().len();
    for (&(start, line), (row, slot)) in lines[1..].iter().zip(rows.iter().zip(&mut starts)) {
        if row.is_empty() && line.trim().is_empty() {
            continue;
        }
        if line.get(indent..) != Some(row.as_str()) {
            break;
        }
        *slot = Some(offset + start + indent);
    }
    starts
}

/// Splits text at the newlines KDL accepts, giving where each line starts and its text.
fn kdl_lines(text: &str) -> Vec<(usize, &str)> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if matches!(c, '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}') {
            lines.push((start, &text[start..at]));
            let crlf = c == '\r' && chars.next_if(|&(_, next)| next == '\n').is_some();
            start = at + c.len_utf8() + usize::from(crlf);
        }
    }
    lines.push((start, &text[start..]));
    lines
}

#[cfg(test)]
mod tests {
    use indoc::{formatdoc, indoc};
    use miette::Diagnostic;

    use super::*;
    use crate::catalog::Model;
    use crate::testing::{covered, pointed};

    fn model(text: &str) -> Model {
        Model::parse("x/y", text).expect("valid model")
    }

    fn problems(text: &str) -> Vec<Problem> {
        Model::parse("x/y", text).expect_err("the model has problems")
    }

    fn messages(text: &str) -> Vec<String> {
        problems(text).iter().map(|problem| problem.message().to_owned()).collect()
    }

    /// The model text of a 1U server with the part counts `counts`, such as `bays 2`, whose
    /// normal face is `rows`, described by `legend`.
    fn server(rows: [&str; 2], counts: &str, legend: &str) -> String {
        let [top, bottom] = rows;
        formatdoc! {r##"
            model {{ name "X"; kind "server"; {counts}
            face #"""
            {top}
            {bottom}
            """#
            legend {{ {legend} }} }}"##}
    }

    fn normal_face(text: &str) -> Face {
        model(text).faces.normal.expect("normal face")
    }

    /// Returns each element as its key, rectangle (row, column, width, height) and the text
    /// its top row covers in the model file.
    fn elements(text: &str) -> Vec<(char, [usize; 4], &str)> {
        normal_face(text)
            .elements()
            .iter()
            .map(|e| (e.key, [e.row, e.column, e.width, e.height], covered(text, e.span)))
            .collect()
    }

    #[test]
    fn reads_faces_and_locates_their_characters() {
        let text = r##"model {
            name "X"; kind "server"; height 1; bays 6
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
        assert_eq!(covered(text, normal.span_at(1, 2, 3)), "bbb");
        let compact = faces.compact.expect("compact face");
        assert_eq!(compact.kind, FaceKind::Compact);
        assert_eq!(covered(text, compact.span_at(0, 2, 4)), "nnnn");
        assert!(faces.strip.is_none());
    }

    #[test]
    fn reads_a_strip_on_a_side_mounted_model() {
        let text = r##"model {
            name "Strip"; kind "pdu"; mount "side"; outlets 1
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
    fn reports_a_picture_that_is_not_a_string() {
        assert_eq!(
            messages(r#"model { name "X"; kind "server"; face 5 compact="yes" }"#),
            [
                "`picture` must be a string, found the number 5",
                "`compact` must be #true or #false, found the string \"yes\"",
            ]
        );
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
    fn reports_faces_that_do_not_fit_a_side_mounted_model() {
        let text = r#"model {
            name "Strip"; kind "pdu"; mount "side"
            face "p"
            face compact=#true "p"
            face strip=#true "pppppp"
            legend { p power }
        }"#;
        let problems = problems(text);
        assert_eq!(
            pointed(text, &problems),
            [
                ("a side-mounted model has only a strip face, not a normal one", r#""p""#),
                ("a side-mounted model has only a strip face, not a compact one", r#""p""#),
                ("a strip face is at most 5 columns wide; this row has 6", "pppppp"),
            ]
        );
        assert_eq!(problems[1].help(), Some("replace `compact=#true` with `strip=#true`"));
    }

    #[test]
    fn reports_an_invalid_height_once() {
        // Four rows would be too many for 1U, but no face is checked against an invalid height.
        let model = |height| {
            format!(
                r#"model {{ name "X"; kind "server"; height {height}; bays 3; face "p\nb\nb\nb"; legend {{ p power; b bay }} }}"#
            )
        };
        assert_eq!(messages(&model("0")), ["`height` must be at least 1"]);
        assert_eq!(messages(&model("\"2\"")).len(), 1);
    }

    #[test]
    fn points_at_the_whole_picture_for_rows_not_located() {
        // The first row is written with an escape, so neither row is located.
        let text = r#"model { name "X"; kind "server"; bays 3; face "p\u{62}\nbb"; legend { p power; b bay } }"#;
        let normal = normal_face(text);
        assert_eq!(normal.rows().collect::<Vec<_>>(), ["pb", "bb"]);
        assert_eq!(normal.span_at(0, 0, 1), normal.span);
        assert_eq!(normal.span_at(1, 0, 2), normal.span);
    }

    /// Where `locate_rows` finds the rows of the picture in `text`, a single `face` node.
    fn located(text: &str) -> Vec<Option<usize>> {
        let document = kdl::KdlDocument::parse_v2(text).expect("valid KDL");
        let entry = document.nodes()[0].entry(0).expect("a picture");
        let picture = entry.value().as_string().expect("a string");
        let rows: Vec<String> = picture.split('\n').map(str::to_owned).collect();
        locate_rows(entry, &rows)
    }

    #[test]
    fn locates_rows_of_a_picture_written_over_several_lines() {
        // CRLF, a blank row and a row indented more than the others.
        let text = "face #\"\"\"\r\n    p nn\r\n\r\n      bb\r\n    \"\"\"#";
        assert_eq!(located(text), [text.find("p nn"), None, text.find("  bb")]);
        // Newlines other than LF, and a row that could be taken for the opening quotes.
        let text = "face #\"\"\"\u{85}  #\u{2028}  ab\u{2028}  \"\"\"#";
        assert_eq!(located(text), [text.find("\u{85}  #").map(|at| at + 4), text.find("ab")]);
    }

    #[test]
    fn locates_the_row_of_a_picture_written_on_one_line() {
        assert_eq!(located(r#"face "p nn""#), [Some(6)]);
        assert_eq!(located(r##"face #"a"b"#"##), [Some(7)]);
        assert_eq!(located(r#"face """#), [None]);
        assert_eq!(located(r#"face (pic)"p nn""#), [Some(11)]);
    }

    #[test]
    fn stops_locating_rows_at_the_first_one_written_with_an_escape() {
        let text = indoc! {r#"
            face """
              ab
              c\"d
              ef
              """
        "#};
        assert_eq!(located(text), [text.find("ab"), None, None]);
        // A `\` at the end of a line joins it to the next one.
        let text = indoc! {r#"
            face """
              ab\
              cd
              ef
              """
        "#};
        assert_eq!(located(text), [None, None]);
    }

    #[test]
    fn cuts_the_picture_into_elements_and_literals() {
        let text = server(["p N bbx", "~~ tttt"], "bays 2", "p power; b bay; t short; ~ fill");
        let face = normal_face(&text);
        assert_eq!(face.cell(0, 2), Some(Cell::Literal('N')));
        assert_eq!(face.cell(0, 6), Some(Cell::Literal('x')));
        assert_eq!(face.cell(1, 2), Some(Cell::Literal(' ')));
        assert_eq!(face.cell(1, 7), None);
        assert_eq!(
            elements(&text),
            [
                ('p', [0, 0, 1, 1], "p"),
                ('b', [0, 4, 1, 1], "b"),
                ('b', [0, 5, 1, 1], "b"),
                ('~', [1, 0, 1, 1], "~"),
                ('~', [1, 1, 1, 1], "~"),
                ('t', [1, 3, 4, 1], "tttt"),
            ]
        );
        assert_eq!(face.cell(1, 5), Some(Cell::Element(5)));
    }

    #[test]
    fn extends_elements_with_underscores_and_bars() {
        let text = server(["b__ c_ tt_", "||| ||"], "bays 1; psus 1", "b bay; c psu; t short");
        assert_eq!(
            elements(&text),
            [('b', [0, 0, 3, 2], "b__"), ('c', [0, 4, 2, 2], "c_"), ('t', [0, 7, 3, 1], "tt_")]
        );
    }

    #[test]
    fn reports_underscores_and_bars_with_nothing_to_continue() {
        // Each run is reported once. Neither continues a literal or a fill, and `|` does not
        // continue a text field either.
        let text = server(["_b x__ ~_ tt", "   |      ||"], "bays 1", "b bay; t short; ~ fill");
        assert_eq!(
            messages(&text),
            [
                "`_` has nothing on its left to continue",
                "`_` has nothing on its left to continue",
                "`_` has nothing on its left to continue",
                "`|` has nothing above it to continue",
                "`|` has nothing above it to continue",
            ]
        );
    }

    #[test]
    fn reports_elements_that_are_not_rectangles() {
        let text = server(["b_ c", "|  |_"], "bays 1; psus 1", "b bay; c psu");
        assert_eq!(
            pointed(&text, &problems(&text)),
            [("this element is not a rectangle", "b_"), ("this element is not a rectangle", "c")]
        );
    }

    #[test]
    fn reports_characters_that_are_not_one_column_wide() {
        let text = server(["中b", "\tb"], "bays 2", "b bay");
        assert_eq!(
            pointed(&text, &problems(&text)),
            [
                ("`中` is 2 columns wide; every character must be one column", "中"),
                ("control characters such as `\\t` cannot be drawn", "\t"),
            ]
        );
    }

    #[test]
    fn links_number_fields_to_the_element_they_touch() {
        let text = server(
            ["##b c##", "~"],
            "bays 1; psus 1",
            r##""#" number gap=1; b bay; c psu; ~ fill"##,
        );
        let face = normal_face(&text);
        let linked: Vec<_> = face
            .elements()
            .iter()
            .filter_map(|e| Some((e.column, face.elements()[e.number_of?].key)))
            .collect();
        assert_eq!(linked, [(0, 'b'), (5, 'c')]);
    }

    #[test]
    fn reports_number_fields_without_one_element_to_number() {
        // A lone field is reported only once, not also for having no room besides its gap.
        let text = server(
            ["# b##b #b ##p #t", "~"],
            "bays 3",
            r##""#" number gap=1; b bay; p power; t short; ~ fill"##,
        );
        let touch = "a number field must touch a bay, port or other numbered element";
        assert_eq!(
            pointed(&text, &problems(&text)),
            [
                (touch, "#"),
                ("this number field touches numbered elements on both sides", "##"),
                ("this number field has no room for a number besides its `gap=1`", "#"),
                (touch, "##"),
                (touch, "#"),
            ]
        );
    }

    #[test]
    fn accepts_glyphs_of_one_character_or_one_per_cell() {
        let text = server(["b__ c c", "~"], "bays 1; psus 2", r#"b bay="abc"; c psu="■"; ~ fill"#);
        assert!(Model::parse("x/y", &text).is_ok());
    }

    #[test]
    fn reports_glyphs_that_do_not_fit_their_elements() {
        let text =
            server(["b__ b cc", "~"], "bays 2; psus 2", r#"b bay="abc"; c psu="中"; ~ fill"#);
        let problems = problems(&text);
        assert_eq!(
            pointed(&text, &problems),
            [
                ("this glyph has 3 characters, but a `b` element covers 1 cell", r#"bay="abc""#),
                ("`中` is 2 columns wide; every character must be one column", r#"psu="中""#),
            ]
        );
        let labels: Vec<_> = problems[0]
            .labels()
            .expect("labels")
            .map(|label| covered(&text, *label.inner()))
            .collect();
        assert_eq!(labels, [r#"bay="abc""#, "b"]);
    }

    #[test]
    fn reports_a_bad_glyph_once() {
        let text = server(["b__ b_", "~"], "bays 2", r#"b bay="a\u{301}"; ~ fill"#);
        assert_eq!(
            messages(&text),
            ["`\\u{301}` is 0 columns wide; every character must be one column"]
        );
        // Two characters for three cells, but the control character is all that is reported.
        let text = server(["b__", "~"], "bays 1", r#"b bay="a\t"; ~ fill"#);
        assert_eq!(messages(&text), ["a glyph must not contain control characters"]);
    }

    #[test]
    fn does_not_size_check_elements_that_are_not_rectangles() {
        let text = server(["b_", "| "], "bays 1", r#"b bay="abc""#);
        assert_eq!(messages(&text), ["this element is not a rectangle"]);
    }

    #[test]
    fn holds_back_unused_keys_while_the_model_has_other_problems() {
        let text = server(["p", "~"], "", "p power; b bay; ~ fill");
        assert_eq!(
            pointed(&text, &problems(&text)),
            [("legend key `b` is not used by any face", "b bay")]
        );
        let with_other_problem = server(["_p", "~"], "", "p power; b bay; ~ fill");
        assert!(!messages(&with_other_problem).iter().any(|m| m.contains("not used")));
    }
}
