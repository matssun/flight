// SPDX-License-Identifier: MIT

/// How Flight reaches a host's tmux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transport {
    /// This machine: run `tmux` directly.
    Local,
    /// Another machine via OpenSSH. `alias` is a host alias for `ssh`; `~/.ssh/config` owns
    /// keys, hostnames, ProxyJump, ControlMaster, host verification and usernames.
    Ssh { alias: String },
}
