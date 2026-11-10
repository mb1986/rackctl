//! Runs `rackctl trace` as a user would.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Runs `rackctl trace` on the configuration at `config`, a directory or a file, without
/// configuration from the environment.
fn trace(config: &Path, endpoint: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rackctl"))
        .arg("-c")
        .arg(config)
        .args(["trace", endpoint])
        .env("NO_COLOR", "1")
        .env("HOME", "/nonexistent-home")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("RACKCTL_CONFIG")
        .output()
        .expect("rackctl runs")
}

fn golden_rack() -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "../../tests/golden/rack"].iter().collect()
}

#[test]
fn follows_the_cables_of_the_golden_rack() {
    for (endpoint, path) in [
        ("srv01:mgmt", "srv01:mgmt -> patch-32:15 -> sg300:10\n"),
        ("sg300:10", "sg300:10 -> patch-32:15 -> srv01:mgmt\n"),
        ("patch-32:15", "srv01:mgmt -> patch-32:15 -> sg300:10\n"),
        ("pdu:8", "pdu:8 -> srv01:psu1\n"),
        ("srv01:nic1", "srv01:nic1 is not connected\n"),
    ] {
        let output = trace(&golden_rack(), endpoint);
        assert!(output.status.success(), "{endpoint}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), path, "{endpoint}");
    }
}

#[test]
fn reports_an_endpoint_the_rack_does_not_have() {
    let output = trace(&golden_rack(), "sg30:1");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "rackctl: unknown device `sg30`; did you mean `sg300`?\n"
    );

    let output = trace(&golden_rack(), "srv01");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).starts_with("rackctl: `srv01` is not an endpoint")
    );
}

#[test]
fn follows_a_cable_between_racks() {
    let dir = tempfile::tempdir().expect("temporary directory");
    fs::write(
        dir.path().join("racks.kdl"),
        r#"rack "office" units=2 { device "sw" model="cisco/sg350-28" u=1; }
           rack "lab" units=3 {
               device "sw" model="cisco/sg350-28" u=3
               device "srv" model="dell/r630-sff8" u=2
           }"#,
    )
    .expect("write the racks");
    fs::write(
        dir.path().join("wiring.kdl"),
        "wiring { net srv:nic1 lab.sw:1; net lab.sw:28 office.sw:28; }",
    )
    .expect("write the wiring");

    let output = trace(dir.path(), "office.sw:28");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "office.sw:28 -> lab.sw:28\n");

    let output = trace(dir.path(), "srv:nic1");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "lab.srv:nic1 -> lab.sw:1\n");

    let output = trace(dir.path(), "sw:1");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "rackctl: `sw` is in several racks; write `lab.sw` or `office.sw`\n"
    );
}

#[test]
fn reports_a_rack_without_wiring() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let rack_file = dir.path().join("rack.kdl");
    fs::write(&rack_file, r#"rack "lab" units=4 { device "a" model="dell/r630-sff8" u=1; }"#)
        .expect("write the rack file");
    let output = trace(&rack_file, "a:mgmt");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "rackctl: there is no wiring: the configuration has no `wiring` node\n"
    );
}
