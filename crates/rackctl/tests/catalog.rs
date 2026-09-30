//! Runs `rackctl catalog show` as a user would.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

/// Runs `rackctl catalog show` with `args`, without configuration from the environment.
fn show(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rackctl"))
        .args(["catalog", "show"])
        .args(args)
        .env("NO_COLOR", "1")
        .env("HOME", "/nonexistent-home")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("RACKCTL_CONFIG")
        .output()
        .expect("rackctl runs")
}

fn golden(name: &str) -> String {
    let path: PathBuf =
        [env!("CARGO_MANIFEST_DIR"), "../../tests/golden/faces", name].iter().collect();
    fs::read_to_string(path).expect("golden file")
}

#[test]
fn draws_a_face_as_in_the_golden_files() {
    for (args, file) in [
        (&["dell/r630-sff8"][..], "dell/r630-sff8.txt"),
        (&["dell/r630-sff8", "--numbers"], "dell/r630-sff8.numbers.txt"),
        (&["dell/r630-sff8", "--state", "off"], "dell/r630-sff8.off.txt"),
        (&["apc/ap7952"], "apc/ap7952.strip.txt"),
    ] {
        let output = show(&[args, &["--plain"]].concat());
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), golden(file), "{file}");
    }
}

#[test]
fn leaves_out_colours_with_no_color() {
    let output = show(&["dell/r630-sff8"]);
    assert_eq!(String::from_utf8_lossy(&output.stdout), golden("dell/r630-sff8.txt"));
}

#[test]
fn reports_an_unknown_model() {
    let output = show(&["nope/x"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&output.stderr), "rackctl: unknown model `nope/x`\n");
}

#[test]
fn draws_a_model_without_a_face_as_an_empty_frame() {
    let output = show(&["generic/blank-2u", "--plain"]);
    assert!(output.status.success());
    let (screws, plate) = (format!("┊⊕{}⊕┊", " ".repeat(48)), format!("┊{}┊", " ".repeat(50)));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let rows: Vec<&str> = stdout.lines().skip(2).take(4).collect();
    assert_eq!(
        rows,
        [
            format!("11 {screws} 11"),
            format!("   {plate}   "),
            format!("10 {plate} 10"),
            format!("   {screws}   ")
        ]
    );
}
