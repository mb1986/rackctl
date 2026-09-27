//! Reading values from KDL documents with helpful error reporting.
//!
//! Configuration files are written by hand, so mistakes are expected. Instead of stopping at
//! the first one, the helpers in this module collect every problem they find, each pointing
//! at the exact place in the file, so that all of them can be fixed at once.
//!
//! Typical use: parse the text with [`parse`], wrap each node in a [`NodeReader`], read the
//! values the node should contain and call [`NodeReader::finish`] to report anything
//! unexpected. [`FileError`] then presents all problems together with the file's contents.

use std::io;
use std::str::FromStr;

use kdl::{KdlDocument, KdlEntry, KdlNode, KdlValue};
use miette::{Diagnostic, LabeledSpan, NamedSource, Report, SourceSpan};
use strum::VariantNames;
use thiserror::Error;

/// A mistake found in a KDL file, together with the location it refers to.
#[derive(Debug, Clone, PartialEq, Eq, Error, Diagnostic)]
#[error("{message}")]
pub struct Problem {
    message: String,
    #[label("{label}")]
    span: SourceSpan,
    label: String,
    #[label(collection)]
    other_labels: Vec<LabeledSpan>,
    #[help]
    help: Option<String>,
}

impl Problem {
    /// Creates a problem described by `message`, located at `span`.
    pub fn new(message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            message: message.into(),
            span,
            label: "here".to_owned(),
            other_labels: Vec::new(),
            help: None,
        }
    }

    /// Sets the short note displayed under the highlighted text.
    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Highlights another place in the same file that the problem involves, such as the
    /// other device in an overlap.
    #[must_use]
    pub fn with_label_at(mut self, span: SourceSpan, label: impl Into<String>) -> Self {
        self.other_labels.push(LabeledSpan::new_with_span(Some(label.into()), span));
        self
    }

    /// Adds a hint that tells the user how to fix the problem.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Returns the description of the problem.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the location of the problem in the source text.
    #[must_use]
    pub const fn span(&self) -> SourceSpan {
        self.span
    }

    /// Returns the hint for fixing the problem, if there is one.
    #[must_use]
    pub fn help(&self) -> Option<&str> {
        self.help.as_deref()
    }
}

/// Every problem found in one file, bundled with the file's contents so that each problem
/// can be displayed in context.
#[derive(Debug, Clone, Error, Diagnostic)]
#[error("{name}: {}", count(.problems.len()))]
pub struct FileError {
    name: String,
    #[source_code]
    src: NamedSource<String>,
    #[related]
    problems: Vec<Problem>,
}

impl FileError {
    /// Creates an error for the file `name`, whose contents are `text`.
    pub fn new(name: impl Into<String>, text: impl Into<String>, problems: Vec<Problem>) -> Self {
        let name = name.into();
        Self { src: NamedSource::new(&name, text.into()), name, problems }
    }

    /// Creates an error for the file `name`, which could not be read.
    pub fn unreadable(name: impl Into<String>, error: &io::Error) -> Self {
        let problem =
            Problem::new(format!("cannot read the file: {error}"), SourceSpan::from(0..0));
        Self::new(name, "", vec![problem])
    }

    /// Returns the individual problems.
    #[must_use]
    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    /// Returns one report per problem, each showing the problem in the file's text. When the
    /// text is not available, for example because the file cannot be read, the report names
    /// the file instead.
    #[must_use]
    pub fn reports(&self) -> Vec<Report> {
        self.problems
            .iter()
            .map(|problem| {
                if self.src.inner().is_empty() {
                    let problem = Problem {
                        message: format!("{}: {}", self.name, problem.message),
                        ..problem.clone()
                    };
                    Report::new(problem)
                } else {
                    Report::new(problem.clone()).with_source_code(self.src.clone())
                }
            })
            .collect()
    }
}

/// Formats a number of problems, for example `1 problem` or `3 problems`.
fn count(problems: usize) -> String {
    if problems == 1 { "1 problem".to_owned() } else { format!("{problems} problems") }
}

