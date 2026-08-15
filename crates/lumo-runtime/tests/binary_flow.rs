#![cfg(feature = "local-tools")]

use std::{path::Path, process::Command};

use serde_json::Value;
use tempfile::tempdir;

#[test]
fn all_three_binaries_complete_a_shared_local_flow() {
    let directory = tempdir().expect("temporary data directory");

    let seeded = run_json(
        env!("CARGO_BIN_EXE_lumo-debug"),
        directory.path(),
        &["seed", "--pin", "123456"],
    );
    assert_eq!(seeded["profile"], "debug");

    let locate = run_json(
        env!("CARGO_BIN_EXE_lumo-controller"),
        directory.path(),
        &["locate"],
    );
    assert_eq!(locate["status"], "queued");

    run_json(
        env!("CARGO_BIN_EXE_lumo-controlled"),
        directory.path(),
        &["setup"],
    );
    let mut report = Value::Null;
    for _ in 0..3 {
        report = run_json(
            env!("CARGO_BIN_EXE_lumo-controlled"),
            directory.path(),
            &[
                "report",
                "--latitude",
                "40.4191",
                "--longitude",
                "-3.7072",
                "--battery",
                "65",
            ],
        );
    }
    assert!(report["controlled"]["lastLocation"].is_object());

    let controller = run_json(
        env!("CARGO_BIN_EXE_lumo-controller"),
        directory.path(),
        &["snapshot"],
    );
    assert_eq!(controller["profile"], "controller");
    assert_eq!(controller["commands"][0]["status"], "completed");
    assert!(controller["events"]
        .as_array()
        .expect("events")
        .iter()
        .any(|event| event["kind"] == "arrival"));
}

fn run_json(binary: &str, data_dir: &Path, args: &[&str]) -> Value {
    let output = Command::new(binary)
        .arg("--data-dir")
        .arg(data_dir)
        .args(args)
        .env("LUMO_RUNTIME_MODE", "local")
        .env("LUMO_API_URL", "https://invalid.example.test")
        .output()
        .expect("binary should start");
    assert!(
        output.status.success(),
        "{} failed: {}",
        binary,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("valid JSON output")
}
