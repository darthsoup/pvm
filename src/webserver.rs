use crate::prompts::Prompts;
use anyhow::{Context, Result};
use clap::ValueEnum;
use serde::Serialize;
use std::path::Path;
use std::process::Command;

use crate::utils::{Config, is_command_available};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WebServer {
    Apache,
    Nginx,
}

impl WebServer {
    pub fn name(self) -> &'static str {
        match self {
            WebServer::Apache => "Apache",
            WebServer::Nginx => "Nginx",
        }
    }

    fn process_names(self) -> &'static [&'static str] {
        match self {
            WebServer::Apache => &["httpd", "apache2"],
            WebServer::Nginx => &["nginx"],
        }
    }

    fn restart_cmd(self) -> (&'static str, Vec<&'static str>) {
        match self {
            WebServer::Apache => {
                let ctl = if is_command_available("apachectl") {
                    "apachectl"
                } else if is_command_available("apache2ctl") {
                    "apache2ctl"
                } else {
                    "httpd"
                };
                (ctl, vec!["restart"])
            }
            WebServer::Nginx => ("nginx", vec!["-s", "reload"]),
        }
    }
}

pub fn detect_apache() -> bool {
    is_command_available("apachectl")
        || is_command_available("apache2ctl")
        || is_command_available("httpd")
}

pub fn detect_nginx() -> bool {
    is_command_available("nginx")
}

pub fn detect_running_servers() -> Vec<WebServer> {
    let mut servers = Vec::new();
    if detect_apache() && is_running(WebServer::Apache) {
        servers.push(WebServer::Apache);
    }
    if detect_nginx() && is_running(WebServer::Nginx) {
        servers.push(WebServer::Nginx);
    }
    servers
}

pub fn is_running(server: WebServer) -> bool {
    for name in server.process_names() {
        let ok = Command::new("pgrep")
            .args(["-x", name])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            return true;
        }
    }
    false
}

pub fn restart_server(server: WebServer) -> Result<()> {
    let (cmd, args) = server.restart_cmd();

    if crate::prompts::interrupted() {
        return Err(crate::prompts::Interrupted.into());
    }
    if let Err(first_error) = crate::utils::command_output(Command::new(cmd).args(&args)) {
        let mut sudo = Command::new("sudo");
        if !crate::prompts::interactive() {
            sudo.arg("-n");
        }
        crate::utils::command_output(sudo.arg(cmd).args(&args)).with_context(|| {
            format!(
                "Failed to restart {}. Unprivileged attempt: {first_error:#}",
                server.name()
            )
        })?;
    }

    Ok(())
}

#[derive(Debug, Serialize)]
pub struct RestartOutcome {
    pub server: WebServer,
    pub status: String,
    pub error: Option<String>,
    pub notice: Option<String>,
}

pub fn handle_post_switch_restart(
    opts: &crate::cli::RestartArgs,
    config: &Config,
    prompts: &impl Prompts,
    quiet: bool,
) -> Result<Vec<RestartOutcome>> {
    let mut outcomes = Vec::new();
    if opts.no_restart {
        return Ok(outcomes);
    }
    for server in detect_running_servers() {
        if opts.skip.contains(&server) {
            continue;
        }
        let interactive = !quiet && crate::prompts::interactive();
        let restart =
            restart_decision(server, opts, config.restart_policy(), interactive, prompts)?;
        let notice =
            (!restart && !interactive && !opts.restart && config.restart_policy() == "ask")
                .then(|| "No interactive confirmation; use --restart or --no-restart".to_owned());
        let error = if restart {
            restart_server(server).err().map(|e| format!("{e:#}"))
        } else {
            None
        };
        outcomes.push(RestartOutcome {
            server,
            status: if error.is_some() {
                "failed"
            } else if restart {
                "restarted"
            } else {
                "skipped"
            }
            .into(),
            error,
            notice,
        });
    }
    Ok(outcomes)
}

fn restart_decision(
    server: WebServer,
    opts: &crate::cli::RestartArgs,
    policy: &str,
    interactive: bool,
    prompts: &impl Prompts,
) -> Result<bool> {
    if opts.no_restart || opts.skip.contains(&server) {
        return Ok(false);
    }
    if opts.restart {
        return Ok(true);
    }
    match policy {
        "always" => Ok(true),
        "never" => Ok(false),
        _ if interactive => prompts.confirm(&format!("{} is running. Restart it?", server.name())),
        _ => Ok(false),
    }
}

