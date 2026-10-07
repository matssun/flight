<!-- SPDX-License-Identifier: MIT -->

# Flight

A tmux session manager with an agent dashboard, in Rust. Session sources and connect rules follow sesh; the agent-state dashboard follows Fleet.

Status: v0. A ratatui dashboard over one or more tmux servers (local, and remote over SSH).

    flight                         # watch the local tmux server on socket 'flight' (tmux -L flight)
    flight --ssh mini-2            # also watch a remote host via an ssh alias
    flight --once                  # print one frame as text and exit

Flight never assumes your default tmux server: run your agents on `tmux -L flight`. Keys: arrows or j/k, Enter, Tab, r, q.

## Distributed mode

Run `flight node run` as a service (launchd/systemd), not from a shell or tmux: see `contrib/launchd/README.md` for why and how.


One machine runs the orchestrator, every machine with agents runs a node, and any machine can show the dashboard. Everything is mutually authenticated (TLS 1.3, identities pinned by key fingerprint); nodes dial the orchestrator, so the orchestrator may live on this machine, on the LAN, or hosted, and the dashboard cannot tell which.

    # on the orchestrator machine
    flight orchestrator run --listen 0.0.0.0:7676 --advertise 192.168.1.20:7676
    flight orchestrator enrollment create          # prints a one-time bundle

    # on each machine with agents
    flight node join <bundle> --name mini-2        # enroll (leaves nothing behind if it fails)
    flight node run                                # observe tmux -L flight and report

    # wherever you want the dashboard
    flight orchestrator enrollment create          # a fresh bundle
    flight ui join <bundle> --name laptop
    flight ui                                      # or: flight ui --once

    flight orchestrator trust list                 # who is trusted
    flight orchestrator trust revoke <id>          # drop a machine at once

Settings live under `~/.config/flight/{orchestrator,node,ui}/` (override with `--config-dir`); each role has its own identity. See `docs/adr/ADR-002-node-orchestrator.md`.
