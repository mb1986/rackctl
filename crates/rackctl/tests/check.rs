//! Runs `rackctl check` as a user would.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use indoc::formatdoc;
use rackctl_core::catalog::Catalog;

/// Returns `rackctl check` without colour and without configuration from the environment.
fn rackctl_check() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rackctl"));
    command
        .arg("check")
        .env("NO_COLOR", "1")
        .env("HOME", "/nonexistent-home")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("RACKCTL_CONFIG");
    command
}

fn check(rack_file: &Path) -> Output {
    rackctl_check().arg("-c").arg(rack_file).output().expect("rackctl runs")
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
    let expected = formatdoc! {r#"
        catalog  {models} models, all valid ({models} built-in, 0 user)
        rack     {rack_file}: ok

        rack "lab", 12U
          devices  3: 1 server, 1 switch, 1 PDU
          models   3 different models
          space    3 of 12 U used, 9 U free
          free     U3-U11
          strips   pdu (left, front, U3-U12)
        "#,
        rack_file = rack_file.display(),
    };
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
fn reports_an_invalid_model_once() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let rack_file = dir.path().join("rack.kdl");
    fs::create_dir_all(dir.path().join("catalog/lab")).expect("mkdir");
    fs::write(dir.path().join("catalog/lab/broken.kdl"), r#"model { name "Broken" }"#)
        .expect("write the model");
    fs::write(
        &rack_file,
        r#"rack "lab" units=4 {
            device "a" model="lab/broken" u=1
            device "b" model="lab/broken" u=2
        }"#,
    )
    .expect("write the rack file");

    let output = check(&rack_file);
    let stderr = stderr(&output);
    assert_eq!(stderr.matches("the model `lab/broken` has problems").count(), 2);
    assert_eq!(stderr.matches("the model is missing `kind`").count(), 1);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn summarizes_an_empty_rack() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let rack_file = dir.path().join("rack.kdl");
    fs::write(&rack_file, r#"rack "lab" units=4"#).expect("write the rack file");

    let output = check(&rack_file);
    assert!(stdout(&output).contains("\n  devices  0\n"));
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn ignores_an_empty_rackctl_config() {
    let home = tempfile::tempdir().expect("temporary directory");
    let rack_dir = home.path().join(".config/rackctl");
    fs::create_dir_all(&rack_dir).expect("mkdir");
    fs::write(rack_dir.join("rack.kdl"), r#"rack "home" units=4"#).expect("write the rack file");

    let output = rackctl_check()
        .env("HOME", home.path())
        .env("RACKCTL_CONFIG", "")
        .output()
        .expect("rackctl runs");
    assert!(stdout(&output).contains("rack \"home\", 4U"), "stderr: {}", stderr(&output));
    assert_eq!(output.status.code(), Some(0));
}

#[cfg(target_os = "linux")]
#[test]
fn reports_an_output_that_cannot_be_written() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let rack_file = dir.path().join("rack.kdl");
    fs::write(&rack_file, r#"rack "lab" units=4"#).expect("write the rack file");
    let full = fs::File::create("/dev/full").expect("/dev/full");

    let output = rackctl_check().arg("-c").arg(&rack_file).stdout(full).output().expect("runs");
    assert!(stderr(&output).starts_with("rackctl: cannot write the output: "));
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn reports_a_missing_rack_file() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let output = check(&dir.path().join("rack.kdl"));
    assert!(stderr(&output).contains("cannot read the file"));
    assert_eq!(output.status.code(), Some(2));
}
