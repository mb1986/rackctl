//! Embeds the built-in device catalog, the repository's `catalog/` folder, in the library.
//!
//! Writes `builtin_catalog.rs` to `OUT_DIR`: a list of every `.kdl` file in the folder, as
//! its path relative to the folder and its contents, embedded with `include_str!`. The
//! compiler tracks the embedded files, and Cargo reruns this script when files are added or
//! removed.

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn main() -> io::Result<()> {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("set by Cargo"));
    let root = manifest_dir.join("../../catalog").canonicalize()?;
    println!("cargo::rerun-if-changed={}", root.display());

    let mut files = Vec::new();
    collect(&root, &mut files)?;
    files.sort();

    let mut code = String::from("&[\n");
    for path in files {
        let relative = path
            .strip_prefix(&root)
            .expect("file inside the catalog")
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let absolute = path.to_string_lossy();
        writeln!(code, "    ({relative:?}, include_str!({absolute:?})),")
            .expect("writing to a String");
    }
    code.push_str("]\n");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("set by Cargo"));
    fs::write(out_dir.join("builtin_catalog.rs"), code)
}

/// Adds every `.kdl` file in `dir` and its subdirectories to `files`.
fn collect(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "kdl") {
            files.push(path);
        }
    }
    Ok(())
}
