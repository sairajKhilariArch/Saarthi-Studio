//! The startup flags a packager or a user runs before anything else. Both used
//! to be ignored, which meant `--version` booted the app and touched the real
//! home. These tests run the built binary, not a helper function.

use std::process::Command;

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_free-ai-studio"))
}

#[test]
fn version_prints_the_package_version_and_exits() {
    let output = binary()
        .arg("--version")
        .output()
        .expect("the built binary should run");
    assert!(
        output.status.success(),
        "--version exited {:?}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "--version printed {stdout:?}"
    );
    assert!(
        stdout.contains("Sarthi Studio"),
        "--version printed {stdout:?}"
    );
}

#[test]
fn short_version_flag_matches_the_long_one() {
    let long = binary().arg("--version").output().unwrap();
    let short = binary().arg("-V").output().unwrap();
    assert_eq!(long.stdout, short.stdout);
}

#[test]
fn help_lists_the_startup_modes_and_exits() {
    let output = binary().arg("--help").output().unwrap();
    assert!(output.status.success(), "--help exited {:?}", output.status);
    let stdout = String::from_utf8_lossy(&output.stdout);
    for flag in ["--headless", "--browser", "--version", "--help"] {
        assert!(stdout.contains(flag), "--help omitted {flag}: {stdout:?}");
    }
}
