//! Configuration file loading: `rack.kdl`.

use std::fs;
use std::path::Path;

use crate::catalog::{Catalog, ModelError};
use crate::kdl_reader::FileError;
use crate::rack::Rack;

/// Loads the rack file at `path` and checks it against `catalog`.
///
/// # Errors
///
/// Returns one [`FileError`] for each file with problems: the rack file first, followed by
/// the files of any invalid models it uses.
pub fn load_rack(path: &Path, catalog: &Catalog) -> Result<Rack, Vec<FileError>> {
    let name = path.display().to_string();
    let text =
        fs::read_to_string(path).map_err(|error| vec![FileError::unreadable(&name, &error)])?;
    let rack =
        Rack::parse(&text).map_err(|problems| vec![FileError::new(&name, &text, problems)])?;
    let problems = rack.check(catalog);
    if problems.is_empty() {
        return Ok(rack);
    }

    let mut errors = vec![FileError::new(name, text, problems)];
    let mut reported = Vec::new();
    for device in &rack.devices {
        if let Err(ModelError::Invalid(error)) = catalog.model(&device.model)
            && !reported.contains(&&device.model)
        {
            reported.push(&device.model);
            errors.push(error.clone());
        }
    }
    Err(errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, path: &str, text: &str) {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().expect("file inside the directory")).expect("mkdir");
        fs::write(path, text).expect("write file");
    }

    fn names(errors: &[FileError]) -> Vec<String> {
        errors.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn loads_a_valid_rack() {
        let dir = tempfile::tempdir().expect("temporary directory");
        write(dir.path(), "catalog/x/server.kdl", r#"model { name "Server"; kind "server" }"#);
        write(dir.path(), "rack.kdl", r#"rack "r" units=4 { device "a" model="x/server" u=1 }"#);
        let catalog = Catalog::open(&[dir.path().join("catalog")]).expect("readable catalog");

        let rack = load_rack(&dir.path().join("rack.kdl"), &catalog).expect("valid rack");
        assert_eq!(rack.devices.len(), 1);
    }

    #[test]
    fn reports_the_rack_and_each_invalid_model_once() {
        let dir = tempfile::tempdir().expect("temporary directory");
        write(dir.path(), "catalog/x/broken.kdl", r#"model { name "Broken" }"#);
        write(
            dir.path(),
            "rack.kdl",
            r#"rack "r" units=4 {
                device "a" model="x/broken" u=1
                device "b" model="x/broken" u=2
            }"#,
        );
        let catalog = Catalog::open(&[dir.path().join("catalog")]).expect("readable catalog");

        let errors = load_rack(&dir.path().join("rack.kdl"), &catalog).expect_err("invalid");
        let rack = dir.path().join("rack.kdl").display().to_string();
        let model = dir.path().join("catalog/x/broken.kdl").display().to_string();
        assert_eq!(names(&errors), [format!("{rack}: 2 problems"), format!("{model}: 1 problem")]);
    }

    #[test]
    fn reports_a_missing_file() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let errors =
            load_rack(&dir.path().join("rack.kdl"), &Catalog::default()).expect_err("missing");
        assert!(errors[0].problems()[0].message().starts_with("cannot read the file: "));
    }
}