/// Parses `text` as a KDL v2 document.
///
/// # Errors
///
/// Returns one problem for each syntax error in `text`.
pub fn parse(text: &str) -> Result<KdlDocument, Vec<Problem>> {
    KdlDocument::parse_v2(text).map_err(|error| {
        error
            .diagnostics
            .into_iter()
            .map(|diagnostic| {
                let message = diagnostic.message.unwrap_or_else(|| "invalid KDL".to_owned());
                let mut problem = Problem::new(message, diagnostic.span);
                if let Some(label) = diagnostic.label {
                    problem = problem.with_label(label);
                }
                if let Some(help) = diagnostic.help {
                    problem = problem.with_help(help);
                }
                problem
            })
            .collect()
    })
}

/// Reads the arguments and properties of a single KDL node.
///
/// Each accessor returns the requested value, or `None` when the value is missing, has the
/// wrong type or is out of range. In that case the reason is recorded as a [`Problem`].
/// Once all expected values have been read, call [`NodeReader::finish`] to report anything
/// else found on the node, such as a misspelled property.
pub struct NodeReader<'n, 'p> {
    node: &'n KdlNode,
    problems: &'p mut Vec<Problem>,
    used: Vec<bool>,
    known_properties: Vec<String>,
    block_read: bool,
}

impl<'n, 'p> NodeReader<'n, 'p> {
    /// Creates a reader for `node` that records problems in `problems`.
    pub fn new(node: &'n KdlNode, problems: &'p mut Vec<Problem>) -> Self {
        Self {
            node,
            problems,
            used: vec![false; node.entries().len()],
            known_properties: Vec::new(),
            block_read: false,
        }
    }