pub fn restart_explicit(server: Option<WebServer>) -> Result<()> {
    let _lock = crate::state::StateLock::acquire(false)?;
    let servers = server
        .map(|s| vec![s])
        .unwrap_or_else(detect_running_servers);
    if servers.is_empty() {
        eprintln!("No supported web servers are currently running.");
    }
    for server in servers {
        restart_server(server)?;
        eprintln!("{} restarted", server.name());
    }
    Ok(())
}

pub fn update_apache_config(php_version: &str) -> Result<()> {
    let config_paths = [
        "/etc/apache2/httpd.conf",
        "/etc/httpd/conf/httpd.conf",
        "/usr/local/etc/httpd/httpd.conf",
        "/opt/homebrew/etc/httpd/httpd.conf",
    ];

    for path_str in &config_paths {
        let path = Path::new(path_str);
        if path.exists() {
            return update_apache_config_file(path, php_version);
        }
    }

    Ok(())
}

fn update_apache_config_file(path: &Path, php_version: &str) -> Result<()> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let php_ver_nodot = php_version.replace('.', "");
    let module_candidates = [
        format!("/usr/local/lib/httpd/modules/libphp{}.so", php_version),
        format!("/opt/homebrew/lib/httpd/modules/libphp{}.so", php_version),
        format!("/usr/lib/apache2/modules/libphp{}.so", php_ver_nodot),
        format!("/usr/lib/apache2/modules/libphp{}.so", php_version),
        String::from("/usr/lib/apache2/modules/libphp.so"),
    ];

    let has_php = content
        .lines()
        .any(|line| line.trim_start().starts_with("LoadModule php"));
    if !has_php {
        return Ok(());
    }
    let module = module_candidates
        .iter()
        .find(|p| Path::new(p.as_str()).exists())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No Apache module found for PHP {php_version}; existing configuration retained"
            )
        })?;
    let backup = {
        let mut b = path.to_path_buf();
        b.set_extension("conf.pvm.bak");
        b
    };
    if !backup.exists() {
        std::fs::copy(path, &backup)
            .with_context(|| format!("Failed to create backup at {}", backup.display()))?;
    }

    let mut lines: Vec<String> = content.lines().map(String::from).collect();

    for line in &mut lines {
        let trimmed = line.trim_start();
        if (trimmed.starts_with("LoadModule php") || trimmed.starts_with("LoadModule php_module"))
            && !trimmed.starts_with('#')
        {
            *line = format!("#{}", line);
        }
    }

    {
        let new_line = format!("LoadModule php_module {}", module);
        // Insert after the last commented-out LoadModule php line so the
        // directive stays grouped with existing PHP module lines
        let insert_at = lines
            .iter()
            .rposition(|l| l.contains("LoadModule php"))
            .map(|i| i + 1)
            .unwrap_or(lines.len());
        lines.insert(insert_at, new_line);
    }

    let new_content = lines.join("\n");
    crate::utils::atomic_write(path, format!("{new_content}\n").as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Confirm(bool);
    impl Prompts for Confirm {
        fn select(&self, _: &[String], _: Option<usize>) -> Result<Option<usize>> {
            panic!("unexpected picker")
        }
        fn confirm(&self, _: &str) -> Result<bool> {
            Ok(self.0)
        }
    }
    #[test]
    fn restart_precedence_and_noninteractive_policy() {
        let mut opts = crate::cli::RestartArgs::default();
        assert!(!restart_decision(WebServer::Apache, &opts, "ask", false, &Confirm(true)).unwrap());
        assert!(!restart_decision(WebServer::Apache, &opts, "ask", true, &Confirm(false)).unwrap());
        assert!(restart_decision(WebServer::Apache, &opts, "ask", true, &Confirm(true)).unwrap());
        assert!(
            restart_decision(WebServer::Apache, &opts, "always", false, &Confirm(false)).unwrap()
        );
        opts.restart = true;
        assert!(
            restart_decision(WebServer::Apache, &opts, "never", false, &Confirm(false)).unwrap()
        );
        opts.skip.push(WebServer::Apache);
        assert!(
            !restart_decision(WebServer::Apache, &opts, "always", true, &Confirm(true)).unwrap()
        );
        opts.skip.clear();
        opts.no_restart = true;
        assert!(
            !restart_decision(WebServer::Apache, &opts, "always", true, &Confirm(true)).unwrap()
        );
    }
    #[test]
    fn apache_without_php_directives_is_not_modified_or_backed_up() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("httpd.conf");
        std::fs::write(&path, "ServerName localhost\n").unwrap();
        update_apache_config_file(&path, "99.1").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "ServerName localhost\n"
        );
        assert!(!temp.path().join("httpd.conf.pvm.bak").exists());
    }
}
