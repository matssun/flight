#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Compare ways of observing many tmux panes (`flight-load observe`).

For each pane count and strategy: create N real panes running a stand-in agent that redraws
its screen when a control file changes, churn a given percentage of the panes per second
(each change writes a new `rev N` marker and a timestamp), run the strategy for a while, and
report round time, detection latency, panes left stale after the churn stops, and CPU.

CPU is the sum of: the bench process and every subprocess it waited for (tmux clients),
measured with RUSAGE_CHILDREN, plus the tmux server process.

    observe_bench.py --agent PATH_TO_STAND_IN --flight-load PATH --panes 100,250 \
        --churn 5 --strategies seq/all,conc:8/all,seq/skip,ctl/all,ctl/skip
"""
import argparse, json, os, random, resource, subprocess, sys, threading, time

SOCKET = "obsbench"


def sh(cmd, check=False):
    return subprocess.run(cmd, shell=True, capture_output=True, text=True, check=check).stdout.strip()


def server_cpu():
    pids = sh(f"pgrep -f 'tmux.*-L {SOCKET}|tmux: server.*{SOCKET}'").split()
    total = 0.0
    for pid in pids:
        out = sh(f"ps -o cputime= -p {pid}")
        if out:
            p = out.split(":")
            total += float(p[-1]) + 60 * int(p[-2]) + (3600 * int(p[-3]) if len(p) > 2 else 0)
    return total


def make_panes(agent, workdir, n):
    """Create N panes (session ob1..obN); return pane ids in session order, or None on failure."""
    sh(f"tmux -L {SOCKET} kill-server 2>/dev/null")
    time.sleep(6)  # let the previous server's ptys be released (macOS caps them at 511)
    os.makedirs(workdir, exist_ok=True)
    for i in range(1, n + 1):
        write_rev(workdir, i, 0)
        sh(f"tmux -L {SOCKET} new-session -d -s ob{i} -c /tmp 'exec {agent} {workdir}/scr_ob{i}.txt'")
    time.sleep(2)
    ids = {}
    for line in sh(f"tmux -L {SOCKET} list-panes -a -F '#{{pane_id}} #{{session_name}}'").splitlines():
        pane, session = line.split()
        ids[int(session[2:]) - 1] = pane
    if len(ids) != n:
        print(f"could only create {len(ids)} of {n} panes (pty limit?)", flush=True)
        return None
    return [ids[i] for i in range(n)]


def write_rev(workdir, i, rev):
    tmp = f"{workdir}/.tmp_ob{i}"
    with open(tmp, "w") as f:
        f.write(f"rev {rev}\nDone!\n\n> \n")
    os.replace(tmp, f"{workdir}/scr_ob{i}.txt")


class Churn(threading.Thread):
    """Change `percent` of the panes per second, recording (rev, wall time) per pane."""

    def __init__(self, workdir, n, percent):
        super().__init__(daemon=True)
        self.workdir, self.n, self.percent = workdir, n, percent
        self.stop = threading.Event()
        self.rev = [0] * n
        self.writes = [[(0, time.time())] for _ in range(n)]

    def run(self):
        tick, rng = 0.1, random.Random(7)
        per_tick = self.n * self.percent / 100 * tick
        carry = 0.0
        while not self.stop.is_set():
            carry += per_tick
            for _ in range(int(carry)):
                i = rng.randrange(self.n)
                self.rev[i] += 1
                write_rev(self.workdir, i + 1, self.rev[i])
                self.writes[i].append((self.rev[i], time.time()))
            carry -= int(carry)
            time.sleep(tick)


def run_strategy(args, ids, capture, policy):
    n = len(ids)
    churn = Churn(args.workdir, n, args.churn)
    cmd = [args.flight_load, "observe", "--socket", SOCKET, "--capture", capture, "--policy", policy,
           "--interval-ms", str(args.interval), "--secs", str(args.secs), "--settle", "3"]
    r0, s0 = resource.getrusage(resource.RUSAGE_CHILDREN), server_cpu()
    t0 = time.time()
    if args.churn > 0:
        churn.start()
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, text=True)
    # Stop the churn when the timed part is over so the final rounds can settle.
    threading.Timer(args.secs, churn.stop.set).start()
    lines = [json.loads(l) for l in proc.stdout if l.startswith("{")]
    proc.wait()
    wall = time.time() - t0
    r1, s1 = resource.getrusage(resource.RUSAGE_CHILDREN), server_cpu()
    churn.stop.set()
    if proc.returncode != 0 or not lines:
        return None
    cpu_client = (r1.ru_utime + r1.ru_stime) - (r0.ru_utime + r0.ru_stime)
    took = sorted(l["ms"] for l in lines)
    pick = lambda q: took[min(len(took) - 1, int(q * len(took)))]
    # detection latency: first round whose observed rev >= the written rev
    lat = []
    for i in range(n):
        key = ids[i]
        for rev, tw in churn.writes[i][1:]:
            for l in lines:
                if l["revs"].get(key, -1) >= rev and l["t"] / 1000 >= tw:
                    lat.append((l["t"] / 1000 - tw) * 1000)
                    break
    lat.sort()
    lp = lambda q: lat[min(len(lat) - 1, int(q * len(lat)))] if lat else float("nan")
    last = lines[-1]["revs"]
    stale_idx = [i for i in range(n) if last.get(ids[i], -1) != churn.rev[i]]
    stale = len(stale_idx)
    if args.debug_stale:
        end_t = lines[-1]["t"] / 1000
        for i in stale_idx[:6]:
            seen = [l["revs"].get(ids[i], None) for l in lines[-5:]]
            recent = [(rev, round(end_t - tw, 2)) for rev, tw in churn.writes[i][-2:]]
            print(f"   stale {ids[i]}: expected rev {churn.rev[i]}, last 5 rounds saw {seen}, "
                  f"last writes (rev, seconds before last round) {recent}", flush=True)
    return {
        "round_p50": pick(0.5), "round_p95": pick(0.95), "round_max": took[-1],
        "captured_avg": sum(l["captured"] for l in lines) / len(lines),
        "lat_p50": lp(0.5), "lat_p95": lp(0.95), "lat_max": lat[-1] if lat else float("nan"),
        "changes": len(lat), "stale": stale,
        "cpu_client": 100 * cpu_client / wall, "cpu_server": 100 * (s1 - s0) / wall,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--agent", required=True)
    ap.add_argument("--flight-load", required=True)
    ap.add_argument("--workdir", default="/tmp/obsbench")
    ap.add_argument("--panes", default="100")
    ap.add_argument("--churn", type=float, default=5.0, help="percent of panes changing per second")
    ap.add_argument("--strategies", default="seq/all,conc:8/all,seq/skip,ctl/all,ctl/skip")
    ap.add_argument("--interval", type=int, default=1000)
    ap.add_argument("--secs", type=int, default=30)
    ap.add_argument("--debug-stale", action="store_true", help="explain panes that end stale")
    args = ap.parse_args()
    print(f"churn={args.churn}%/s interval={args.interval}ms secs={args.secs}")
    print(f"{'panes':>5} {'strategy':<14} {'round p50/p95/max ms':>22} {'caps/rd':>7} "
          f"{'detect p50/p95/max ms':>23} {'stale':>5} {'cpu% client+server':>19}")
    for n in [int(x) for x in args.panes.split(",")]:
        ids = make_panes(args.agent, args.workdir, n)
        if ids is None:
            continue
        got = len(ids)
        for spec in args.strategies.split(","):
            capture, policy = spec.split("/")
            r = run_strategy(args, ids, capture, policy)
            if r is None:
                print(f"{got:5d} {spec:<14} FAILED")
                continue
            print(f"{got:5d} {spec:<14} {r['round_p50']:7.0f}/{r['round_p95']:6.0f}/{r['round_max']:6.0f} "
                  f"{r['captured_avg']:7.1f} {r['lat_p50']:8.0f}/{r['lat_p95']:6.0f}/{r['lat_max']:6.0f} "
                  f"{r['stale']:5d} {r['cpu_client']:8.1f} + {r['cpu_server']:5.1f}", flush=True)
    sh(f"tmux -L {SOCKET} kill-server 2>/dev/null")


if __name__ == "__main__":
    sys.exit(main())
