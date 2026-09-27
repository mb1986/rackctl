//! Helpers shared by the unit tests.

use std::fs;

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
