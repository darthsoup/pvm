use crate::utils::command_output as checked_output;
use anyhow::{Context, Result, bail};
use serde::Serialize;
#[cfg(target_os = "linux")]
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::aliases::{load_aliases, resolve_version_or_alias};
use crate::cli::RestartArgs;
use crate::detector::{self, PhpVersion};
use crate::prompts::TerminalPrompts;
use crate::state::{self, StateLock};
use crate::utils::Config;
use crate::webserver::{self, RestartOutcome};

#[derive(Default)]
pub struct SwitchOptions {
    pub restart: RestartArgs,
    pub update_apache: bool,
    pub hook: bool,
    pub quiet: bool,
}
#[derive(Debug, Serialize)]
pub struct SwitchResult {
    pub requested: String,
    pub version: String,
    pub binary_path: PathBuf,
    pub changed: bool,
    pub token: String,
    pub apache: String,
    pub restarts: Vec<RestartOutcome>,
    pub errors: Vec<String>,
}

trait SwitchBackend {
    fn quick_active(&self) -> Option<(String, PathBuf)>;
    fn versions(&self) -> Vec<PhpVersion>;
    fn switch(&self, target: &PhpVersion, all: &[PhpVersion], hook: bool) -> Result<()>;
    fn verify(&self) -> Result<(String, PathBuf)>;
    fn apache(&self, version: &str, changed: bool) -> Result<bool>;
    fn restart(
        &self,
        args: &RestartArgs,
        config: &Config,
        quiet: bool,
    ) -> Result<Vec<RestartOutcome>>;
}
struct SystemBackend;
impl SwitchBackend for SystemBackend {
    fn quick_active(&self) -> Option<(String, PathBuf)> {
        detector::get_active_php_quick()
    }
    fn versions(&self) -> Vec<PhpVersion> {
        detector::detect_php_versions()
    }
    fn switch(&self, target: &PhpVersion, all: &[PhpVersion], hook: bool) -> Result<()> {
        do_switch(target, all, hook)
    }
    fn verify(&self) -> Result<(String, PathBuf)> {
        let path = which::which("php").context("php is not in PATH after switching")?;
        let version = detector::probe_php_binary(&path)
            .context("Active PHP could not execute successfully after switching")?;
        Ok((version.version, path))
    }
    fn apache(&self, version: &str, changed: bool) -> Result<bool> {
        let pending = crate::utils::pvm_dir()?.join("pending-apache");
        reconcile_apache(&pending, version, changed, || {
            webserver::update_apache_config(version)
        })
    }
    fn restart(
        &self,
        args: &RestartArgs,
        config: &Config,
        quiet: bool,
    ) -> Result<Vec<RestartOutcome>> {
        webserver::handle_post_switch_restart(args, config, &TerminalPrompts, quiet)
    }
}

// Keep a failed explicit update retryable even when the CLI already switched.
fn reconcile_apache(
    pending: &Path,
    version: &str,
    changed: bool,
    update: impl FnOnce() -> Result<()>,
) -> Result<bool> {
    if changed {
        crate::utils::atomic_write(pending, version.as_bytes())?;
    } else {
        match std::fs::read_to_string(pending) {
            Ok(value) if value == version => {}
            Ok(_) => return Ok(false),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e.into()),
        }
    }
    update()?;
    std::fs::remove_file(pending)?;
    Ok(true)
}

pub fn switch_version(version: &str, opts: &SwitchOptions) -> Result<SwitchResult> {
    let _lock = StateLock::acquire_with_output(opts.hook, opts.quiet)?;
    let config = Config::load()?;
    if opts.hook && !config.auto_enabled() {
        bail!("Automatic switching is disabled");
    }
    let resolved = resolve_version_or_alias(version, &load_aliases()?)?;
    // Invalidate caches before any system side effect, including failed switches.
    let mut attempt_token = if opts.hook {
        state::token(&config)?
    } else {
        state::publish(&config)?
    };
    let result = switch_with_backend(version, &resolved, opts, &config, &SystemBackend, || {
        attempt_token = state::publish(&config)?;
        Ok(attempt_token.clone())
    });
    let mut result = match result {
        Err(error) if opts.hook => return Err(error.context(HookFailureToken(attempt_token))),
        other => other?,
    };
    if result.token.is_empty() {
        result.token = attempt_token;
    }
    Ok(result)
}

