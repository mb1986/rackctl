//! Configuration file loading: `rack.kdl` and `wiring.kdl`.

use std::fs;
use std::io;
use std::path::Path;

use crate::catalog::Catalog;
use crate::kdl_reader::FileError;
use crate::rack::Rack;
use crate::wiring::{Cabling, Wiring};

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