    /// Returns the node's name, for example `device`.
    #[must_use]
    pub fn name(&self) -> &'n str {
        self.node.name().value()
    }

    /// Returns the location of the whole node in the source text.
    #[must_use]
    pub fn span(&self) -> SourceSpan {
        self.node.span()
    }

    /// Reads the argument at `index` as a string.
    ///
    /// `what` names the argument in error messages, for example `"name"`.
    pub fn arg_str(&mut self, index: usize, what: &str) -> Option<String> {
        let entry = self.argument(index, what)?;
        self.string_value(entry, what)
    }

    /// Reads the argument at `index` as a whole number of type `T`.
    pub fn arg_int<T: TryFrom<i128>>(&mut self, index: usize, what: &str) -> Option<T> {
        let entry = self.argument(index, what)?;
        self.int_value(entry, what)
    }

    /// Reads the argument at `index` as one of the values of the enum `T`, written by name,
    /// for example `kind "server"`.
    pub fn arg_enum<T: FromStr + VariantNames>(&mut self, index: usize, what: &str) -> Option<T> {
        let entry = self.argument(index, what)?;
        self.enum_value(entry, what)
    }

    /// Reads a string property that must be present.
    pub fn req_str(&mut self, key: &str) -> Option<String> {
        let entry = self.required(key)?;
        self.string_value(entry, key)
    }

    /// Reads a whole-number property of type `T` that must be present.
    pub fn req_int<T: TryFrom<i128>>(&mut self, key: &str) -> Option<T> {
        let entry = self.required(key)?;
        self.int_value(entry, key)
    }

    /// Reads an optional string property.
    pub fn opt_str(&mut self, key: &str) -> Option<String> {
        let entry = self.property(key)?;
        self.string_value(entry, key)
    }

    /// Reads an optional whole-number property of type `T`.
    pub fn opt_int<T: TryFrom<i128>>(&mut self, key: &str) -> Option<T> {
        let entry = self.property(key)?;
        self.int_value(entry, key)
    }

    /// Reads an optional property holding one of the values of the enum `T`, written by
    /// name, for example `face="rear"`.
    pub fn opt_enum<T: FromStr + VariantNames>(&mut self, key: &str) -> Option<T> {
        let entry = self.property(key)?;
        self.enum_value(entry, key)
    }

    /// Returns the node's block of child nodes, if it has one.
    ///
    /// Only nodes whose block is read this way may have one; [`NodeReader::finish`]
    /// reports a block on any other node.
    pub fn children(&mut self) -> Option<&'n KdlDocument> {
        self.block_read = true;
        self.node.children()
    }

    /// Reports every argument and property that none of the accessors read, and a block
    /// of child nodes that was not expected.
    pub fn finish(self) {
        if let Some(block) = self.node.children().filter(|_| !self.block_read) {
            let problem = Problem::new(
                format!("`{}` does not take a block of child nodes", self.name()),
                block.span(),
            )
            .with_label("not expected");
            self.problems.push(problem);
        }
        for (entry, _) in self.node.entries().iter().zip(&self.used).filter(|(_, used)| !**used) {
            let problem = match entry.name() {
                Some(name) => {
                    let name = name.value();
                    let problem = Problem::new(
                        format!("unknown property `{name}` on `{}`", self.name()),
                        entry.span(),
                    )
                    .with_label("unknown property");
                    match closest(name, self.known_properties.iter().map(String::as_str)) {
                        Some(close) => problem.with_help(format!("did you mean `{close}`?")),
                        None => problem,
                    }
                }
                None => {
                    Problem::new(format!("unexpected argument on `{}`", self.name()), entry.span())
                        .with_label("not expected")
                }
            };
            self.problems.push(problem);
        }
    }

    /// Returns the argument at `index`, counting arguments only, or reports that it is
    /// missing.
    fn argument(&mut self, index: usize, what: &str) -> Option<&'n KdlEntry> {
        let position = self
            .node
            .entries()
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.name().is_none())
            .nth(index)
            .map(|(position, _)| position);
        let Some(position) = position else {
            let problem = Problem::new(
                format!("`{}` is missing its {what}", self.name()),
                self.node.name().span(),
            )
            .with_label(format!("expected {what} after this"));
            self.problems.push(problem);
            return None;
        };
        self.used[position] = true;
        Some(&self.node.entries()[position])
    }

    /// Returns the property `key`.
    ///
    /// When a property appears more than once, the last value is used, as the KDL
    /// specification requires, and the repetition is reported once.
    fn property(&mut self, key: &str) -> Option<&'n KdlEntry> {
        self.known_properties.push(key.to_owned());
        let mut found = Vec::new();
        for (position, entry) in self.node.entries().iter().enumerate() {
            if entry.name().is_some_and(|name| name.value() == key) {
                self.used[position] = true;
                found.push(entry);
            }
        }
        if let [first, second, rest @ ..] = found.as_slice() {
            let mut problem =
                Problem::new(format!("`{key}` is given more than once"), second.span())
                    .with_label("given again here")
                    .with_label_at(first.span(), "first given here");
            for entry in rest {
                problem = problem.with_label_at(entry.span(), "given again here");
            }
            self.problems.push(problem);
        }
        found.last().copied()
    }

    /// Returns whether the node has a property whose name looks like a misspelling of `key`,
    /// such as `modle` for `model`. [`NodeReader::finish`] reports that property with a
    /// suggestion, so a missing `key` need not be reported as well.
    #[must_use]
    pub fn has_misspelling_of(&self, key: &str) -> bool {
        self.node
            .entries()
            .iter()
            .filter_map(KdlEntry::name)
            .any(|name| name.value() != key && closest(name.value(), [key].into_iter()).is_some())
    }

    /// Returns the property `key`, or reports that it is missing.
    fn required(&mut self, key: &str) -> Option<&'n KdlEntry> {
        let entry = self.property(key);
        if entry.is_none() && !self.has_misspelling_of(key) {
            let problem = Problem::new(
                format!("`{}` is missing the property `{key}`", self.name()),
                self.node.name().span(),
            )
            .with_label(format!("add {key}=..."));
            self.problems.push(problem);
        }
        entry
    }

    fn string_value(&mut self, entry: &KdlEntry, what: &str) -> Option<String> {
        if let KdlValue::String(value) = entry.value() {
            return Some(value.clone());
        }
        let problem = Problem::new(
            format!("`{what}` must be a string, found {}", describe(entry.value())),
            entry.span(),
        );
        self.problems.push(problem);
        None
    }

    fn enum_value<T: FromStr + VariantNames>(&mut self, entry: &KdlEntry, what: &str) -> Option<T> {
        let value = self.string_value(entry, what)?;
        if let Ok(parsed) = value.parse() {
            return Some(parsed);
        }
        let choices = format!("`{}`", T::VARIANTS.join("`, `"));
        let mut problem = Problem::new(
            format!("`{what}` must be one of {choices}, found `{value}`"),
            entry.span(),
        );
        if let Some(close) = closest(&value, T::VARIANTS.iter().copied()) {
            problem = problem.with_help(format!("did you mean `{close}`?"));
        }
        self.problems.push(problem);
        None
    }

    fn int_value<T: TryFrom<i128>>(&mut self, entry: &KdlEntry, what: &str) -> Option<T> {
        let problem = match entry.value() {
            KdlValue::Integer(value) => match T::try_from(*value) {
                Ok(value) => return Some(value),
                Err(_) => Problem::new(format!("`{what}` is out of range: {value}"), entry.span()),
            },
            other => Problem::new(
                format!("`{what}` must be a whole number, found {}", describe(other)),
                entry.span(),
            ),
        };
        self.problems.push(problem);
        None
    }
}

