# Running a Flight node as a service (macOS launchd)

`flight node run` is infrastructure. Run it from the OS service manager, not from a login
shell, an ssh session or tmux: a service outlives sessions, restarts on its own, has
predictable logs and does not inherit interactive-session state.

Why this matters on macOS (observed, see ADR-002 "LAN experiment"): after a Wi-Fi bounce, a
Rust process started inside a tmux server that predates the bounce could not route to its
previous LAN peer for as long as it was watched, while the same binary started from a fresh
login context connected at once (and Python in the same tmux server was unaffected). The
node therefore has an opt-in `--exit-after-link-down SECS` (not in the standard plist: a node run by launchd recovered from every Wi-Fi bounce without it, 6 of 6): after an unbroken window of only
immediate "no route" failures it exits with status 75, and the service manager restarts it.
That restart is safe: a new incarnation sends a full snapshot, tmux and the agents are
untouched. A restart loop inside tmux or a shell does **not** help (it is in the affected
session); the restarter must be launchd/systemd.

## Install (you do this; Flight never installs background jobs)

```sh
# 1. fill in the template
FLIGHT_BIN=$HOME/flight/flight           # absolute path of the binary
NODE_CONFIG_DIR=$HOME/flight/node        # the dir `flight node join --config-dir` wrote
LOG_DIR=$HOME/Library/Logs/flight
mkdir -p "$LOG_DIR" ~/Library/LaunchAgents
sed -e "s|@FLIGHT_BIN@|$FLIGHT_BIN|" -e "s|@NODE_CONFIG_DIR@|$NODE_CONFIG_DIR|" \
    -e "s|@LOG_DIR@|$LOG_DIR|" contrib/launchd/name.flight.node.plist \
    > ~/Library/LaunchAgents/name.flight.node.plist
plutil -lint ~/Library/LaunchAgents/name.flight.node.plist

# 2. stop any node started by hand (two nodes with one identity supersede each other)
pkill -f "flight node run" ; tmux -L lan kill-server 2>/dev/null   # whatever you used

# 3. load and start
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/name.flight.node.plist
launchctl print gui/$(id -u)/name.flight.node | grep -E "state|pid|last exit"
tail -f "$LOG_DIR/node.out.log"          # should say: connected to <orchestrator>
```

Remove: `launchctl bootout gui/$(id -u)/name.flight.node && rm ~/Library/LaunchAgents/name.flight.node.plist`.

A per-user LaunchAgent runs while that user is logged in (including a locked screen). For a
headless machine that must run at boot without login, use a LaunchDaemon instead (root-owned
plist in /Library/LaunchDaemons with a `UserName` key); that needs its own review.

## The recovery experiment

Expected sequence with the service installed (Wi-Fi off for 40 s, then on):

```
node Online -> Wi-Fi off -> orchestrator: Stale (~5 s), Disconnected (~9 s)
-> Wi-Fi on -> node keeps failing with "no route" -> after 60 s (+0-25%) it logs
   "this process has had no route ... exiting to be restarted" and exits 75
-> launchd starts a fresh process -> "connected to <orchestrator>" -> Online
```

Record the pid before and after, to show that recovery came from a fresh process:

```sh
P0=$(launchctl print gui/$(id -u)/name.flight.node | awk '/pid =/{print $3}'); echo before=$P0
# ... toggle Wi-Fi off, wait 40 s, on, wait until the node is Online again ...
P1=$(launchctl print gui/$(id -u)/name.flight.node | awk '/pid =/{print $3}'); echo after=$P1
launchctl print gui/$(id -u)/name.flight.node | grep -E "runs|last exit"
```

A pass is `after` != `before`, `last exit code = 75`, and the orchestrator showing the node
`Online` again, repeated five times. If the service restart does not cure it, record that
too: it would mean the failure is tied to something broader than the process's launch
session.
