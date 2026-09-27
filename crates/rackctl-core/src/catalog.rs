//! Device catalog: hardware models loaded from the built-in catalog and the
//! user's catalog directory.

mod model;

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use miette::SourceSpan;
use thiserror::Error;

pub use model::{Components, Depth, Ears, Kind, Model, Mount};

use crate::is_identifier;
use crate::kdl_reader::{FileError, Problem, closest};

/// The device catalog: every hardware model available to the racks.
///
/// Opening a catalog only lists the model files. A model is read the first time it is
/// requested through [`Catalog::model`] and kept for later requests, so each file is parsed
/// at most once and files that are never used are never parsed.
#[derive(Debug, Default)]
pub struct Catalog {
    entries: BTreeMap<String, Entry>,
}

/// One model file and, once it has been requested, the result of reading it.
#[derive(Debug)]
struct Entry {
    path: PathBuf,
    model: OnceLock<Result<Model, FileError>>,
}

/// The reason a model could not be provided.
#[derive(Debug, Error)]
pub enum ModelError<'a> {
    /// The catalog has no model with the requested identifier.
    #[error("unknown model `{id}`")]
    Unknown {
        /// The identifier that was requested.
        id: String,
        /// The most similar identifier in the catalog, if there is one.
        suggestion: Option<String>,
    },
    /// The model's file exists but contains problems.
    #[error(transparent)]
    Invalid(&'a FileError),
}

impl Catalog {
    /// Opens a catalog made of the model files found in `dirs` and their subdirectories.
    ///
    /// Directories are searched in order. A model in a later directory replaces the model
    /// with the same identifier in an earlier one, so a user's catalog can override the
    /// built-in catalog. Directories that do not exist are skipped.
    ///
    /// # Errors
    ///
    /// Returns an error if a directory exists but cannot be read.
    pub fn open(dirs: &[impl AsRef<Path>]) -> io::Result<Self> {
        let mut catalog = Self::default();
        for dir in dirs {
            let dir = dir.as_ref();
            match catalog.add_dir(dir, dir) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                result => result?,
            }
        }
        Ok(catalog)
    }

    /// Returns the identifiers of all models in the catalog, in alphabetical order.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Returns the model `id`, reading its file the first time it is requested.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError::Unknown`] if the catalog has no such model, and
    /// [`ModelError::Invalid`] if the model's file contains problems.
    pub fn model(&self, id: &str) -> Result<&Model, ModelError<'_>> {
        let Some(entry) = self.entries.get(id) else {
            let suggestion = closest(id, self.ids()).map(str::to_owned);
            return Err(ModelError::Unknown { id: id.to_owned(), suggestion });
        };
        entry.model.get_or_init(|| load(id, &entry.path)).as_ref().map_err(ModelError::Invalid)
    }

    /// Adds the model files in `dir` and its subdirectories, naming them relative to `root`.
    fn add_dir(&mut self, root: &Path, dir: &Path) -> io::Result<()> {
        let mut paths = fs::read_dir(dir)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<io::Result<Vec<_>>>()?;
        paths.sort();
        for path in paths {
            if path.is_dir() {
                self.add_dir(root, &path)?;
            } else if is_model_file(&path) {
                let id = model_id(root, &path);
                let model = if is_valid_id(&id) {
                    OnceLock::new()
                } else {
                    OnceLock::from(Err(invalid_name(&path)))
                };
                self.entries.insert(id, Entry { path, model });
            }
        }
        Ok(())
    }
}

/// Model files end in `.kdl`. Files starting with `_` hold shared parts of other models
/// and are not models themselves.
fn is_model_file(path: &Path) -> bool {
    let name = path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default();
    path.extension().is_some_and(|extension| extension == "kdl") && !name.starts_with('_')
}

/// Derives a model identifier from its path, for example `dell/r630-sff8` from
/// `<root>/dell/r630-sff8.kdl`. Components are always joined with `/`.
fn model_id(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path).with_extension("");
    relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// A model identifier is valid when every part is a valid identifier.
fn is_valid_id(id: &str) -> bool {
    id.split('/').all(is_identifier)
}

