//! Runs `rackctl rack` as a user would.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Runs `rackctl rack` with `args` on the configuration at `config`, a directory or a file,
/// without configuration from the environment.
fn rack(config: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rackctl"))
        .arg("-c")
        .arg(config)
        .arg("rack")
        .args(args)
        .env("NO_COLOR", "1")
        .env("HOME", "/nonexistent-home")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("RACKCTL_CONFIG")
        .output()
        .expect("rackctl runs")
}

fn golden(name: &str) -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "../../tests/golden/rack", name].iter().collect()
}

#[test]
fn draws_the_rack_as_in_the_golden_file() {
    let expected = fs::read_to_string(golden("rack.txt")).expect("golden file");
    for args in [&["--plain"][..], &[]] {
        let output = rack(&golden(""), args);
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected, "{args:?}");
    }
}

#[test]
fn reports_a_rack_with_problems_without_drawing_it() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let rack_file = dir.path().join("rack.kdl");
    fs::write(&rack_file, r#"rack "lab" units=4 { device "a" model="nope/x" u=1; }"#)
        .expect("write the rack file");
    let output = rack(&rack_file, &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown model `nope/x`"));
}

#[test]
fn reports_a_wiring_with_problems_without_drawing_the_rack() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let rack_file = dir.path().join("rack.kdl");
    fs::write(&rack_file, r#"rack "lab" units=4 { device "a" model="dell/r630-sff8" u=1; }"#)
        .expect("write the rack file");
    fs::write(dir.path().join("wiring.kdl"), "wiring { net a:nic1 ghost:nic1; }")
        .expect("write the wiring");
    let output = rack(dir.path(), &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown device `ghost`"));
}

/// A directory with the racks `lab` (4U) and `office` (2U), each holding one switch.
fn two_racks() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary directory");
    fs::write(
        dir.path().join("racks.kdl"),
        r#"rack "office" units=2 { device "sw" model="cisco/sg350-28" u=1; }
           rack "lab" units=4 { device "sw" model="cisco/sg350-28" u=4; }"#,
    )
    .expect("write the racks");
    dir
}

#[test]
fn draws_each_rack_under_its_name() {
    let dir = two_racks();
    let output = rack(dir.path(), &["--plain"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<_> = stdout.lines().collect();

    // Two rows per unit, a heading for each rack and a blank line between the racks.
    assert_eq!(lines.len(), 1 + 8 + 1 + 1 + 4);
    assert_eq!(lines[0], "rack \"lab\", 4U");
    assert_eq!(lines[9], "");
    assert_eq!(lines[10], "rack \"office\", 2U");
}

#[test]
fn draws_one_rack_without_a_heading() {
    let dir = two_racks();
    let output = rack(dir.path(), &["office", "--plain"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.lines().count(), 4);
    assert!(!stdout.contains("rack \""));
}

#[test]
fn reports_an_unknown_rack() {
    let dir = two_racks();
    let output = rack(dir.path(), &["attic"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "rackctl: unknown rack `attic`; the racks are lab, office\n"
    );
}
