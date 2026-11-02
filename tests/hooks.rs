#[cfg(unix)]
#[test]
fn shell_and_terminal_contracts() {
    let python = if cfg!(target_os = "macos") {
        "/usr/bin/python3"
    } else {
        "python3"
    };
    let result = std::process::Command::new(python)
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/shells.py"))
        .arg(assert_cmd::cargo::cargo_bin!("pvm"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
