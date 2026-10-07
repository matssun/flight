// SPDX-License-Identifier: MIT

use std::path::PathBuf;

/// `--flag value` options plus positional words, for the small command set. No clever
/// parsing: unknown flags are errors, so a typo never silently changes what runs.
pub struct Args {
    options: Vec<(String, String)>,
    switches: Vec<String>,
    pub positional: Vec<String>,
}

impl Args {
    /// `valued` flags take a value; `switches` do not; anything else starting with `--` is an error.
    pub fn parse(args: &[String], valued: &[&str], switches: &[&str]) -> Result<Self, String> {
        let mut out = Self {
            options: Vec::new(),
            switches: Vec::new(),
            positional: Vec::new(),
        };
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            if valued.contains(&arg.as_str()) {
                let v = it.next().ok_or_else(|| format!("{arg} needs a value"))?;
                out.options.push((arg.clone(), v.clone()));
            } else if switches.contains(&arg.as_str()) {
                out.switches.push(arg.clone());
            } else if arg.starts_with("--") {
                return Err(format!("unknown option {arg:?}"));
            } else {
                out.positional.push(arg.clone());
            }
        }
        Ok(out)
    }

    pub fn value(&self, flag: &str) -> Option<&str> {
        self.options
            .iter()
            .rev()
            .find(|(f, _)| f == flag)
            .map(|(_, v)| v.as_str())
    }

    pub fn values(&self, flag: &str) -> Vec<&str> {
        self.options
            .iter()
            .filter(|(f, _)| f == flag)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    pub fn switch(&self, flag: &str) -> bool {
        self.switches.iter().any(|s| s == flag)
    }
}

/// `--config-dir`, else `$FLIGHT_CONFIG_DIR`, else `$XDG_CONFIG_HOME/flight`, else
/// `~/.config/flight`.
pub fn config_dir(args: &Args) -> Result<PathBuf, String> {
    if let Some(d) = args.value("--config-dir") {
        return Ok(PathBuf::from(d));
    }
    if let Some(d) = std::env::var_os("FLIGHT_CONFIG_DIR") {
        return Ok(PathBuf::from(d));
    }
    if let Some(d) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(d).join("flight"));
    }
    let home =
        std::env::var_os("HOME").ok_or("cannot find a config directory: set --config-dir")?;
    Ok(PathBuf::from(home).join(".config").join("flight"))
}

/// A readable machine name for a new node or UI.
pub fn default_name() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "flight".to_owned())
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn values_switches_and_positional() {
        let args = Args::parse(
            &a(&[
                "--socket", "x", "bundle", "words", "--once", "--socket", "y",
            ]),
            &["--socket"],
            &["--once"],
        )
        .unwrap();
        assert_eq!(args.values("--socket"), vec!["x", "y"]);
        assert_eq!(args.value("--socket"), Some("y"));
        assert!(args.switch("--once"));
        assert_eq!(args.positional, vec!["bundle", "words"]);
    }

    #[test]
    fn unknown_flags_and_missing_values_are_errors() {
        assert!(Args::parse(&a(&["--nope"]), &[], &[]).is_err());
        assert!(Args::parse(&a(&["--socket"]), &["--socket"], &[]).is_err());
    }
}
