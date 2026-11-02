use anyhow::{Result, bail};
use dialoguer::{Confirm, Select};
use std::io::IsTerminal;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};

static INTERRUPTED: OnceLock<Arc<AtomicBool>> = OnceLock::new();
static HANDLER: OnceLock<std::result::Result<(), String>> = OnceLock::new();
pub fn interrupted() -> bool {
    INTERRUPTED
        .get()
        .is_some_and(|flag| flag.load(Ordering::Relaxed))
}
fn prepare() -> Result<()> {
    // Console restores terminal flags before raising SIGINT. Let it return the
    // interruption so our cursor guard and state lock can also be cleaned up.
    if let Err(error) = HANDLER.get_or_init(|| {
        let flag = INTERRUPTED
            .get_or_init(|| Arc::new(AtomicBool::new(false)))
            .clone();
        #[cfg(unix)]
        {
            signal_hook::flag::register(signal_hook::consts::SIGINT, flag)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        #[cfg(windows)]
        {
            ctrlc::set_handler(move || flag.store(true, Ordering::Relaxed))
                .map_err(|e| e.to_string())
        }
    }) {
        bail!("Cannot install prompt interrupt handler: {error}");
    }
    Ok(())
}
struct CursorGuard;
impl Drop for CursorGuard {
    fn drop(&mut self) {
        let _ = dialoguer::console::Term::stderr().show_cursor();
    }
}

#[derive(Debug)]
pub struct Interrupted;
impl std::fmt::Display for Interrupted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Interrupted")
    }
}
impl std::error::Error for Interrupted {}

pub fn interactive() -> bool {
    terminal_policy(
        std::io::stdin().is_terminal(),
        std::io::stderr().is_terminal(),
        &std::env::var("TERM").unwrap_or_default(),
    )
}
fn terminal_policy(stdin: bool, stderr: bool, term: &str) -> bool {
    stdin && stderr && term != "dumb"
}

pub trait Prompts {
    fn select(&self, items: &[String], active: Option<usize>) -> Result<Option<usize>>;
    fn confirm(&self, question: &str) -> Result<bool>;
}
pub struct TerminalPrompts;
fn prompt_result<T>(result: dialoguer::Result<T>) -> Result<T> {
    result.map_err(|e| match e {
        dialoguer::Error::IO(e) if e.kind() == std::io::ErrorKind::Interrupted => {
            Interrupted.into()
        }
        other => other.into(),
    })
}
impl Prompts for TerminalPrompts {
    fn select(&self, items: &[String], active: Option<usize>) -> Result<Option<usize>> {
        if !interactive() {
            bail!("Version selection requires a terminal. Pass a version explicitly: pvm use 8.3");
        }
        prepare()?;
        let _cursor = CursorGuard;
        let mut select = Select::new()
            .with_prompt("Select global PHP (Esc/q cancels)")
            .items(items);
        if let Some(active) = active {
            select = select.default(active);
        }
        prompt_result(select.interact_opt())
    }
    fn confirm(&self, question: &str) -> Result<bool> {
        if !interactive() {
            return Ok(false);
        }
        prepare()?;
        let _cursor = CursorGuard;
        Ok(prompt_result(
            Confirm::new()
                .with_prompt(question)
                .default(false)
                .interact_opt(),
        )?
        .unwrap_or(false))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requires_both_terminals_and_a_capable_terminal() {
        assert!(terminal_policy(true, true, "xterm"));
        assert!(!terminal_policy(false, true, "xterm"));
        assert!(!terminal_policy(true, false, "xterm"));
        assert!(!terminal_policy(true, true, "dumb"));
    }
    #[test]
    #[cfg(unix)]
    fn confirmation_defaults_to_no() {
        if std::env::var_os("PVM_CONFIRM_TEST_CHILD").is_some() {
            assert!(!TerminalPrompts.confirm("Restart fixture?").unwrap());
            return;
        }
        let python = if cfg!(target_os = "macos") {
            "/usr/bin/python3"
        } else {
            "python3"
        };
        let result = std::process::Command::new(python)
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/confirm.py"))
            .arg(std::env::current_exe().unwrap())
            .env("PVM_CONFIRM_TEST_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
