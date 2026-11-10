//! Configuration file loading: `rack.kdl` and `wiring.kdl`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::catalog::Catalog;
use crate::kdl_reader::FileError;
use crate::rack::{Layout, Rack};
use crate::wiring::{Cabling, Wiring};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kdl_reader::Problem;
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
