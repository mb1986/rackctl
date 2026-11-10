//! Configuration loading: the files of a configuration and what they hold.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use kdl::KdlNode;
use miette::SourceSpan;

use crate::catalog::Catalog;
use crate::kdl_reader::{self, FileError, FileId, Problem, span_of};
use crate::rack::{self, Layout, Rack};
use crate::wiring::{self, Cabling, Wiring};

/// A checked configuration.
#[derive(Debug)]
pub struct Config {
    /// The files read. A [`FileId`] is a position among them.
    pub files: Vec<PathBuf>,
    /// The racks.
    pub layout: Layout,
    /// The cables, or `None` when no file has a `wiring` node.
    pub cabling: Option<Cabling>,
}

/// A file being loaded, with the problems found in it.
struct File {
    name: String,
    text: String,
    problems: Vec<Problem>,
}

/// A place in one of the files.
#[derive(Clone, Copy)]
struct Place {
    file: FileId,
    span: SourceSpan,
}

/// What the files hold, gathered while they are read.
#[derive(Default)]
struct Found {
    racks: Vec<Rack>,
    /// Where each rack name is first used.
    rack_names: HashMap<String, Place>,
    /// The wiring, with the place of its node.
    wiring: Option<(Wiring, Place)>,
}

/// Loads the configuration at `path`, a directory or a single file, and checks it against
/// `catalog`.
///
/// The files are those of [`find_files`], and each top-level node says what it holds: a
/// `rack` or the `wiring`. Problems in model files are reported by [`Catalog::invalid_models`].
///
/// # Errors
///
/// Returns the problems found, grouped by file.
pub fn load(path: &Path, catalog: &Catalog) -> Result<Config, Vec<FileError>> {
    let alone = |problem| vec![FileError::new(path.display().to_string(), "", vec![problem])];
    let paths = find_files(path).map_err(|error| alone(Problem::unreadable(&error)))?;
    if paths.is_empty() {
        return Err(alone(prb::no_files()));
    }

    let mut files = Vec::with_capacity(paths.len());
    let mut found = Found::default();
    for path in &paths {
        let file = read_file(path, &files, &mut found);
        files.push(file);
    }
    if has_problems(&files) {
        return Err(errors(files));
    }
    if found.racks.is_empty() {
        return Err(alone(prb::no_rack()));
    }

    let layout = Layout::new(found.racks);
    for rack in layout.racks() {
        files[rack.file.0].problems.extend(rack.check(catalog));
    }
    if has_problems(&files) {
        return Err(errors(files));
    }

    // The wiring names the racks' devices, so it is resolved only against valid racks.
    let cabling = match (found.wiring, layout.racks()) {
        (None, _) => None,
        (Some((wiring, _)), [rack]) => match wiring.resolve(rack, catalog) {
            Ok(cabling) => Some(cabling),
            Err(problems) => {
                files[wiring.file.0].problems = problems;
                return Err(errors(files));
            }
        },
        (Some((_, place)), _) => {
            files[place.file.0].problems.push(prb::wiring_with_several_racks(place.span));
            return Err(errors(files));
        }
    };
    Ok(Config { files: paths, layout, cabling })
}

/// Reads the file at `path` and gathers what it holds into `found`. `earlier` are the files
/// read before it.
fn read_file(path: &Path, earlier: &[File], found: &mut Found) -> File {
    let id = FileId(earlier.len());
    let mut file =
        File { name: path.display().to_string(), text: String::new(), problems: Vec::new() };
    match fs::read_to_string(path) {
        Ok(text) => file.text = text,
        Err(error) => {
            file.problems.push(Problem::unreadable(&error));
            return file;
        }
    }
    let document = match kdl_reader::parse(&file.text) {
        Ok(document) => document,
        Err(problems) => {
            file.problems = problems;
            return file;
        }
    };
    for node in document.nodes() {
        match node.name().value() {
            "rack" => {
                file.problems.extend(repeated_rack(node, id, earlier, found));
                found.racks.extend(rack::read_rack(node, id, &mut file.problems));
            }
            "wiring" => {
                if let Some((_, place)) = &found.wiring {
                    file.problems.push(prb::second_wiring(node, &first(*place, id, earlier)));
                } else {
                    let wiring = wiring::read_wiring(node, id, &mut file.problems);
                    found.wiring = Some((wiring, Place { file: id, span: node.name().span() }));
                }
            }
            _ => file.problems.push(prb::unknown_top_level(node)),
        }
    }
    file
}