fn invalid_name(path: &Path) -> FileError {
    let problem = Problem::new(
        "model file names may only use lowercase letters, digits and `-`",
        SourceSpan::from(0..0),
    )
    .with_help("rename the file, for example `dell/r630-sff8.kdl`");
    FileError::new(path.display().to_string(), "", vec![problem])
}

/// Reads and parses one model file.
fn load(id: &str, path: &Path) -> Result<Model, FileError> {
    let name = path.display().to_string();
    let text = fs::read_to_string(path).map_err(|error| FileError::unreadable(&name, &error))?;
    Model::parse(id, &text).map_err(|problems| FileError::new(name, text, problems))
}

#[cfg(test)]
mod tests {
    use super::*;

    const R630: &str = r#"model { name "Dell PowerEdge R630"; short "R630"; kind "server" }"#;

    /// Creates a directory holding the given files, each written with its contents.
    fn dir_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temporary directory");
        for (path, text) in files {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().expect("file inside the directory")).expect("mkdir");
            fs::write(path, text).expect("write file");
        }
        dir
    }

    #[test]
    fn lists_models_and_loads_them_on_request() {
        let dir = dir_with(&[("dell/r630-sff8.kdl", R630), ("cisco/sg300-28.kdl", "broken {")]);
        let catalog = Catalog::open(&[dir.path()]).expect("readable directory");

        assert_eq!(catalog.ids().collect::<Vec<_>>(), ["cisco/sg300-28", "dell/r630-sff8"]);
        assert_eq!(catalog.model("dell/r630-sff8").expect("valid model").short, "R630");
        assert!(matches!(catalog.model("cisco/sg300-28"), Err(ModelError::Invalid(_))));
    }

    #[test]
    fn returns_the_same_model_on_every_request() {
        let dir = dir_with(&[("dell/r630-sff8.kdl", R630)]);
        let catalog = Catalog::open(&[dir.path()]).expect("readable directory");

        let first = catalog.model("dell/r630-sff8").expect("valid model");
        let second = catalog.model("dell/r630-sff8").expect("valid model");
        assert!(std::ptr::eq(first, second));
    }

    #[test]
    fn lets_a_later_directory_replace_a_model() {
        let builtin = dir_with(&[("dell/r630-sff8.kdl", R630)]);
        let user =
            dir_with(&[("dell/r630-sff8.kdl", r#"model { name "My R630"; kind "server" }"#)]);
        let catalog = Catalog::open(&[builtin.path(), user.path()]).expect("readable directories");

        assert_eq!(catalog.model("dell/r630-sff8").expect("valid model").name, "My R630");
    }

    #[test]
    fn skips_shared_parts_other_files_and_missing_directories() {
        let dir = dir_with(&[
            ("dell/r630-sff8.kdl", R630),
            ("dell/_poweredge-13g.kdl", "anything"),
            ("README.md", "notes"),
        ]);
        let missing = dir.path().join("missing");
        let catalog = Catalog::open(&[dir.path(), &missing]).expect("readable directory");

        assert_eq!(catalog.ids().collect::<Vec<_>>(), ["dell/r630-sff8"]);
    }

    #[test]
    fn suggests_a_similar_identifier() {
        let dir = dir_with(&[("dell/r630-sff8.kdl", R630)]);
        let catalog = Catalog::open(&[dir.path()]).expect("readable directory");

        match catalog.model("dell/r630-sf8") {
            Err(ModelError::Unknown { suggestion, .. }) => {
                assert_eq!(suggestion.as_deref(), Some("dell/r630-sff8"));
            }
            other => panic!("expected an unknown model, got {other:?}"),
        }
    }

    #[test]
    fn reports_invalid_file_names() {
        let dir = dir_with(&[("Dell/R630.kdl", R630)]);
        let catalog = Catalog::open(&[dir.path()]).expect("readable directory");

        let Err(ModelError::Invalid(error)) = catalog.model("Dell/R630") else {
            panic!("expected an invalid model");
        };
        assert_eq!(
            error.problems()[0].message(),
            "model file names may only use lowercase letters, digits and `-`"
        );
    }
}