/// Describes a value for use in messages, for example `the string "30"`.
fn describe(value: &KdlValue) -> String {
    match value {
        KdlValue::String(text) => format!("the string \"{text}\""),
        KdlValue::Integer(number) => format!("the number {number}"),
        KdlValue::Float(number) => format!("the number {number:?}"),
        KdlValue::Bool(flag) => format!("#{flag}"),
        KdlValue::Null => "#null".to_owned(),
    }
}

/// Returns the candidate most similar to `word`, if it is similar enough to be a likely
/// typo. A candidate that differs only in letter case, such as `u` for `U`, always is.
pub(crate) fn closest<'c>(
    word: &str,
    candidates: impl Iterator<Item = &'c str>,
) -> Option<&'c str> {
    candidates
        .map(|candidate| {
            let distance = if word.eq_ignore_ascii_case(candidate) {
                0
            } else {
                edit_distance(word, candidate)
            };
            (distance, candidate)
        })
        .filter(|&(distance, candidate)| distance <= 2 && distance < candidate.len())
        .min_by_key(|&(distance, _)| distance)
        .map(|(_, candidate)| candidate)
}

/// Counts the single-character insertions, deletions and substitutions needed to turn `a`
/// into `b`, also known as the Levenshtein distance.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut current = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(ca != *cb);
            current.push(substitution.min(previous[j + 1] + 1).min(current[j] + 1));
        }
        previous = current;
    }
    previous[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A side of the rack, used to test reading enums.
    #[derive(Debug, PartialEq, Eq, strum::EnumString, strum::VariantNames)]
    #[strum(serialize_all = "kebab-case")]
    enum Face {
        Front,
        Rear,
    }

    /// Parses `text`, reads its first node with `read` and returns the problems recorded.
    fn problems_for(text: &str, read: impl FnOnce(&mut NodeReader<'_, '_>)) -> Vec<Problem> {
        let document = parse(text).expect("valid KDL");
        let mut problems = Vec::new();
        let mut reader = NodeReader::new(&document.nodes()[0], &mut problems);
        read(&mut reader);
        reader.finish();
        problems
    }

    fn messages(problems: &[Problem]) -> Vec<&str> {
        problems.iter().map(Problem::message).collect()
    }

    #[test]
    fn reads_arguments_and_properties() {
        let problems =
            problems_for(r#"device "srv01" model="dell/r630-sff8" u=30 face="rear""#, |node| {
                assert_eq!(node.name(), "device");
                assert_eq!(node.arg_str(0, "name").as_deref(), Some("srv01"));
                assert_eq!(node.req_str("model").as_deref(), Some("dell/r630-sff8"));
                assert_eq!(node.opt_int::<u8>("u"), Some(30));
                assert_eq!(node.opt_enum("face"), Some(Face::Rear));
                assert_eq!(node.opt_str("mount"), None);
            });
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn reports_a_missing_argument_and_property() {
        let problems = problems_for("device u=1", |node| {
            assert_eq!(node.arg_str(0, "name"), None);
            assert_eq!(node.req_str("model"), None);
            assert_eq!(node.req_int::<u8>("height"), None);
            assert_eq!(node.req_int::<u8>("u"), Some(1));
        });
        assert_eq!(
            messages(&problems),
            [
                "`device` is missing its name",
                "`device` is missing the property `model`",
                "`device` is missing the property `height`",
            ]
        );
    }

    #[test]
    fn reports_wrong_types_and_ranges() {
        let problems = problems_for(r#"device "a" u="30" height=300 model=1 depth=1.0"#, |node| {
            let _ = node.arg_str(0, "name");
            assert_eq!(node.opt_int::<u8>("u"), None);
            assert_eq!(node.opt_int::<u8>("height"), None);
            assert_eq!(node.opt_str("model"), None);
            assert_eq!(node.opt_int::<u8>("depth"), None);
        });
        assert_eq!(
            messages(&problems),
            [
                "`u` must be a whole number, found the string \"30\"",
                "`height` is out of range: 300",
                "`model` must be a string, found the number 1",
                "`depth` must be a whole number, found the number 1.0",
            ]
        );
    }

    #[test]
    fn reports_an_invalid_enum_value_with_a_suggestion() {
        let problems = problems_for(r#"device "a" face="raer""#, |node| {
            let _ = node.arg_str(0, "name");
            assert_eq!(node.opt_enum::<Face>("face"), None);
        });
        assert_eq!(messages(&problems), ["`face` must be one of `front`, `rear`, found `raer`"]);
        assert_eq!(problems[0].help(), Some("did you mean `rear`?"));
    }

    #[test]
    fn reports_unknown_properties_and_arguments() {
        let problems = problems_for(r#"device "a" "b" hieght=2 colour="red""#, |node| {
            let _ = node.arg_str(0, "name");
            let _ = node.opt_int::<u8>("height");
        });
        assert_eq!(
            messages(&problems),
            [
                "unexpected argument on `device`",
                "unknown property `hieght` on `device`",
                "unknown property `colour` on `device`",
            ]
        );
        assert_eq!(problems[1].help(), Some("did you mean `height`?"));
        assert_eq!(problems[2].help(), None);
    }

    #[test]
    fn uses_the_last_of_repeated_properties_and_reports_it_once() {
        let problems = problems_for(r#"device "a" u=1 u=2 u=3"#, |node| {
            let _ = node.arg_str(0, "name");
            assert_eq!(node.opt_int::<u8>("u"), Some(3));
        });
        assert_eq!(messages(&problems), ["`u` is given more than once"]);
        let labels: Vec<_> = problems[0]
            .labels()
            .expect("labels")
            .map(|label| label.label().unwrap_or_default().to_owned())
            .collect();
        assert_eq!(labels, ["given again here", "first given here", "given again here"]);
    }

    #[test]
    fn reports_a_misspelled_required_property_only_once() {
        let problems = problems_for(r#"device "a" modle="x/y""#, |node| {
            let _ = node.arg_str(0, "name");
            assert_eq!(node.req_str("model"), None);
        });
        assert_eq!(messages(&problems), ["unknown property `modle` on `device`"]);
        assert_eq!(problems[0].help(), Some("did you mean `model`?"));
    }

    #[test]
    fn suggests_names_that_differ_only_in_case() {
        let problems = problems_for(r#"device "a" U=1"#, |node| {
            let _ = node.arg_str(0, "name");
            let _ = node.opt_int::<u8>("u");
        });
        assert_eq!(problems[0].help(), Some("did you mean `u`?"));
    }

    #[test]
    fn reports_syntax_errors() {
        let problems = parse(r#"device "unterminated"#).expect_err("invalid KDL");
        assert!(!problems.is_empty());
    }

    #[test]
    fn renders_all_problems_with_the_file_source() {
        let text = "device \"srv01\" model=\"dell/r630-sff8\" hieght=2\n";
        let problems = problems_for(text, |node| {
            let _ = node.arg_str(0, "name");
            let _ = node.req_str("model");
            let _ = node.opt_int::<u8>("height");
        });
        let error = FileError::new("rack.kdl", text, problems);
        assert_eq!(error.to_string(), "rack.kdl: 1 problem");

        let mut report = String::new();
        miette::GraphicalReportHandler::new_themed(miette::GraphicalTheme::unicode_nocolor())
            .render_report(&mut report, &error)
            .expect("rendering succeeds");
        assert!(report.contains("unknown property `hieght` on `device`"), "{report}");
        assert!(report.contains("rack.kdl:1:39"), "{report}");
        assert!(report.contains("did you mean `height`?"), "{report}");
    }

    #[test]
    fn measures_edit_distance() {
        assert_eq!(edit_distance("hieght", "height"), 2);
        assert_eq!(edit_distance("face", "face"), 0);
        assert_eq!(edit_distance("", "abc"), 3);
    }
}
