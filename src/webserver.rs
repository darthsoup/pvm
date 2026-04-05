use anyhow::{Context, Result};
use colored::Colorize;
use std::path::Path;
use std::process::Command;

use crate::utils::{is_command_available, prompt_yes_no, Config};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    let status = Command::new(cmd)
        .args(&args)
        .status()
        .with_context(|| format!("Failed to run {} {}", cmd, args.join(" ")));

    let succeeded = match status {
        Ok(s) => s.success(),
        Err(_) => false,
    };

    if !succeeded {
        let sudo_status = Command::new("sudo")
            .arg(cmd)
            .args(&args)
            .status()
            .with_context(|| format!("Failed to run sudo {} {}", cmd, args.join(" ")))?;
        if !sudo_status.success() {
            anyhow::bail!("Failed to restart {}", server.name());
        }
    }

    println!("{} {} restarted", "✓".green(), server.name());
    Ok(())
}

pub struct RestartOpts<'a> {
    pub no_restart: bool,
    pub skip: &'a [String],
    pub silent: bool,
}

pub fn handle_post_switch_restart(opts: &RestartOpts) -> Result<()> {
    if opts.no_restart {
        return Ok(());
    }

    let config = Config::load().unwrap_or_default();
    let policy = config.restart_policy();

    for server in detect_running_servers() {
        let name_lower = server.name().to_lowercase();
        if opts.skip.iter().any(|s| s.to_lowercase() == name_lower) {
            continue;
        }

        let should_restart = match policy {
            "always" => true,
            "never" => false,
            _ => {
                if opts.silent {
                    false // no prompts in shell-hook mode
                } else {
                    prompt_yes_no(
                        &format!("{} is running. Restart it?", server.name()),
                        true,
                    )
                }
            }
        };

        if should_restart {
            restart_server(server)?;
        }
    }

    Ok(())
}

pub fn update_apache_config(php_version: &str) -> Result<()> {
    let config_paths = [
        "/etc/apache2/httpd.conf",
        "/etc/httpd/conf/httpd.conf",
        "/usr/local/etc/httpd/httpd.conf",
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

    let php_ver_nodot = php_version.replace('.', "");
    let module_candidates = [
        format!("/usr/local/lib/httpd/modules/libphp{}.so", php_version),
        format!("/opt/homebrew/lib/httpd/modules/libphp{}.so", php_version),
        format!("/usr/lib/apache2/modules/libphp{}.so", php_ver_nodot),
        format!("/usr/lib/apache2/modules/libphp{}.so", php_version),
        String::from("/usr/lib/apache2/modules/libphp.so"),
    ];

    if let Some(module) = module_candidates
        .iter()
        .find(|p| Path::new(p.as_str()).exists())
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
    std::fs::write(path, new_content)
        .with_context(|| format!("Failed to write {}", path.display()))
}