/// Reports the name of the `rack` node when a rack before it has the same one. The node is
/// in the file `id`, read after `earlier`.
fn repeated_rack(
    node: &KdlNode,
    id: FileId,
    earlier: &[File],
    found: &mut Found,
) -> Option<Problem> {
    let name = node.entry(0)?.value().as_string()?;
    let place = Place { file: id, span: span_of(node, 0) };
    match found.rack_names.entry(name.to_owned()) {
        Entry::Occupied(used) => {
            Some(prb::repeated_rack(name, place.span, &first(*used.get(), id, earlier)))
        }
        Entry::Vacant(slot) => {
            slot.insert(place);
            None
        }
    }
}

/// Describes `place`, where something was first written, for a problem in the file `id`,
/// read after `earlier`.
fn first(place: Place, id: FileId, earlier: &[File]) -> prb::First<'_> {
    if place.file == id {
        return prb::First::Here(place.span);
    }
    let file = &earlier[place.file.0];
    let line = file.text[..place.span.offset()].matches('\n').count() + 1;
    prb::First::In { file: &file.name, line }
}

fn has_problems(files: &[File]) -> bool {
    files.iter().any(|file| !file.problems.is_empty())
}

/// Returns the problems of each file that has some.
fn errors(files: Vec<File>) -> Vec<FileError> {
    files
        .into_iter()
        .filter(|file| !file.problems.is_empty())
        .map(|file| FileError::new(file.name, file.text, file.problems))
        .collect()
}

/// Folders at the top of a configuration directory that hold models and profiles.
const DATA_DIRS: &[&str] = &["catalog", "profiles"];

/// Returns the configuration files at `path`, sorted: the file itself, or every `.kdl` file
/// of the directory and its subdirectories.
///
/// Hidden entries, links to directories and the top-level [`DATA_DIRS`] are skipped.
///
/// # Errors
///
/// Returns the error of a file or directory that cannot be read.
pub fn find_files(path: &Path) -> io::Result<Vec<PathBuf>> {
    if !fs::metadata(path)?.is_dir() {
        return Ok(vec![path.to_owned()]);
    }
    let mut files = Vec::new();
    add_files(path, true, &mut files)?;
    files.sort();
    Ok(files)
}

/// Adds the configuration files in `dir` and its subdirectories. `top` tells whether `dir`
/// is the configuration directory itself.
fn add_files(dir: &Path, top: bool, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        if entry.file_type()?.is_dir() {
            if !(top && DATA_DIRS.contains(&name.as_ref())) {
                add_files(&path, false, files)?;
            }
        } else if path.extension().is_some_and(|extension| extension == "kdl") {
            files.push(path);
        }
    }
    Ok(())
}

/// Loads the racks of the configuration file at `path` and checks them against `catalog`.
///
/// Problems in model files are reported by [`Catalog::invalid_models`].
///
/// # Errors
///
/// Returns the problems found in the file.
pub fn load_layout(path: &Path, catalog: &Catalog) -> Result<Layout, FileError> {
    let name = path.display().to_string();
    let text = fs::read_to_string(path).map_err(|error| FileError::unreadable(&name, &error))?;
    let layout = Layout::parse(&text).map_err(|problems| FileError::new(&name, &text, problems))?;
    let problems = layout.check(catalog);
    if problems.is_empty() { Ok(layout) } else { Err(FileError::new(name, text, problems)) }
}

/// Loads the rack file at `path` and checks it against `catalog`.
///
/// Problems in model files are reported by [`Catalog::invalid_models`].
///
/// # Errors
///
/// Returns the problems found in the rack file.
pub fn load_rack(path: &Path, catalog: &Catalog) -> Result<Rack, FileError> {
    let name = path.display().to_string();
    let text = fs::read_to_string(path).map_err(|error| FileError::unreadable(&name, &error))?;
    let rack = Rack::parse(&text).map_err(|problems| FileError::new(&name, &text, problems))?;
    let problems = rack.check(catalog);
    if problems.is_empty() { Ok(rack) } else { Err(FileError::new(name, text, problems)) }
}

