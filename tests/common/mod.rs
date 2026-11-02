#![allow(dead_code)]
use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
pub struct Fixture {
    pub temp: tempfile::TempDir,
    pub home: PathBuf,
    pub bin: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let bin = temp.path().join("bin");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&bin).unwrap();
        Self { temp, home, bin }
    }
    pub fn root(&self) -> &Path {
        self.temp.path()
    }
    pub fn cmd(&self) -> Command {
        let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("pvm"));
        cmd.env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("PATH", &self.bin)
            .env("SHELL", "/bin/zsh")
            .env("TERM", "dumb")
            .env("NO_COLOR", "1")
            .current_dir(self.root());
        cmd
    }
    #[cfg(unix)]
    pub fn executable(&self, name: &str, script: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = self.bin.join(name);
        fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
    #[cfg(unix)]
    pub fn php(&self) {
        self.executable("php", r#"case "$1" in
--version) printf 'PHP 99.1.2 (cli) (built: Jan 1 2026)\n';;
--ini) printf 'Loaded Configuration File: /fake/php.ini\nScan for additional .ini files in: /fake/conf.d\n';;
-m) printf '[PHP Modules]\njson\nPDO\n';;
*) printf '%s\n' "$@"; exit "${PVM_TEST_EXIT:-0}";;
esac"#);
    }
}
