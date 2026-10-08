// SPDX-License-Identifier: MIT

use std::time::Duration;

const DEFAULT_SOCKET: &str = "flight";
const DEFAULT_REFRESH_SECS: u64 = 2;

/// One remote tmux server, reached through an OpenSSH host alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
    pub alias: String,
    pub socket: String,
}

/// Command-line configuration. Flight never assumes the default tmux server: the local
/// endpoint is a named socket (default `flight`), and remotes are explicit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub local_socket: Option<String>,
    pub ssh: Vec<SshTarget>,
    pub refresh: Duration,
    pub once: bool,
}

pub const USAGE: &str =
    "usage: flight                  (no arguments: run Flight on this machine, see `flight solo --help`)
       flight [--socket NAME] [--no-local] [--ssh ALIAS[:SOCKET]]... [--refresh SECS] [--once]

  --socket NAME        local tmux socket (tmux -L NAME); default 'flight'
  --no-local           do not watch a local tmux server
  --ssh ALIAS[:SOCKET] also watch a remote host via an ssh alias (default socket 'flight')
  --refresh SECS       refresh interval; default 2
  --once               print one frame as text and exit";

/// The distributed commands, shown with --help.
pub const ROLES: &str = "distributed mode (an orchestrator, nodes that observe tmux, a dashboard):
  flight orchestrator ...   run or administer the orchestrator
  flight node ...           join an orchestrator, observe local tmux and report
  flight ui ...             join an orchestrator and show the dashboard
  (each prints its own usage)";

impl Config {
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Self, String> {
        let mut c = Self {
            local_socket: Some(DEFAULT_SOCKET.to_owned()),
            ssh: Vec::new(),
            refresh: Duration::from_secs(DEFAULT_REFRESH_SECS),
            once: false,
        };
        let mut it = args.into_iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--socket" => c.local_socket = Some(value(&mut it, "--socket")?),
                "--no-local" => c.local_socket = None,
                "--ssh" => c.ssh.push(parse_ssh(&value(&mut it, "--ssh")?)?),
                "--refresh" => c.refresh = parse_secs(&value(&mut it, "--refresh")?)?,
                "--once" => c.once = true,
                other => return Err(format!("unknown argument {other:?}")),
            }
        }
        if c.local_socket.is_none() && c.ssh.is_empty() {
            return Err("nothing to watch: --no-local needs at least one --ssh".to_owned());
        }
        Ok(c)
    }
}

fn value(it: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    it.next().ok_or_else(|| format!("{flag} needs a value"))
}

fn parse_ssh(v: &str) -> Result<SshTarget, String> {
    let (alias, socket) = v.split_once(':').unwrap_or((v, DEFAULT_SOCKET));
    if alias.is_empty() || socket.is_empty() {
        return Err(format!("bad --ssh value {v:?}"));
    }
    Ok(SshTarget {
        alias: alias.to_owned(),
        socket: socket.to_owned(),
    })
}

fn parse_secs(v: &str) -> Result<Duration, String> {
    match v.parse::<u64>() {
        Ok(s) if s > 0 => Ok(Duration::from_secs(s)),
        _ => Err(format!(
            "--refresh needs a positive number of seconds, got {v:?}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Config, String> {
        Config::parse(args.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn defaults_to_the_named_flight_socket_never_the_default_server() {
        let c = parse(&[]).unwrap();
        assert_eq!(c.local_socket.as_deref(), Some("flight"));
        assert!(c.ssh.is_empty() && !c.once);
    }

    #[test]
    fn ssh_targets_with_and_without_a_socket() {
        let c = parse(&["--ssh", "mini-2", "--ssh", "mini-3:agents"]).unwrap();
        assert_eq!(
            c.ssh,
            [
                SshTarget {
                    alias: "mini-2".into(),
                    socket: "flight".into()
                },
                SshTarget {
                    alias: "mini-3".into(),
                    socket: "agents".into()
                },
            ]
        );
    }

    #[test]
    fn rejects_bad_input() {
        for bad in [
            &["--bogus"][..],
            &["--ssh"],
            &["--ssh", ":x"],
            &["--refresh", "0"],
            &["--refresh", "x"],
            &["--no-local"],
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn remote_only_is_allowed() {
        let c = parse(&["--no-local", "--ssh", "mini-2"]).unwrap();
        assert_eq!(c.local_socket, None);
    }
}
