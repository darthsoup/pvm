use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

// ── Cached regexes (compiled once on first use) ───────────────────────────────

/// `PHP 8.3.4` → captures "8.3" and "4".
static PHP_VERSION_FULL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"PHP (\d+\.\d+)\.(\d+)").unwrap());
/// `PHP 8.3` → captures "8.3".
static PHP_VERSION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"PHP (\d+\.\d+)").unwrap());
/// Any `8.3`-style major.minor.
static VERSION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d+\.\d+)").unwrap());
/// `(built: ...)` date from `php --version`.
static BUILT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\(built:\s*([^)]+)\)").unwrap());
/// Canonical Homebrew PHP installations, including revisions such as `8.5.12_1`.
static HOMEBREW_PHP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:/opt/homebrew|/usr/local|/home/linuxbrew/\.linuxbrew)/Cellar/php(?:@(\d+\.\d+))?/((\d+\.\d+)\.\d+)(?:_\d+)?/bin/php$",
    )
    .unwrap()
});
/// Exact versioned binary names, used only in standard system bin directories.
static PHP_BINARY_NAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^php[@-]?(\d+\.\d+)$").unwrap());

// ── Data types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PhpVersion {
    pub version: String,
    pub full_version: String,
    pub binary_path: PathBuf,
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

    detect_candidates(candidates, get_active_php_canon())
}

