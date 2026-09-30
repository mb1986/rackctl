//! Runs `rackctl trace` as a user would.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Runs `rackctl trace` on `rack_file`, without configuration from the environment.
fn trace(rack_file: &Path, endpoint: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rackctl"))
        .arg("-c")
        .arg(rack_file)
        .args(["trace", endpoint])
        .env("NO_COLOR", "1")
        .env("HOME", "/nonexistent-home")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("RACKCTL_CONFIG")
        .output()
        .expect("rackctl runs")
}

fn golden_rack() -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "../../tests/golden/rack/rack.kdl"].iter().collect()
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
fn reports_a_rack_without_wiring() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let rack_file = dir.path().join("rack.kdl");
    fs::write(&rack_file, r#"rack "lab" units=4 { device "a" model="dell/r630-sff8" u=1; }"#)
        .expect("write the rack file");
    let output = trace(&rack_file, "a:mgmt");
    assert_eq!(output.status.code(), Some(2));
    let wiring_file = dir.path().join("wiring.kdl");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!("rackctl: there is no wiring: {} does not exist\n", wiring_file.display())
    );
}