/// Loads the wiring file at `path` and resolves it against `rack`, whose models come from
/// `catalog`. The wiring is optional: without a file, there is no cabling.
///
/// # Errors
///
/// Returns the problems found in the wiring file.
pub fn load_wiring(
    path: &Path,
    rack: &Rack,
    catalog: &Catalog,
) -> Result<Option<Cabling>, FileError> {
    let name = path.display().to_string();
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(FileError::unreadable(&name, &error)),
    };
    let wiring = Wiring::parse(&text).map_err(|problems| FileError::new(&name, &text, problems))?;
    let cabling =
        wiring.resolve(rack, catalog).map_err(|problems| FileError::new(&name, &text, problems))?;
    Ok(Some(cabling))
}

/// The problems reported for a configuration as a whole.
mod prb {
    use kdl::KdlNode;
    use miette::SourceSpan;

    use crate::kdl_reader::Problem;

    /// The top-level nodes of a configuration.
    const NODES: &[&str] = &["rack", "wiring"];

    /// Where something was first written.
    pub(super) enum First<'a> {
        /// In the same file as the problem.
        Here(SourceSpan),
        /// In another file.
        In { file: &'a str, line: usize },
    }

    impl First<'_> {
        /// Adds the place to `problem`. `what` says what happened there, such as `first used`.
        fn note(&self, problem: Problem, what: &str) -> Problem {
            match self {
                Self::Here(span) => problem.with_label_at(*span, format!("{what} here")),
                Self::In { file, line } => {
                    problem.with_help(format!("{what} in {file}, line {line}"))
                }
            }
        }
    }

    pub(super) fn no_files() -> Problem {
        Problem::new("there are no `.kdl` files in the directory", SourceSpan::from(0..0))
    }

    pub(super) fn no_rack() -> Problem {
        Problem::new("the configuration has no `rack { ... }` node", SourceSpan::from(0..0))
    }

    pub(super) fn unknown_top_level(node: &KdlNode) -> Problem {
        let key = node.name().value();
        Problem::new(format!("unknown top-level node `{key}`"), node.name().span())
            .with_help("a configuration holds `rack` and `wiring` nodes")
            .with_suggestion(key, NODES.iter().copied())
    }

    pub(super) fn repeated_rack(name: &str, span: SourceSpan, first: &First) -> Problem {
        let problem = Problem::new(format!("the rack name `{name}` is used more than once"), span)
            .with_label("used again here");
        first.note(problem, "first used")
    }

    pub(super) fn second_wiring(node: &KdlNode, first: &First) -> Problem {
        let span = node.name().span();
        let problem = Problem::new("the configuration has more than one `wiring` node", span)
            .with_label("given again here");
        first.note(problem, "first given")
    }

    pub(super) fn wiring_with_several_racks(span: SourceSpan) -> Problem {
        Problem::new("`wiring` is not supported with several racks yet", span)
    }
}

#[cfg(test)]
mod tests {
    use miette::Diagnostic;

    use super::*;
    use crate::testing::dir_with;

    const SERVER: &str = r#"model { name "Server"; kind "server" }"#;

    /// Finds the files at `path` and names them relative to `root`.
    fn found(root: &Path, path: &Path) -> Vec<String> {
        let files = find_files(path).expect("readable path");
        let name = |file: &PathBuf| {
            file.strip_prefix(root).expect("inside the root").display().to_string()
        };
        files.iter().map(name).collect()
    }

    #[test]
    fn finds_the_kdl_files_of_a_directory_sorted() {
        let dir = dir_with(&[
            ("wiring.kdl", ""),
            ("rack.kdl", ""),
            ("rack.kdl.draft1", ""),
            ("notes.md", ""),
            ("site/b/rack.kdl", ""),
            ("site/access.kdl", ""),
        ]);
        assert_eq!(
            found(dir.path(), dir.path()),
            ["rack.kdl", "site/access.kdl", "site/b/rack.kdl", "wiring.kdl"]
        );
    }

    #[test]
    fn finds_a_single_file_whatever_its_name() {
        let dir = dir_with(&[("all.conf", ""), ("rack.kdl", "")]);
        assert_eq!(found(dir.path(), &dir.path().join("all.conf")), ["all.conf"]);
    }

