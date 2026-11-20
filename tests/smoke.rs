use std::process::Command;

#[test]
fn binary_reports_its_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_ict-jig-gen"))
        .arg("--version")
        .output()
        .expect("failed to run ict-jig-gen binary");

    assert!(
        output.status.success(),
        "binary exited with {:?}",
        output.status
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is valid UTF-8");
    assert_eq!(stdout.trim(), ict_jig_gen::banner());
}

#[test]
fn binary_without_arguments_shows_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_ict-jig-gen"))
        .output()
        .expect("run binary");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage"));
}
