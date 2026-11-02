use crate::utils::{Config, atomic_write, ensure_pvm_dir};
use anyhow::{Context, Result, bail};
use std::fs::{File, OpenOptions, TryLockError};
use std::sync::atomic::{AtomicU64, Ordering};

/// Keep the file open for the lifetime of the guard. Never unlink lock files.
pub struct StateLock {
    _file: File,
}
impl StateLock {
    pub fn acquire(hook: bool) -> Result<Self> {
        Self::acquire_with_output(hook, false)
    }
    pub fn acquire_with_output(hook: bool, quiet: bool) -> Result<Self> {
        let path = ensure_pvm_dir()?.join("switch.lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) if hook => {
                bail!("A PVM switch is in progress; activation will retry")
            }
            Err(TryLockError::WouldBlock) => {
                if !quiet {
                    eprintln!("Waiting for another PVM operation...");
                }
                file.lock().context("Failed to lock PVM state")?;
            }
            Err(TryLockError::Error(e)) => return Err(e).context("Failed to lock PVM state"),
        }
        Ok(Self { _file: file })
    }
}

pub fn publish(config: &Config) -> Result<String> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let token = format!(
        "{time:x}-{:x}-{:x}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    atomic_write(
        &ensure_pvm_dir()?.join("hook-state"),
        format!("{token}\t{}\n", u8::from(config.auto_enabled())).as_bytes(),
    )?;
    Ok(token)
}

pub fn token(config: &Config) -> Result<String> {
    let path = ensure_pvm_dir()?.join("hook-state");
    if let Ok(content) = std::fs::read_to_string(path)
        && let Some((token, flag)) = content.trim().split_once('\t')
        && !token.is_empty()
        && flag == if config.auto_enabled() { "1" } else { "0" }
    {
        return Ok(token.to_owned());
    }
    publish(config)
}