#[derive(Debug)]
pub struct HookFailureToken(pub String);
impl std::fmt::Display for HookFailureToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Automatic PHP activation failed")
    }
}

fn switch_with_backend(
    requested: &str,
    resolved: &str,
    opts: &SwitchOptions,
    config: &Config,
    backend: &impl SwitchBackend,
    invalidate: impl FnOnce() -> Result<String>,
) -> Result<SwitchResult> {
    let mut result = SwitchResult {
        requested: requested.into(),
        version: resolved.into(),
        binary_path: PathBuf::new(),
        changed: false,
        token: String::new(),
        apache: "skipped".into(),
        restarts: Vec::new(),
        errors: Vec::new(),
    };
    if let Some((version, path)) = backend.quick_active()
        && version == resolved
    {
        result.binary_path = path;
    } else {
        let versions = backend.versions();
        let target = find_target(&versions, resolved)?;
        result.token = invalidate()?;
        backend
            .switch(target, &versions, opts.hook)
            .context("PHP switching failed; system selection may have changed")?;
        let (actual, path) = backend
            .verify()
            .context("Switch verification failed; system selection may have changed")?;
        if actual != resolved {
            bail!(
                "Requested PHP {resolved}, but PATH resolves to PHP {actual} ({}). System selection may have changed; check PATH with pvm doctor",
                path.display()
            );
        }
        result.changed = true;
        result.binary_path = path;
    }
    if opts.update_apache && !opts.hook {
        match backend.apache(resolved, result.changed) {
            Ok(false) => {}
            Ok(true) => result.apache = "checked".into(),
            Err(error) => {
                result.apache = "failed".into();
                result
                    .errors
                    .push(format!("Apache configuration: {error:#}"));
            }
        }
    }
    if !opts.hook && (result.changed || opts.restart.restart) {
        collect_restarts(&mut result, opts, config, backend)?;
    }

    Ok(result)
}

fn collect_restarts(
    result: &mut SwitchResult,
    opts: &SwitchOptions,
    config: &Config,
    backend: &impl SwitchBackend,
) -> Result<()> {
    let mut restart = opts.restart.clone();
    if result.apache == "failed" {
        restart.skip.push(webserver::WebServer::Apache);
    }
    match backend.restart(&restart, config, opts.quiet) {
        Ok(outcomes) => {
            result
                .errors
                .extend(outcomes.iter().filter_map(|o| o.error.clone()));
            result.restarts = outcomes;
        }
        Err(error) => {
            if error
                .downcast_ref::<crate::prompts::Interrupted>()
                .is_some()
            {
                return Err(error).context("PHP is active; restart confirmation interrupted");
            }
            result.errors.push(format!("Web server restart: {error:#}"));
        }
    }
    Ok(())
}

