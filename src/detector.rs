use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

// ── Data types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PhpVersion {
    pub version: String,
    pub full_version: String,
    pub binary_path: PathBuf,
    pub ini_path: Option<String>,
    pub scan_dir: Option<String>,
    pub active: bool,
}

impl PhpVersion {
    pub fn major_minor_tuple(&self) -> (u32, u32) {
        let parts: Vec<&str> = self.version.splitn(2, '.').collect();
        let major = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
        let minor = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        (major, minor)
    }
}

pub fn detect_php_versions() -> Vec<PhpVersion> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    #[cfg(target_os = "macos")]
    candidates.extend(scan_homebrew_paths());

    #[cfg(target_os = "linux")]
    candidates.extend(scan_linux_paths());

    candidates.extend(scan_path_binaries());

    let mut versions_map: HashMap<String, PhpVersion> = HashMap::new();
    for binary in candidates {
        if !binary.exists() && !binary.is_symlink() {
            continue;
        }
        if let Some(version) = probe_php_binary(&binary) {
            versions_map
                .entry(version.version.clone())
                .or_insert(version);
        }
    }

    let active_canon = get_active_php_canon();

    let mut versions: Vec<PhpVersion> = versions_map.into_values().collect();

    for v in &mut versions {
        if let Some(ref active) = active_canon {
            v.active = is_same_file(&v.binary_path, active);
        }
    }

    versions.sort_by_key(|v| std::cmp::Reverse(v.major_minor_tuple()));
    versions
}

#[cfg(target_os = "macos")]
fn scan_homebrew_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    let opt_prefixes = ["/usr/local/opt", "/opt/homebrew/opt"];
    for prefix in &opt_prefixes {
        let dir = Path::new(prefix);
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with("php@") || name_str == "php" {
                    let bin = entry.path().join("bin/php");
                    if bin.exists() {
                        paths.push(bin);
                    }
                }
            }
        }
    }

    let cellar_prefixes = ["/usr/local/Cellar", "/opt/homebrew/Cellar"];
    for prefix in &cellar_prefixes {
        let dir = Path::new(prefix);
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with("php@") || name_str == "php" {
                    // /opt/homebrew/Cellar/php@8.3/<patch>/bin/php
                    if let Ok(ver_entries) = std::fs::read_dir(entry.path()) {
                        for ver_entry in ver_entries.flatten() {
                            let bin = ver_entry.path().join("bin/php");
                            if bin.exists() {
                                paths.push(bin);
                            }
                        }
                    }
                }
            }
        }
    }

    paths
}

#[cfg(target_os = "linux")]
fn scan_linux_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    let bin_dirs = ["/usr/bin", "/usr/local/bin"];
    for dir_str in &bin_dirs {
        let dir = Path::new(dir_str);
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str == "php" || (name_str.starts_with("php") && is_php_versioned_name(&name_str)) {
                    let path = entry.path();
                    if path.is_file() || path.is_symlink() {
                        paths.push(path);
                    }
                }
            }
        }
    }

    // Debian/Ubuntu: /etc/php/<version>/ exists alongside /usr/bin/phpX.Y
    let etc_php = Path::new("/etc/php");
    if let Ok(entries) = std::fs::read_dir(etc_php) {
        for entry in entries.flatten() {
            let ver_str = entry.file_name();
            let ver_str = ver_str.to_string_lossy();
            for bin_dir in &["/usr/bin", "/usr/local/bin"] {
                let bin = PathBuf::from(bin_dir).join(format!("php{}", ver_str));
                if bin.exists() {
                    paths.push(bin);
                }
            }
        }
    }

    let usr_lib_php = Path::new("/usr/lib/php");
    if let Ok(entries) = std::fs::read_dir(usr_lib_php) {
        for entry in entries.flatten() {
            let bin = entry.path().join("php");
            if bin.exists() {
                paths.push(bin);
            }
        }
    }

    paths
}

