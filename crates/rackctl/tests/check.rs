//! Runs `rackctl check` as a user would.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use rackctl_core::catalog::Catalog;

fn check(rack_file: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rackctl"))
        .arg("check")
        .arg("-c")
        .arg(rack_file)
        .env("NO_COLOR", "1")
        .env("HOME", "/nonexistent-home")
        .env_remove("RACKCTL_CONFIG")
        .output()
        .expect("rackctl runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn summarizes_a_valid_rack() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let rack_file = dir.path().join("rack.kdl");
    fs::write(
        &rack_file,
        r#"rack "lab" units=12 {
            device "sw" model="cisco/sg350-28" u=12
            device "srv" model="dell/r730-sff8" u=1
            device "pdu" model="apc/ap7952" mount="left" u=3
        }"#,
    )
    .expect("write the rack file");

    let output = check(&rack_file);
    let models = Catalog::builtin().ids().count();
    let expected = format!(
        "catalog  {models} models, all valid ({models} built-in, 0 user)\n\
         rack     {}: ok\n\
         \n\
         rack \"lab\", 12U\n\
         \x20 devices  3: 1 server, 1 switch, 1 PDU\n\
         \x20 models   3 different models\n\
         \x20 space    3 of 12 U used, 9 U free\n\
         \x20 free     U3-U11\n\
         \x20 strips   pdu (left, front, U3-U12)\n",
        rack_file.display()
    );
    assert_eq!(stdout(&output), expected);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
}

#[test]
fn counts_models_from_the_user_catalog() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let rack_file = dir.path().join("rack.kdl");
    fs::create_dir_all(dir.path().join("catalog/lab")).expect("mkdir");
    fs::write(
        dir.path().join("catalog/lab/shelf.kdl"),
        r#"model { name "Lab shelf"; kind "shelf"; height 2 }"#,
    )
    .expect("write the model");
    fs::write(&rack_file, r#"rack "lab" units=4 { device "shelf" model="lab/shelf" u=1 }"#)
        .expect("write the rack file");

    let output = check(&rack_file);
    let built_in = Catalog::builtin().ids().count();
    assert!(stdout(&output).starts_with(&format!(
        "catalog  {} models, all valid ({built_in} built-in, 1 user)\n",
        built_in + 1
    )));
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn reports_problems_and_exits_with_code_2() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let rack_file = dir.path().join("rack.kdl");
    fs::write(
        &rack_file,
        r#"rack "lab" units=12 {
            device "a" model="dell/r730-sff8" u=1
            device "b" model="dell/r630-sff8" u=2
        }"#,
    )
    .expect("write the rack file");

    let output = check(&rack_file);
    assert!(stdout(&output).ends_with(&format!("rack     {}: 1 problem\n", rack_file.display())));
    assert!(stderr(&output).contains("`b` overlaps `a` on U2"));
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn reports_a_missing_rack_file() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let output = check(&dir.path().join("rack.kdl"));
    assert!(stderr(&output).contains("cannot read the file"));
    assert_eq!(output.status.code(), Some(2));
}
