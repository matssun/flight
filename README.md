<!-- SPDX-License-Identifier: MIT -->

# Flight

A session manager with an agent dashboard, in Rust. tmux is the internal backend that keeps sessions alive; you never operate it. Session sources and connect rules follow sesh; the agent-state dashboard follows Fleet.

## Try it

    cargo install --path flight    # or: cargo build, then target/debug/flight
    flight

That is all. The first run sets Flight up on this machine (about a second), then shows the dashboard; quitting stops what it started, and your sessions keep running and are there next time. Press `n` to start Claude or a shell.

Older, tmux-reading mode (no creating sessions): `flight --once`, `flight --ssh ALIAS`, `flight --socket NAME`.

Create sessions from the dashboard: press `n`, choose the machine and directory, pick Claude or Shell, Create. Enter opens a session; `Ctrl-Space` then `q` returns to the dashboard and the session keeps running. Flight uses its own private tmux server and never touches your default one. Keys: arrows or j/k, Enter open, n new, / search, ? help, r refresh, q quit; the mouse selects (second click opens) and scrolls.

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