pub fn find_target<'a>(versions: &'a [PhpVersion], version: &str) -> Result<&'a PhpVersion> {
    detector::find_version(versions, version).ok_or_else(|| {
        if versions.is_empty() {
            anyhow::anyhow!(
                "No PHP versions found. Install PHP via Homebrew or your package manager"
            )
        } else {
            anyhow::anyhow!(
                "PHP {version} not found. Installed: {}. Run pvm list",
                versions
                    .iter()
                    .map(|v| v.version.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    })
}
#[cfg(target_os = "macos")]
fn do_switch(target: &PhpVersion, all: &[PhpVersion], hook: bool) -> Result<()> {
    let Ok(brew) = which::which("brew") else {
        return update_symlink(&target.binary_path, hook);
    };
    switch_homebrew(&brew, target, all)
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn switch_homebrew(brew: &Path, target: &PhpVersion, all: &[PhpVersion]) -> Result<()> {
    crate::output::verbose("Switch backend: Homebrew");
    let prefix_output = checked_output(Command::new(brew).arg("--prefix"))?;
    let prefix = Path::new(std::str::from_utf8(&prefix_output.stdout)?.trim());
    anyhow::ensure!(prefix.is_absolute(), "Homebrew returned an invalid prefix");
    let owner = |php: &PhpVersion| -> Option<String> {
        let binary = php.binary_path.canonicalize().ok()?;
        for formula in [format!("php@{}", php.version), String::from("php")] {
            let installed = prefix.join("opt").join(&formula).join("bin/php");
            if installed.canonicalize().ok().as_ref() == Some(&binary) {
                return Some(formula);
            }
        }
        None
    };
    let formula = owner(target).with_context(|| format!(
        "PHP {} ({}) is not provided by this Homebrew installation. Use pvm exec or adjust PATH",
        target.version, target.binary_path.display()
    ))?;
    let binary = prefix.join("opt").join(&formula).join("bin/php");
    anyhow::ensure!(
        detector::probe_php_binary(&binary).is_some_and(|php| php.version == target.version),
        "Homebrew formula {formula} does not provide usable PHP {}",
        target.version
    );
    if let Some(active) = all.iter().find(|php| php.active).and_then(owner)
        && active != formula
    {
        checked_output(Command::new(brew).args(["unlink", &active]))?;
    }
    checked_output(Command::new(brew).args(["link", "--overwrite", "--force", &formula]))?;
    Ok(())
}
#[cfg(target_os = "linux")]
fn do_switch(target: &PhpVersion, _all: &[PhpVersion], hook: bool) -> Result<()> {
    if which::which("update-alternatives").is_ok() {
        crate::output::verbose("Switch backend: update-alternatives");
        let mut command = Command::new("update-alternatives");
        command.args([
            OsStr::new("--set"),
            OsStr::new("php"),
            target.binary_path.as_os_str(),
        ]);
        match checked_output(&mut command) {
            Ok(_) => return Ok(()),
            Err(error) if !hook => {
                crate::output::verbose(&format!("Unprivileged switch failed: {error:#}"));
                let mut sudo = Command::new("sudo");
                if !crate::prompts::interactive() {
                    sudo.arg("-n");
                }
                checked_output(sudo.args([
                    OsStr::new("update-alternatives"),
                    OsStr::new("--set"),
                    OsStr::new("php"),
                    target.binary_path.as_os_str(),
                ]))?;
                return Ok(());
            }
            Err(error) => {
                return Err(error)
                    .context("Hook cannot request privileges; run pvm use explicitly");
            }
        }
    }
    update_symlink(&target.binary_path, hook)
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn do_switch(_target: &PhpVersion, _all: &[PhpVersion], _hook: bool) -> Result<()> {
    bail!("PHP version switching is not supported on this platform")
}
#[cfg(unix)]
fn update_symlink(binary: &Path, hook: bool) -> Result<()> {
    crate::output::verbose("Switch backend: symlink");
    let mut errors = Vec::new();
    for link in [
        Path::new("/usr/local/bin/php"),
        Path::new("/opt/homebrew/bin/php"),
    ] {
        let parent = link.parent().unwrap();
        if !parent.exists() {
            continue;
        }
        if link.exists() && !link.is_symlink() {
            errors.push(format!(
                "Refusing to replace regular file {}",
                link.display()
            ));
            continue;
        }
        let attempt = || -> Result<()> {
            let temp = tempfile::tempdir_in(parent)?;
            let staged = temp.path().join("php");
            std::os::unix::fs::symlink(binary, &staged)?;
            std::fs::rename(staged, link)?;
            Ok(())
        };
        match attempt() {
            Ok(()) => return Ok(()),
            Err(error) => errors.push(format!("{}: {error:#}", link.display())),
        }
        if !hook {
            let mut sudo = Command::new("sudo");
            if !crate::prompts::interactive() {
                sudo.arg("-n");
            }
            match checked_output(sudo.arg("ln").arg("-sfn").arg(binary).arg(link)) {
                Ok(_) => return Ok(()),
                Err(error) => errors.push(format!("{error:#}")),
            }
        }
    }
    bail!(
        "Could not update PHP symlink: {}. Run pvm use explicitly or check directory permissions",
        errors.join("; ")
    )
}

pub fn exec_version(version: &str, args: &[OsString], run: bool) -> Result<()> {
    let resolved = resolve_version_or_alias(version, &load_aliases()?)?;
    let versions = detector::detect_php_versions();
    let target = find_target(&versions, &resolved)?;
    let (command, arguments) = if run {
        (target.binary_path.as_os_str(), args)
    } else {
        let (command, arguments) = args
            .split_first()
            .context("No command specified. Use pvm exec 8.3 -- php script.php")?;
        (command.as_os_str(), arguments)
    };
    let bin_dir = target
        .binary_path
        .parent()
        .context("Invalid PHP binary path")?;
    let shim = tempfile::tempdir()?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        std::fs::canonicalize(&target.binary_path)?,
        shim.path().join("php"),
    )?;
    #[cfg(not(unix))]
    if target
        .binary_path
        .file_name()
        .is_none_or(|name| name != "php.exe")
    {
        bail!("Executing a PHP binary with a nonstandard filename is unsupported on this platform");
    }
    let mut paths = vec![shim.path().to_path_buf(), bin_dir.to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let mut child = Command::new(command);
    child
        .args(arguments)
        .env("PATH", std::env::join_paths(paths)?)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let status = child
        .status()
        .with_context(|| format!("Failed to run {command:?}"))?;
    // Clean the temporary shim before exiting (process::exit does not run destructors).
    drop(shim);
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        std::process::exit(
            status
                .code()
                .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)),
        );
    }
    #[cfg(not(unix))]
    std::process::exit(status.code().unwrap_or(1));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Fake {
        actual: &'static str,
        apache_error: bool,
        pending: Option<PathBuf>,
        active: bool,
        apache_skipped: std::cell::Cell<bool>,
    }
    impl SwitchBackend for Fake {
        fn quick_active(&self) -> Option<(String, PathBuf)> {
            self.active
                .then(|| (self.actual.into(), "/fake/php".into()))
        }
        fn versions(&self) -> Vec<PhpVersion> {
            vec![PhpVersion {
                version: "8.3".into(),
                full_version: "PHP 8.3.4".into(),
                binary_path: "/fake/php".into(),
                active: false,
            }]
        }
        fn switch(&self, _: &PhpVersion, _: &[PhpVersion], _: bool) -> Result<()> {
            Ok(())
        }
        fn verify(&self) -> Result<(String, PathBuf)> {
            Ok((self.actual.into(), "/fake/php".into()))
        }
        fn apache(&self, version: &str, changed: bool) -> Result<bool> {
            let update = || {
                if self.apache_error {
                    bail!("permission denied");
                }
                Ok(())
            };
            if let Some(path) = &self.pending {
                reconcile_apache(path, version, changed, update)
            } else {
                update().map(|_| true)
            }
        }
        fn restart(&self, args: &RestartArgs, _: &Config, _: bool) -> Result<Vec<RestartOutcome>> {
            self.apache_skipped
                .set(args.skip.contains(&webserver::WebServer::Apache));
            Ok(vec![])
        }
    }
    #[test]
    fn rejects_wrong_php_after_successful_backend_operation() {
        let error = switch_with_backend(
            "8.3",
            "8.3",
            &SwitchOptions::default(),
            &Config::default(),
            &Fake {
                actual: "8.4",
                ..Default::default()
            },
            || Ok("token".into()),
        )
        .unwrap_err();
        assert!(error.to_string().contains("PATH resolves to PHP 8.4"));
    }
    #[test]
    fn failed_apache_update_is_retried_before_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut backend = Fake {
            actual: "8.3",
            apache_error: true,
            pending: Some(dir.path().join("pending")),
            ..Default::default()
        };
        let opts = SwitchOptions {
            update_apache: true,
            restart: RestartArgs {
                restart: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let run = |backend: &Fake| {
            switch_with_backend("8.3", "8.3", &opts, &Config::default(), backend, || {
                Ok("token".into())
            })
            .unwrap()
        };
        assert_eq!(run(&backend).apache, "failed");
        assert!(backend.apache_skipped.get());
        backend.active = true;
        assert_eq!(run(&backend).apache, "failed");
        assert!(backend.apache_skipped.get());
        backend.apache_error = false;
        let retry = run(&backend);
        assert_eq!(retry.apache, "checked");
        assert!(!retry.changed);
        assert!(!backend.apache_skipped.get());
        assert_eq!(run(&backend).apache, "skipped");
    }

    #[cfg(unix)]
    #[test]
    fn homebrew_preflights_target_before_unlinking_active_php() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let executable = |path: &Path, script: &str| {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, format!("#!/bin/sh\n{script}\n")).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        let brew = root.join("brew");
        executable(
            &brew,
            r#"prefix=${0%/*}
if [ "$1" = --prefix ]; then echo "$prefix"; else echo "$*" >> "$prefix/calls"; fi"#,
        );
        let active = PhpVersion {
            version: "8.4".into(),
            full_version: "8.4.1".into(),
            binary_path: root.join("opt/php/bin/php"),
            active: true,
        };
        executable(&active.binary_path, "echo 'PHP 8.4.1'");
        let mut target = PhpVersion {
            version: "8.3".into(),
            full_version: "8.3.1".into(),
            binary_path: root.join("custom/php"),
            active: false,
        };
        executable(&target.binary_path, "echo 'PHP 8.3.1'");
        let installed = [active];
        assert!(switch_homebrew(&brew, &target, &installed).is_err());
        target.binary_path = root.join("opt/php@8.3/bin/php");
        executable(&target.binary_path, "echo 'PHP 8.4.1'");
        assert!(switch_homebrew(&brew, &target, &installed).is_err());
        let log = root.join("calls");
        assert!(!log.exists());
        executable(&target.binary_path, "echo 'PHP 8.3.1'");
        switch_homebrew(&brew, &target, &installed).unwrap();
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "unlink php\nlink --overwrite --force php@8.3\n"
        );
        switch_homebrew(&brew, &installed[0], &[]).unwrap();
        assert!(
            std::fs::read_to_string(log)
                .unwrap()
                .ends_with("link --overwrite --force php\n")
        );
    }

    #[test]
    fn reports_partial_success_and_hooks_skip_apache() {
        let backend = Fake {
            actual: "8.3",
            apache_error: true,
            ..Default::default()
        };
        let mut opts = SwitchOptions {
            update_apache: true,
            ..Default::default()
        };
        let result = switch_with_backend("8.3", "8.3", &opts, &Config::default(), &backend, || {
            Ok("token".into())
        })
        .unwrap();
        assert!(result.changed);
        assert_eq!(result.apache, "failed");
        assert_eq!(result.errors.len(), 1);
        opts.hook = true;
        let result = switch_with_backend("8.3", "8.3", &opts, &Config::default(), &backend, || {
            Ok("token".into())
        })
        .unwrap();
        assert!(result.errors.is_empty());
        assert_eq!(result.apache, "skipped");
    }
}