fn detect_candidates(candidates: Vec<PathBuf>, active_canon: Option<PathBuf>) -> Vec<PhpVersion> {
    let mut versions_map: HashMap<String, PhpVersion> = HashMap::new();
    let mut seen_binaries = HashSet::new();
    for binary in candidates {
        // Different opt, Cellar, and PATH entries can resolve to the same binary.
        // Deduplicate before both path inference and fallback execution, including
        // failed probes. A dangling symlink cannot identify an installed version.
        let Ok(canon) = std::fs::canonicalize(&binary) else {
            continue;
        };
        if !binary.is_file() || !seen_binaries.insert(canon.clone()) {
            continue;
        }

        // Fast path: derive the version from the resolved path without executing
        // php. Skips duplicate opt/Cellar/PATH entries for an already-seen version.
        if let Some(hint) = version_from_path(&canon) {
            if versions_map.contains_key(&hint.version) {
                continue;
            }
            let full_version = hint
                .full_version
                .map(|full| format!("PHP {}", full))
                .unwrap_or_else(|| format!("PHP {}", hint.version));
            versions_map.insert(
                hint.version.clone(),
                PhpVersion {
                    version: hint.version,
                    full_version,
                    binary_path: binary,
                    active: false,
                },
            );
            continue;
        }

        // Fallback: path carries no version (custom build) — execute php once.
        if let Some(version) = probe_php_binary(&binary) {
            versions_map
                .entry(version.version.clone())
                .or_insert(version);
        }
    }

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
                if name_str == "php"
                    || (name_str.starts_with("php") && is_php_versioned_name(&name_str))
                {
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
        "5.6", "7.0", "7.1", "7.2", "7.3", "7.4", "8.0", "8.1", "8.2", "8.3", "8.4",
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
    let output = Command::new(binary).arg("--version").output().ok()?;
    if !output.status.success() && output.stdout.is_empty() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let cap = PHP_VERSION_FULL_RE.captures(&stdout)?;

    let major_minor = cap.get(1)?.as_str().to_string();
    let patch = cap.get(2)?.as_str();
    let full_version = format!("PHP {}.{}", major_minor, patch);

    Some(PhpVersion {
        version: major_minor,
        full_version,
        binary_path: binary.to_path_buf(),
        active: false,
    })
}

pub fn probe_ini(binary: &Path) -> (Option<String>, Option<String>) {
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
    let s = s
        .trim_start_matches("php@")
        .trim_start_matches("php")
        .trim_start_matches("PHP");
    if let Some(cap) = VERSION_RE.captures(s) {
        return cap.get(1).unwrap().as_str().to_string();
    }
    s.to_string()
}

pub fn get_build_date(binary: &Path) -> Option<String> {
    let output = Command::new(binary).arg("--version").output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    BUILT_RE
        .captures(&stdout)
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

/// Returns the active PHP major.minor version and its binary path.
/// The fast path derives the version from the resolved `php` symlink/path with no
/// subprocess; only an unrecognizable (custom) path falls back to executing php.
pub fn get_active_php_quick() -> Option<(String, PathBuf)> {
    let php_path = which::which("php").ok()?;
    active_php_from_path(php_path)
}

fn active_php_from_path(php_path: PathBuf) -> Option<(String, PathBuf)> {
    // Fast path: derive the version from the resolved symlink/path — no subprocess.
    let canon = std::fs::canonicalize(&php_path).ok()?;
    if let Some(hint) = version_from_path(&canon) {
        return Some((hint.version, php_path));
    }

    // Fallback: custom build whose path carries no version — execute php.
    let output = Command::new(&php_path).arg("--version").output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = PHP_VERSION_RE
        .captures(&stdout)?
        .get(1)?
        .as_str()
        .to_string();
    Some((version, php_path))
}

struct PathVersion {
    version: String,
    full_version: Option<String>,
}

/// Infer versions only from recognized package-manager layouts. Arbitrary
/// directory names and custom executables must be verified by running PHP.
fn version_from_path(path: &Path) -> Option<PathVersion> {
    if let Some(cap) = HOMEBREW_PHP_RE.captures(path.to_str()?) {
        let version = cap.get(3)?.as_str();
        if cap
            .get(1)
            .is_some_and(|formula| formula.as_str() != version)
        {
            return None;
        }
        return Some(PathVersion {
            version: version.to_string(),
            full_version: Some(cap.get(2)?.as_str().to_string()),
        });
    }

    let parent = path.parent()?;
    if !["/usr/bin", "/usr/local/bin", "/bin"]
        .iter()
        .any(|directory| parent == Path::new(directory))
    {
        return None;
    }
    let cap = PHP_BINARY_NAME_RE.captures(path.file_name()?.to_str()?)?;
    Some(PathVersion {
        version: cap.get(1)?.as_str().to_string(),
        full_version: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_homebrew_versions_and_revision_suffixes() {
        for path in [
            "/opt/homebrew/Cellar/php/8.3.11/bin/php",
            "/opt/homebrew/Cellar/php/8.3.11_1/bin/php",
            "/usr/local/Cellar/php@8.3/8.3.11_12/bin/php",
            "/home/linuxbrew/.linuxbrew/Cellar/php/8.3.11_1/bin/php",
        ] {
            let hint = version_from_path(Path::new(path)).unwrap();
            assert_eq!(hint.version, "8.3", "{path}");
            assert_eq!(hint.full_version.as_deref(), Some("8.3.11"), "{path}");
        }
    }

    #[test]
    fn recognizes_exact_system_binary_names() {
        for path in ["/usr/bin/php8.3", "/usr/local/bin/php-8.3", "/bin/php@8.3"] {
            let hint = version_from_path(Path::new(path)).unwrap();
            assert_eq!(hint.version, "8.3");
            assert!(hint.full_version.is_none());
        }
    }

    #[test]
    fn rejects_custom_paths_and_ambiguous_installation_layouts() {
        for path in [
            "/custom/8.1/bin/php",
            "/custom/php8.1/bin/php",
            "/custom/bin/php8.3",
            "/custom/Cellar/php/8.3.11/bin/php",
            "/usr/bin/php8.3-custom",
            "/usr/bin/php8.3.11",
            "/opt/homebrew/Cellar/php@8.1/8.3.11/bin/php",
            "/opt/homebrew/Cellar/php/8.3.11_bad/bin/php",
            "/opt/homebrew/Cellar/php/8.3.11/bin/php-cgi",
        ] {
            assert!(version_from_path(Path::new(path)).is_none(), "{path}");
        }
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use std::os::unix::fs::{PermissionsExt, symlink};
        use std::sync::atomic::{AtomicU64, Ordering};

        struct Fixture(PathBuf);

        impl Fixture {
            fn new() -> Self {
                static SEQUENCE: AtomicU64 = AtomicU64::new(0);
                let unique = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos();
                let path = std::env::temp_dir().join(format!(
                    "pvm-detector-{}-{unique}-{}",
                    std::process::id(),
                    SEQUENCE.fetch_add(1, Ordering::Relaxed)
                ));
                std::fs::create_dir(&path).unwrap();
                Self(path)
            }

            fn binary(&self, relative: &str, success: bool) -> PathBuf {
                let binary = self.0.join(relative);
                std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
                let output = if success {
                    "printf 'PHP 8.3.11\\n'\n"
                } else {
                    "exit 1\n"
                };
                std::fs::write(
                    &binary,
                    format!("#!/bin/sh\nprintf 'probe\\n' >> \"$0.calls\"\n{output}"),
                )
                .unwrap();
                std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
                binary
            }
        }

        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        #[test]
        fn probes_custom_binary_instead_of_trusting_parent_version() {
            let fixture = Fixture::new();
            let binary = fixture.binary("custom/8.1/bin/php", true);
            let (version, path) = active_php_from_path(binary.clone()).unwrap();
            assert_eq!(version, "8.3");
            assert_eq!(path, binary);
        }

        #[test]
        fn does_not_infer_version_of_broken_custom_executable() {
            let fixture = Fixture::new();
            let binary = fixture.binary("custom/php8.3", false);
            assert!(active_php_from_path(binary).is_none());
        }

        #[test]
        fn probes_each_canonical_binary_once_and_preserves_active_path() {
            let fixture = Fixture::new();
            let binary = fixture.binary("custom/8.1/bin/php", true);
            let alias = fixture.0.join("php-alias");
            symlink(&binary, &alias).unwrap();
            let versions = detect_candidates(
                vec![binary.clone(), alias, binary.clone()],
                Some(std::fs::canonicalize(&binary).unwrap()),
            );
            assert_eq!(versions.len(), 1);
            assert_eq!(versions[0].version, "8.3");
            assert_eq!(versions[0].full_version, "PHP 8.3.11");
            assert_eq!(versions[0].binary_path, binary);
            assert!(versions[0].active);
            let calls = std::fs::read_to_string(binary.with_file_name("php.calls")).unwrap();
            assert_eq!(calls.lines().count(), 1);
        }

        #[test]
        fn does_not_retry_a_failed_probe_through_duplicate_paths() {
            let fixture = Fixture::new();
            let binary = fixture.binary("custom/php", false);
            let alias = fixture.0.join("php-alias");
            symlink(&binary, &alias).unwrap();
            let versions = detect_candidates(vec![binary.clone(), alias], None);
            assert!(versions.is_empty());
            let calls = std::fs::read_to_string(binary.with_file_name("php.calls")).unwrap();
            assert_eq!(calls.lines().count(), 1);
        }

        #[test]
        fn excludes_dangling_symlinks() {
            let fixture = Fixture::new();
            let alias = fixture.0.join("php8.3");
            symlink(fixture.0.join("missing"), &alias).unwrap();
            assert!(detect_candidates(vec![alias.clone()], None).is_empty());
            assert!(active_php_from_path(alias).is_none());
        }
    }
}