    #[test]
    fn skips_models_profiles_and_hidden_entries() {
        let dir = dir_with(&[
            ("rack.kdl", ""),
            ("catalog/x/server.kdl", ""),
            ("profiles/x/server.kdl", ""),
            ("site/catalog/rack.kdl", ""),
            (".git/rack.kdl", ""),
            (".#rack.kdl", ""),
        ]);
        assert_eq!(found(dir.path(), dir.path()), ["rack.kdl", "site/catalog/rack.kdl"]);
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_links_to_directories() {
        let dir = dir_with(&[("rack.kdl", "")]);
        std::os::unix::fs::symlink(dir.path(), dir.path().join("loop")).expect("symlink");
        assert_eq!(found(dir.path(), dir.path()), ["rack.kdl"]);
    }

    #[test]
    fn reports_a_path_that_cannot_be_read() {
        let dir = dir_with(&[]);
        let error = find_files(&dir.path().join("missing")).expect_err("missing");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    const NIC_SERVER: &str = r#"model { name "Server"; kind "server"; nics 1 }"#;

    /// Two servers in the rack `r`.
    const RACK: &str = r#"rack "r" units=4 {
        device "a" model="x/server" u=1
        device "b" model="x/server" u=2
    }"#;

    type Loaded = Result<Config, Vec<FileError>>;

    /// Loads `path` in a directory holding `files` and a catalog with the model `x/server`.
    fn load_in(files: &[(&str, &str)], path: &str) -> (tempfile::TempDir, Loaded) {
        let mut all = vec![("catalog/x/server.kdl", NIC_SERVER)];
        all.extend_from_slice(files);
        let dir = dir_with(&all);
        let catalog = Catalog::open(&[dir.path().join("catalog")]).expect("readable catalog");
        let loaded = load(&dir.path().join(path), &catalog);
        (dir, loaded)
    }

    /// The messages of each file with problems, the files named relative to `dir`.
    fn reported(dir: &Path, loaded: Loaded) -> Vec<(String, Vec<String>)> {
        let name = |error: &FileError| {
            let path = Path::new(error.name());
            path.strip_prefix(dir).unwrap_or(path).display().to_string()
        };
        let messages = |error: &FileError| {
            error.problems().iter().map(|problem| problem.message().to_owned()).collect()
        };
        let errors = loaded.expect_err("invalid configuration");
        errors.iter().map(|error| (name(error), messages(error))).collect()
    }

    fn in_file(file: &str, messages: &[&str]) -> (String, Vec<String>) {
        (file.to_owned(), messages.iter().map(|&message| message.to_owned()).collect())
    }

    #[test]
    fn loads_one_file_holding_everything() {
        let text = format!("{RACK}\nwiring {{ net a:nic1 b:nic1; }}");
        let (dir, loaded) = load_in(&[("all.kdl", &text)], "all.kdl");
        let config = loaded.expect("valid configuration");

        assert_eq!(config.files, [dir.path().join("all.kdl")]);
        assert_eq!(config.layout.racks().len(), 1);
        assert_eq!(config.cabling.expect("cabling").links().len(), 1);
    }

    #[test]
    fn merges_the_files_of_a_directory() {
        let (_dir, loaded) = load_in(
            &[("rack.kdl", RACK), ("site/wiring.kdl", "wiring { net a:nic1 b:nic1; }")],
            "",
        );
        let config = loaded.expect("valid configuration");

        assert_eq!(config.files.len(), 2);
        assert_eq!(config.cabling.expect("cabling").links().len(), 1);
    }

    #[test]
    fn remembers_the_file_of_each_rack() {
        let (_dir, loaded) = load_in(
            &[("b.kdl", r#"rack "a" units=4"#), ("a.kdl", r#"rack "b" units=4; rack "c" units=4"#)],
            "",
        );
        let config = loaded.expect("valid configuration");

        let racks: Vec<_> =
            config.layout.racks().iter().map(|rack| (rack.name.as_str(), rack.file)).collect();
        assert_eq!(racks, [("a", FileId(1)), ("b", FileId(0)), ("c", FileId(0))]);
        assert!(config.cabling.is_none());
    }

    #[test]
    fn reports_each_problem_in_its_file() {
        let (dir, loaded) = load_in(
            &[
                ("a.kdl", r#"rack "a" units=4 { device "s" model="x/nope" u=1 }"#),
                ("b.kdl", r#"rack "b" units=4"#),
                (
                    "c.kdl",
                    r#"rack "c" units=4 {
                        device "s" model="x/server" u=1
                        device "t" model="x/server" u=1
                    }"#,
                ),
            ],
            "",
        );
        assert_eq!(
            reported(dir.path(), loaded),
            [
                in_file("a.kdl", &["unknown model `x/nope`"]),
                in_file("c.kdl", &["`t` overlaps `s` on U1"]),
            ]
        );
    }

    #[test]
    fn reads_the_other_files_when_one_cannot_be_parsed() {
        let (dir, loaded) =
            load_in(&[("a.kdl", r#"rack "a" units=4 {"#), ("b.kdl", r#"rack "b" units=99"#)], "");
        let reported = reported(dir.path(), loaded);
        assert_eq!(reported.len(), 2);
        assert_eq!(reported[0].0, "a.kdl");
        assert_eq!(reported[1], in_file("b.kdl", &["`units` must be between 1 and 60"]));
    }

    #[test]
    fn reports_a_rack_name_used_twice_in_a_file() {
        let (_dir, loaded) = load_in(&[("a.kdl", r#"rack "r" units=4; rack "r" units=8"#)], "");
        let errors = loaded.expect_err("repeated name");
        let problem = &errors[0].problems()[0];
        assert_eq!(problem.message(), "the rack name `r` is used more than once");
        assert_eq!(problem.labels().expect("labels").count(), 2);
        assert_eq!(problem.help(), None);
    }

    #[test]
    fn reports_a_rack_name_used_in_two_files() {
        let (dir, loaded) = load_in(
            &[("a.kdl", "// the first\nrack \"r\" units=4"), ("b.kdl", r#"rack "r" units=8"#)],
            "",
        );
        let errors = loaded.expect_err("repeated name");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].name(), dir.path().join("b.kdl").display().to_string());
        let problem = &errors[0].problems()[0];
        assert_eq!(problem.message(), "the rack name `r` is used more than once");
        let first = dir.path().join("a.kdl");
        assert_eq!(problem.help(), Some(&*format!("first used in {}, line 2", first.display())));
    }

    #[test]
    fn reports_unknown_top_level_nodes() {
        let text = format!("{RACK}\nwirng {{}}\ndevice \"x\"");
        let (_dir, loaded) = load_in(&[("all.kdl", &text)], "all.kdl");
        let errors = loaded.expect_err("unknown nodes");
        let problems: Vec<_> = errors[0]
            .problems()
            .iter()
            .map(|problem| (problem.message(), problem.help()))
            .collect();
        assert_eq!(
            problems,
            [
                ("unknown top-level node `wirng`", Some("did you mean `wiring`?")),
                (
                    "unknown top-level node `device`",
                    Some("a configuration holds `rack` and `wiring` nodes")
                ),
            ]
        );
    }

    #[test]
    fn needs_files_and_a_rack() {
        let (dir, loaded) = load_in(&[("wiring.kdl", "wiring {}")], "wiring.kdl");
        assert_eq!(
            reported(dir.path(), loaded),
            [in_file("wiring.kdl", &["the configuration has no `rack { ... }` node"])]
        );

        let (dir, loaded) = load_in(&[("empty/notes.md", "")], "empty");
        assert_eq!(
            reported(dir.path(), loaded),
            [in_file("empty", &["there are no `.kdl` files in the directory"])]
        );

        let (_dir, loaded) = load_in(&[], "missing");
        let errors = loaded.expect_err("missing");
        assert!(errors[0].problems()[0].message().starts_with("cannot read the file: "));
    }

    #[test]
    fn reports_the_problems_of_the_wiring_in_its_file() {
        let (dir, loaded) =
            load_in(&[("rack.kdl", RACK), ("wiring.kdl", "wiring { net a:nic1 b:nic2; }")], "");
        assert_eq!(
            reported(dir.path(), loaded),
            [in_file("wiring.kdl", &["`b:nic2` does not exist: `b` has NIC 1"])]
        );
    }

    #[test]
    fn reports_a_second_wiring_node() {
        let (dir, loaded) =
            load_in(&[("rack.kdl", RACK), ("w1.kdl", "wiring {}"), ("w2.kdl", "wiring {}")], "");
        let errors = loaded.expect_err("two wiring nodes");
        assert_eq!(errors[0].name(), dir.path().join("w2.kdl").display().to_string());
        let problem = &errors[0].problems()[0];
        assert_eq!(problem.message(), "the configuration has more than one `wiring` node");
        let first = dir.path().join("w1.kdl");
        assert_eq!(problem.help(), Some(&*format!("first given in {}, line 1", first.display())));
    }

    #[test]
    fn refuses_wiring_with_several_racks_for_now() {
        let (dir, loaded) = load_in(
            &[("racks.kdl", r#"rack "a" units=4; rack "b" units=4"#), ("w.kdl", "wiring {}")],
            "",
        );
        assert_eq!(
            reported(dir.path(), loaded),
            [in_file("w.kdl", &["`wiring` is not supported with several racks yet"])]
        );
    }

    #[test]
    fn loads_a_valid_rack() {
        let dir = dir_with(&[
            ("catalog/x/server.kdl", SERVER),
            ("rack.kdl", r#"rack "r" units=4 { device "a" model="x/server" u=1 }"#),
        ]);
        let catalog = Catalog::open(&[dir.path().join("catalog")]).expect("readable catalog");

        let rack = load_rack(&dir.path().join("rack.kdl"), &catalog).expect("valid rack");
        assert_eq!(rack.devices.len(), 1);
    }

    #[test]
    fn loads_every_rack_and_checks_each() {
        let dir = dir_with(&[
            ("catalog/x/server.kdl", SERVER),
            (
                "rack.kdl",
                r#"rack "a" units=4 { device "s" model="x/server" u=1 }
                   rack "b" units=4 { device "s" model="x/server" u=4 }"#,
            ),
        ]);
        let catalog = Catalog::open(&[dir.path().join("catalog")]).expect("readable catalog");
        let rack_file = dir.path().join("rack.kdl");

        let layout = load_layout(&rack_file, &catalog).expect("valid layout");
        assert_eq!(layout.racks().len(), 2);

        fs::write(
            &rack_file,
            r#"rack "a" units=4; rack "b" units=4 { device "s" model="x/s" u=1 }"#,
        )
        .expect("write the file");
        let error = load_layout(&rack_file, &catalog).expect_err("invalid");
        let messages: Vec<_> = error.problems().iter().map(Problem::message).collect();
        assert_eq!(messages, ["unknown model `x/s`"]);
    }

    #[test]
    fn reports_the_problems_of_the_rack_file() {
        let dir = dir_with(&[
            ("catalog/x/server.kdl", SERVER),
            ("catalog/x/broken.kdl", r#"model { name "Broken" }"#),
            (
                "rack.kdl",
                r#"rack "r" units=4 {
                    device "a" model="x/broken" u=1
                    device "b" model="x/server" u=1
                }"#,
            ),
        ]);
        let catalog = Catalog::open(&[dir.path().join("catalog")]).expect("readable catalog");

        let error = load_rack(&dir.path().join("rack.kdl"), &catalog).expect_err("invalid");
        let messages: Vec<_> = error.problems().iter().map(Problem::message).collect();
        assert_eq!(messages, ["the model `x/broken` has problems"]);
    }

    #[test]
    fn loads_the_wiring_when_there_is_one() {
        let dir = dir_with(&[
            ("catalog/x/server.kdl", r#"model { name "Server"; kind "server"; nics 1 }"#),
            (
                "rack.kdl",
                r#"rack "r" units=4 {
                    device "a" model="x/server" u=1
                    device "b" model="x/server" u=2
                }"#,
            ),
        ]);
        let catalog = Catalog::open(&[dir.path().join("catalog")]).expect("readable catalog");
        let rack = load_rack(&dir.path().join("rack.kdl"), &catalog).expect("valid rack");
        let wiring_file = dir.path().join("wiring.kdl");
        let load = || load_wiring(&wiring_file, &rack, &catalog);

        assert!(load().expect("no wiring").is_none());

        fs::write(&wiring_file, "wiring { net a:nic1 b:nic1; }").expect("write the wiring");
        assert_eq!(load().expect("valid wiring").expect("cabling").links().len(), 1);

        fs::write(&wiring_file, "wiring { net a:nic1 b:nic2; }").expect("write the wiring");
        let error = load().expect_err("invalid wiring");
        let messages: Vec<_> = error.problems().iter().map(Problem::message).collect();
        assert_eq!(messages, ["`b:nic2` does not exist: `b` has NIC 1"]);

        fs::remove_file(&wiring_file).expect("remove the wiring");
        fs::create_dir(&wiring_file).expect("a directory in its place");
        let error = load().expect_err("unreadable wiring");
        assert!(error.problems()[0].message().starts_with("cannot read the file: "));
    }

    #[test]
    fn reports_a_missing_file() {
        let dir = dir_with(&[]);
        let error =
            load_rack(&dir.path().join("rack.kdl"), &Catalog::default()).expect_err("missing");
        assert!(error.problems()[0].message().starts_with("cannot read the file: "));
    }
}
