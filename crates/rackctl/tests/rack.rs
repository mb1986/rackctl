//! Runs `rackctl rack` as a user would.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Runs `rackctl rack` on `rack_file` with `args`, without configuration from the environment.
fn rack(rack_file: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rackctl"))
        .arg("-c")
        .arg(rack_file)
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
        let output = rack(&golden("rack.kdl"), args);
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
    let output = rack(&rack_file, &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown device `ghost`"));
}
