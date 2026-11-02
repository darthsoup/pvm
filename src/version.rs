use anyhow::{Result, bail};
use std::fmt;
use std::str::FromStr;

/// PVM selects release lines; patch suffixes are accepted for compatibility.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseLine(String);

impl FromStr for ReleaseLine {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> Result<Self> {
        let value = single_line(input)?;
        let lower = value.to_ascii_lowercase();
        let value = lower
            .strip_prefix("php@")
            .or_else(|| lower.strip_prefix("php"))
            .unwrap_or(&lower);
        let parts: Vec<_> = value.split('.').collect();
        if !(2..=3).contains(&parts.len())
            || parts
                .iter()
                .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
        {
            bail!(
                "Invalid PHP version '{input}'. Use a release line such as 8.3 or a defined alias"
            );
        }
        let numbers: Vec<u32> = parts
            .iter()
            .map(|p| p.parse())
            .collect::<std::result::Result<_, _>>()?;
        Ok(Self(format!("{}.{}", numbers[0], numbers[1])))
    }
}

impl fmt::Display for ReleaseLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

pub fn single_line(input: &str) -> Result<&str> {
    let value = input.trim();
    if value.is_empty() || value.contains(['\n', '\r']) {
        bail!("Expected one nonempty PHP version or alias");
    }
    Ok(value)
}

pub fn alias_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_complete_release_requests() {
        for value in ["8.3", " 8.3.4 ", "PHP8.3", "pHp@8.3.9"] {
            assert_eq!(value.parse::<ReleaseLine>().unwrap().to_string(), "8.3");
        }
        for value in [
            "",
            "garbage8.3",
            "8.3garbage",
            "8",
            "8.3.4.5",
            "8.3\n8.4",
            "@8.3",
        ] {
            assert!(value.parse::<ReleaseLine>().is_err(), "{value}");
        }
    }
}