#[cfg(target_os = "linux")]
fn is_php_versioned_name(name: &str) -> bool {
    // Exclude "phpize", "php-config", etc. — only match "php8.3", "php7.4" …
    name[3..].starts_with(|c: char| c.is_ascii_digit())
}

fn scan_path_binaries() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(p) = which::which("php") {
        paths.push(p);
    }

    let known_versions = [
        "5.6", "7.0", "7.1", "7.2", "7.3", "7.4",
        "8.0", "8.1", "8.2", "8.3", "8.4",
    ];
    for ver in &known_versions {
        if let Ok(p) = which::which(format!("php{}", ver)) {
            paths.push(p);
        }
        if let Ok(p) = which::which(format!("php{}", ver.replace('.', ""))) {
            paths.push(p);
        }
    }

    paths
}

fn probe_php_binary(binary: &Path) -> Option<PhpVersion> {
    let version_re = Regex::new(r"PHP (\d+\.\d+)\.(\d+)").ok()?;

    let output = Command::new(binary).arg("--version").output().ok()?;
    if !output.status.success() && output.stdout.is_empty() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let cap = version_re.captures(&stdout)?;

    let major_minor = cap.get(1)?.as_str().to_string();
    let patch = cap.get(2)?.as_str();
    let full_version = format!("PHP {}.{}", major_minor, patch);

    let (ini_path, scan_dir) = probe_ini(binary);

    Some(PhpVersion {
        version: major_minor,
        full_version,
        binary_path: binary.to_path_buf(),
        ini_path,
        scan_dir,
        active: false,
    })
}

fn probe_ini(binary: &Path) -> (Option<String>, Option<String>) {
    let output = match Command::new(binary).arg("--ini").output() {
        Ok(o) => o,
        Err(_) => return (None, None),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut ini_path = None;
    let mut scan_dir = None;

    for line in stdout.lines() {
        if line.contains("Loaded Configuration File:") {
            let value = extract_after_colon(line);
            if value != "(none)" && !value.is_empty() {
                ini_path = Some(value.to_string());
            }
        } else if line.contains("Scan for additional .ini files in:") {
            let value = extract_after_colon(line);
            if value != "(none)" && !value.is_empty() {
                scan_dir = Some(value.to_string());
            }
        }
    }

    (ini_path, scan_dir)
}

fn extract_after_colon(line: &str) -> &str {
    line.split_once(':')
        .map(|x| x.1)
        .map(str::trim)
        .unwrap_or("")
}

fn get_active_php_canon() -> Option<PathBuf> {
    which::which("php")
        .ok()
        .map(|p| std::fs::canonicalize(&p).unwrap_or(p))
}

fn is_same_file(a: &Path, b: &Path) -> bool {
    let a_canon = std::fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let b_canon = std::fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());
    a_canon == b_canon
}

pub fn get_active_version(versions: &[PhpVersion]) -> Option<&PhpVersion> {
    versions.iter().find(|v| v.active)
}

pub fn find_version<'a>(versions: &'a [PhpVersion], target: &str) -> Option<&'a PhpVersion> {
    let normalized = normalize_version_str(target);
    versions.iter().find(|v| v.version == normalized)
}

// Accepts: "8.3", "8.3.4", "php8.3", "PHP8.3", "php@8.3"
pub fn normalize_version_str(s: &str) -> String {
    let s = s.trim_start_matches("php@")
             .trim_start_matches("php")
             .trim_start_matches("PHP");
    let re = Regex::new(r"(\d+\.\d+)").unwrap();
    if let Some(cap) = re.captures(s) {
        return cap.get(1).unwrap().as_str().to_string();
    }
    s.to_string()
}

pub fn get_build_date(binary: &Path) -> Option<String> {
    let output = Command::new(binary).arg("--version").output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let re = Regex::new(r"\(built:\s*([^)]+)\)").ok()?;
    re.captures(&stdout)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
}

pub fn get_loaded_modules(binary: &Path) -> Vec<String> {
    let output = match Command::new(binary).arg("-m").output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('['))
        .map(|l| l.trim().to_string())
        .collect()
}
