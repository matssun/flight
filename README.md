<!-- SPDX-License-Identifier: MIT -->

# Flight

A tmux session manager with an agent dashboard, in Rust. Session sources and connect rules follow sesh; the agent-state dashboard follows Fleet.

Status: v0. A ratatui dashboard over one or more tmux servers (local, and remote over SSH).

    flight                         # watch the local tmux server on socket 'flight' (tmux -L flight)
    flight --ssh mini-2            # also watch a remote host via an ssh alias
    flight --once                  # print one frame as text and exit

Flight never assumes your default tmux server: run your agents on `tmux -L flight`. Keys: arrows or j/k, Enter, Tab, r, q.
