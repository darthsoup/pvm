mod common;
use common::Fixture;
use serde_json::Value;
use std::fs;

#[test]
fn syntax_conflicts_and_required_script() {
    let f = Fixture::new();
    for args in [
        vec!["--list", "current"],
        vec!["use", "8.3", "--skip", "apach"],
        vec!["use", "8.3", "--restart", "--no-restart"],
        vec!["current", "--json", "--short"],
        vec!["run", "8.3"],
    ] {
        f.cmd().args(args).assert().code(2);
    }
    f.cmd().args(["--verbose", "--list"]).assert().success();
    f.cmd()
        .args(["use"])
        .write_stdin("")
        .assert()
        .code(1)
        .stderr(predicates::str::contains("requires a terminal"));
}
#[test]
fn pin_requires_force_and_canonicalizes_versions() {
    let f = Fixture::new();
    for value in ["not-a-version", "junk8.3", "8.3junk", "8.3\n8.4"] {
        f.cmd().args(["pin", value]).assert().failure();
    }
    assert!(!f.root().join(".pvmrc").exists());
    f.cmd().args(["pin", "PHP@8.3.123"]).assert().success();
    let path = f.root().join(".pvmrc");
    assert_eq!(fs::read_to_string(&path).unwrap(), "8.3\n");
    fs::write(&path, " 8.3 \n").unwrap();
    f.cmd().args(["pin", "8.3"]).assert().success();
    assert_eq!(fs::read_to_string(&path).unwrap(), " 8.3 \n");
    f.cmd()
        .args(["pin", "8.4"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--force"));
    f.cmd().args(["pin", "--force", "8.4"]).assert().success();
    assert_eq!(fs::read_to_string(path).unwrap(), "8.4\n");
}
#[test]
fn aliases_resolve_chains_and_reject_invalid_mutations() {
    let f = Fixture::new();
    f.cmd().args(["alias", "base", "8.3.4"]).assert().success();
    f.cmd().args(["alias", "legacy", "base"]).assert().success();
    let path = f.home.join(".pvm/aliases.json");
    let before = fs::read(&path).unwrap();
    for args in [
        vec!["alias", "base", "legacy"],
        vec!["alias", "bad", "missing"],
        vec!["alias", "8.3", "8.4"],
        vec!["unalias", "base"],
    ] {
        f.cmd().args(args).assert().failure();
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    f.cmd().args(["pin", "legacy"]).assert().success();
    assert_eq!(
        fs::read_to_string(f.root().join(".pvmrc")).unwrap(),
        "legacy\n"
    );
    assert!(f.home.join(".pvm/hook-state").is_file());
}
#[test]
fn config_preserves_unknown_fields_and_invalid_values_do_not_write() {
    let f = Fixture::new();
    fs::create_dir_all(f.home.join(".pvm")).unwrap();
    let path = f.home.join(".pvm/config.json");
    fs::write(&path, r#"{"other":{"keep":true}}"#).unwrap();
    f.cmd()
        .args(["config", "set", "auto_switch", "false"])
        .assert()
        .success();
    let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(value["other"]["keep"], true);
    assert_eq!(value["auto_switch"], false);
    assert!(
        fs::read_to_string(f.home.join(".pvm/hook-state"))
            .unwrap()
            .ends_with("\t0\n")
    );
    let before = fs::read(&path).unwrap();
    for args in [
        vec!["config", "set", "auto_switch", "yes"],
        vec!["config", "set", "restart_webserver", "sometimes"],
        vec!["config", "set", "unknown", "true"],
    ] {
        f.cmd().args(args).assert().failure();
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    f.cmd()
        .args(["config", "get", "restart_webserver"])
        .assert()
        .success()
        .stdout("ask\n");
    fs::write(&path, r#"{"restart_webserver":"invalid"}"#).unwrap();
    f.cmd().args(["config", "get"]).assert().failure();
    f.cmd().args(["use", "99.1"]).assert().failure();
}
#[test]
fn unknown_shell_fails_without_script_and_read_commands_do_not_create_state() {
    let f = Fixture::new();
    for args in [vec!["init"], vec!["completions"]] {
        f.cmd()
            .env("SHELL", "/bin/unsupported")
            .args(args)
            .assert()
            .code(1)
            .stdout("");
    }
    f.cmd().args(["config", "get"]).assert().success();
    assert!(!f.home.join(".pvm").exists());
}
#[test]
fn json_errors_and_diagnostic_statuses_are_truthful() {
    let f = Fixture::new();
    for command in ["current", "doctor"] {
        let result = f
            .cmd()
            .args([command, "--json"])
            .assert()
            .code(1)
            .get_output()
            .clone();
        let doc: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(doc["schema_version"], 1);
        assert_eq!(doc["command"], command);
        if command == "current" {
            assert!(doc["data"].is_null());
            assert!(doc["error"].is_object());
        } else {
            assert!(doc["data"]["summary"]["failed"].as_u64().unwrap() > 0);
        }
        assert!(result.stderr.is_empty());
        assert!(!result.stdout.contains(&27));
    }
}
#[cfg(unix)]
#[test]
fn forwards_script_flags_and_child_exit_codes_without_global_switching() {
    let f = Fixture::new();
    f.php();
    f.cmd()
        .args(["run", "99.1", "index.php", "--help", "-v", "--unknown=x"])
        .assert()
        .success()
        .stdout("index.php\n--help\n-v\n--unknown=x\n");
    f.cmd()
        .args(["exec", "99.1", "--", "php", "index.php", "--help"])
        .env("PVM_TEST_EXIT", "23")
        .assert()
        .code(23)
        .stdout("index.php\n--help\n");
    assert!(!f.home.join(".pvm").exists());
    // OS-native script paths survive argument parsing and launch.
    use std::os::unix::ffi::OsStringExt;
    let script = std::ffi::OsString::from_vec(b"script-\xff.php".to_vec());
    let output = f
        .cmd()
        .args(["run", "99.1"])
        .arg(&script)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(output, b"script-\xff.php\n");
    fs::rename(f.bin.join("php"), f.bin.join("php83")).unwrap();
    f.cmd()
        .args(["exec", "99.1", "--", "php", "script.php", "--help"])
        .assert()
        .success()
        .stdout("script.php\n--help\n");
}
#[cfg(unix)]
#[test]
fn json_records_short_output_color_and_verbose_streams() {
    let f = Fixture::new();
    f.php();
    f.cmd().args(["alias", "zulu", "99.1"]).assert().success();
    f.cmd().args(["alias", "alpha", "zulu"]).assert().success();
    f.cmd()
        .args(["current", "--short"])
        .assert()
        .success()
        .stdout("99.1\n");
    let result = f
        .cmd()
        .args(["--color", "always", "current", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let doc: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(doc["data"]["current"]["full_version"], "99.1.2");
    assert_eq!(
        doc["data"]["current"]["aliases"],
        serde_json::json!(["alpha", "zulu"])
    );
    assert!(!result.stdout.contains(&27));
    let result = f
        .cmd()
        .args(["--color", "always", "current"])
        .assert()
        .success()
        .get_output()
        .clone();
    assert!(result.stdout.contains(&27));
    f.cmd()
        .args(["--verbose", "which", "alpha"])
        .assert()
        .success()
        .stdout(format!("{}\n", f.bin.join("php").display()))
        .stderr(predicates::str::contains("release line 99.1"));
    let output = f
        .cmd()
        .args(["info", "alpha", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doc: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(doc["data"]["version"]["ini_path"], "/fake/php.ini");
}
#[cfg(unix)]
#[test]
fn hook_protocol_does_not_restart_or_request_privileges() {
    let f = Fixture::new();
    f.php();
    fs::write(f.root().join(".pvmrc"), "99.1\n").unwrap();
    let result = f
        .cmd()
        .args(["auto", "--hook"])
        .assert()
        .success()
        .get_output()
        .clone();
    let fields: Vec<_> = std::str::from_utf8(&result.stdout)
        .unwrap()
        .trim()
        .split('\t')
        .collect();
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[1], "99.1");
    assert_eq!(fields[2], "0");
    assert!(result.stderr.is_empty());
    let lock = fs::OpenOptions::new()
        .write(true)
        .open(f.home.join(".pvm/switch.lock"))
        .unwrap();
    lock.lock().unwrap();
    f.cmd()
        .args(["auto", "--hook"])
        .timeout(std::time::Duration::from_secs(2))
        .assert()
        .failure()
        .stdout("")
        .stderr(predicates::str::contains("in progress"));
}

#[test]
fn concurrent_alias_writes_preserve_both_updates() {
    let f = Fixture::new();
    let mut first = f.cmd();
    first.args(["alias", "first", "8.3"]);
    let mut second = f.cmd();
    second.args(["alias", "second", "8.4"]);
    std::thread::scope(|scope| {
        let a = scope.spawn(|| first.output().unwrap());
        let b = scope.spawn(|| second.output().unwrap());
        assert!(a.join().unwrap().status.success());
        assert!(b.join().unwrap().status.success());
    });
    let aliases: Value =
        serde_json::from_slice(&fs::read(f.home.join(".pvm/aliases.json")).unwrap()).unwrap();
    assert_eq!(aliases["first"], "8.3");
    assert_eq!(aliases["second"], "8.4");
}

#[test]
fn explicit_help_color_and_completion_contracts() {
    let f = Fixture::new();
    let result = f
        .cmd()
        .args(["--color", "always", "--help"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(result.contains(&27));
    let result = f
        .cmd()
        .args(["--color", "never", "--help"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(!result.contains(&27));
    for shell in ["bash", "zsh", "fish"] {
        let output = f
            .cmd()
            .args(["completions", "--shell", shell])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let text = String::from_utf8(output).unwrap();
        assert!(!text.contains("--hook"));
        assert!(!text.contains("-l hook"));
        assert!(text.contains("restart"));
        assert!(text.contains("config"));
        assert!(text.contains("json"));
    }
}
