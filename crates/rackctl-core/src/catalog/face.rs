//! Faces: the pictures a model's front panel is drawn from.

use kdl::{KdlEntry, KdlNode};
use miette::SourceSpan;

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

/// One face of a model: its picture, row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Face {
    /// Which face this is.
    pub kind: FaceKind,
    /// Location of the picture in the model file.
    pub span: SourceSpan,
    rows: Vec<String>,
    /// Where each row starts in the model file, when it is written there as it is.
    row_offsets: Vec<Option<usize>>,
}

impl Face {
    /// Returns the rows of the picture, from the top.
    pub fn rows(&self) -> impl Iterator<Item = &str> {
        self.rows.iter().map(String::as_str)
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
    *slot = Some(Face { kind, span, rows, row_offsets });
}

/// Checks the faces against the model: strips only on side-mounted models, and the number
/// and width of the rows.
pub fn check_faces(faces: &Faces, mount: Mount, height: Option<u8>, problems: &mut Vec<Problem>) {
    for face in faces.iter() {
        // A face of the wrong kind for the model is not checked any further.
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
}
