use crate::cli::ShellChoice;
pub fn print(shell: ShellChoice) {
    print!(
        "{}",
        match shell {
            ShellChoice::Bash => include_str!("hooks/bash.sh"),
            ShellChoice::Zsh => include_str!("hooks/zsh.sh"),
            ShellChoice::Fish => include_str!("hooks/fish.fish"),
        }
    );
}
