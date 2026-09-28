//! Helpers shared by the unit tests.

use std::fs;

use miette::SourceSpan;

use crate::kdl_reader::Problem;

/// The part of `text` that `span` covers.
pub fn covered(text: &str, span: SourceSpan) -> &str {
    &text[span.offset()..span.offset() + span.len()]
}

/// Each problem's message with the part of `text` it points at.
pub fn pointed<'a>(text: &'a str, problems: &'a [Problem]) -> Vec<(&'a str, &'a str)> {
    problems.iter().map(|problem| (problem.message(), covered(text, problem.span()))).collect()
}

/// Creates a temporary directory holding `files`, given as path and contents.
pub fn dir_with(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary directory");
    for (path, text) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().expect("file inside the directory")).expect("mkdir");
        fs::write(path, text).expect("write file");
    }
    dir
}
