// SPDX-License-Identifier: MIT

//! `ControlSkipObserver` against an in-memory tmux. The sequential observer over the same
//! world is the oracle: whatever the world does, both must report the same thing, and the
//! control path must do it with fewer commands and fall back safely when its channel breaks.

use flight_node::{
    ControlLink, ControlSkipObserver, PaneObserver, Round, SequentialObserver, ServerOutcome,
    TmuxServers, Unavailable,
};
use flight_state::ServerId;
use flight_tmux::{ControlReply, TmuxError, TmuxOutput, TmuxRunner};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

const OWN_PID: u32 = 4242;

#[derive(Clone)]
struct Pane {
    id: String,
    session: String,
    pid: u32,
    command: String,
    title: String,
    activity: u64,
    screen: String,
}

#[derive(Default)]
struct World {
    panes: Vec<Pane>,
    no_server: bool,
    link_dead: bool,
    connect_fails: bool,
    /// Whether our control client shows in `session_attached` (it does in real tmux).
    control_visible: bool,
    control_attached: bool,
    real_clients: Vec<String>,
    fail_capture: BTreeSet<String>,
    list_commands: usize,
    captures: Vec<String>,
    connects: usize,
}

type Shared = Arc<Mutex<World>>;

fn pane(id: &str, session: &str, pid: u32, screen: &str, activity: u64) -> Pane {
    Pane {
        id: id.into(),
        session: session.into(),
        pid,
        command: "claude".into(),
        title: "t".into(),
        activity,
        screen: screen.into(),
    }
}

impl World {
    fn attached(&self, session: &str) -> usize {
        self.real_clients.iter().filter(|s| *s == session).count()
            + usize::from(self.control_visible && self.control_attached && session == "work")
    }

    fn rows(&self) -> String {
        self.panes
            .iter()
            .map(|p| {
                format!(
                    "{}\t{}\tw\t@1\t0\t/tmp\t{}\t1\t1\t{}\t{}\t{}\t0\t\t\t\t/tmp\t$1\t{}\n",
                    p.id,
                    p.session,
                    p.pid,
                    self.attached(&p.session),
                    p.command,
                    p.activity,
                    p.title
                )
            })
            .collect()
    }
}

struct Runner(Shared);

impl TmuxRunner for Runner {
    fn run(&self, args: &[&str]) -> Result<TmuxOutput, TmuxError> {
        let w = self.0.lock().unwrap();
        if w.no_server {
            return Err(TmuxError::Failed {
                code: Some(1),
                stderr: "no server running on /tmp/x".into(),
            });
        }
        match args {
            ["list-panes", ..] => Ok(TmuxOutput { stdout: w.rows() }),
            ["capture-pane", rest @ ..] => {
                let id = rest.iter().position(|a| *a == "-t").map(|i| rest[i + 1]);
                match w.panes.iter().find(|p| Some(p.id.as_str()) == id) {
                    Some(p) if !w.fail_capture.contains(&p.id) => Ok(TmuxOutput {
                        stdout: p.screen.clone(),
                    }),
                    _ => Err(TmuxError::Failed {
                        code: Some(1),
                        stderr: "can't find pane".into(),
                    }),
                }
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

struct Link(Shared);

fn reply(lines: &str, ok: bool) -> ControlReply {
    ControlReply {
        lines: lines.lines().map(str::to_owned).collect(),
        ok,
    }
}

impl ControlLink for Link {
    fn run(&mut self, commands: &[String]) -> Result<Vec<ControlReply>, TmuxError> {
        let mut w = self.0.lock().unwrap();
        if w.link_dead || w.no_server {
            return Err(TmuxError::Control("connection closed".into()));
        }
        Ok(commands
            .iter()
            .map(|c| {
                if c.starts_with("list-panes") {
                    w.list_commands += 1;
                    reply(&w.rows(), true)
                } else if c.starts_with("list-clients") {
                    let mut lines = format!("{OWN_PID}\twork\n");
                    for s in &w.real_clients {
                        lines.push_str(&format!("9\t{s}\n"));
                    }
                    reply(&lines, true)
                } else {
                    let id = c.split(' ').nth(3).unwrap_or_default().to_owned();
                    w.captures.push(id.clone());
                    match w.panes.iter().find(|p| p.id == id) {
                        Some(p) if !w.fail_capture.contains(&id) => reply(&p.screen, true),
                        _ => reply("can't find pane", false),
                    }
                }
            })
            .collect())
    }

    fn client_pid(&self) -> u32 {
        OWN_PID
    }
}

fn server() -> ServerId {
    ServerId::new("flight")
}

struct Rig {
    world: Shared,
    sequential: SequentialObserver,
    control: ControlSkipObserver,
}

fn rig(panes: Vec<Pane>) -> Rig {
    let world: Shared = Arc::new(Mutex::new(World {
        panes,
        ..World::default()
    }));
    let mut servers = TmuxServers::new();
    servers.add(server(), Box::new(Runner(world.clone())));
    let servers = Arc::new(servers);
    let mut control = ControlSkipObserver::new(servers.clone());
    let w = world.clone();
    control.watch(server(), move || {
        let mut g = w.lock().unwrap();
        g.connects += 1;
        if g.connect_fails || g.no_server {
            return Err(TmuxError::Control("cannot attach".into()));
        }
        g.control_attached = true;
        Ok(Box::new(Link(w.clone())) as Box<dyn ControlLink>)
    });
    Rig {
        world,
        sequential: SequentialObserver::new(servers),
        control,
    }
}

impl Rig {
    /// Observe on both paths and require them to agree; returns the control path's round.
    fn step(&mut self, now: u64) -> Round {
        let want = self.sequential.observe(now);
        let got = self.control.observe(now);
        assert_eq!(
            got, want,
            "control path disagrees with the reference at {now}"
        );
        got.into_iter().next().unwrap()
    }

    fn captures(&self) -> usize {
        self.world.lock().unwrap().captures.len()
    }

    fn with<R>(&self, f: impl FnOnce(&mut World) -> R) -> R {
        f(&mut self.world.lock().unwrap())
    }
}

fn observed(r: &Round) -> &[flight_node::PaneObservation] {
    match &r.outcome {
        ServerOutcome::Observed(p) => p,
        other => panic!("not observed: {other:?}"),
    }
}

#[test]
fn unchanged_panes_are_not_captured_again() {
    let mut r = rig(vec![
        pane("%1", "work", 11, "one", 100),
        pane("%2", "work", 12, "two", 100),
    ]);
    r.step(200);
    assert_eq!(r.captures(), 2);
    r.step(201);
    r.step(202);
    assert_eq!(r.captures(), 2, "an unchanged fleet needs no captures");
}

#[test]
fn only_the_pane_whose_window_was_active_is_captured() {
    let mut r = rig(vec![
        pane("%1", "work", 11, "one", 100),
        pane("%2", "work", 12, "two", 100),
    ]);
    r.step(200);
    r.with(|w| {
        w.panes[1].screen = "two!".into();
        w.panes[1].activity = 205;
    });
    let round = r.step(206);
    assert_eq!(r.captures(), 3);
    assert_eq!(observed(&round)[1].screen_lines, vec!["two!".to_owned()]);
}

#[test]
fn activity_in_the_second_of_the_last_capture_is_captured_again() {
    // Output after our capture, in the same second, leaves the activity value unchanged.
    let mut r = rig(vec![pane("%1", "work", 11, "before", 200)]);
    r.step(200);
    r.with(|w| w.panes[0].screen = "after".into());
    let round = r.step(201);
    assert_eq!(observed(&round)[0].screen_lines, vec!["after".to_owned()]);
}

#[test]
fn a_new_process_command_or_title_is_captured_even_if_activity_looks_old() {
    let mut r = rig(vec![pane("%1", "work", 11, "a", 100)]);
    r.step(200);
    r.with(|w| {
        w.panes[0].pid = 99;
        w.panes[0].screen = "new process".into();
    });
    assert_eq!(
        observed(&r.step(201))[0].screen_lines,
        vec!["new process".to_owned()]
    );
    r.with(|w| {
        w.panes[0].title = "changed".into();
        w.panes[0].screen = "new title".into();
    });
    assert_eq!(
        observed(&r.step(202))[0].screen_lines,
        vec!["new title".to_owned()]
    );
}

#[test]
fn a_screen_is_never_kept_beyond_the_age_bound_or_across_a_clock_step_back() {
    // Not compared with the reference: the point is how long the control path may lag a
    // change that tmux gave no activity signal for.
    let mut r = rig(vec![pane("%1", "work", 11, "old", 100)]);
    let seen = |r: &mut Rig, now| {
        observed(&r.control.observe(now)[0]).to_vec()[0]
            .screen_lines
            .clone()
    };
    assert_eq!(seen(&mut r, 200), vec!["old".to_owned()]);
    r.with(|w| w.panes[0].screen = "silent change".into());
    assert_eq!(seen(&mut r, 210), vec!["old".to_owned()]);
    assert_eq!(seen(&mut r, 231), vec!["silent change".to_owned()]);
    r.with(|w| w.panes[0].screen = "after step back".into());
    assert_eq!(seen(&mut r, 50), vec!["after step back".to_owned()]);
}

#[test]
fn a_failed_capture_keeps_the_pane_without_evidence_and_is_retried() {
    let mut r = rig(vec![pane("%1", "work", 11, "x", 100)]);
    r.with(|w| {
        w.fail_capture.insert("%1".into());
    });
    assert!(observed(&r.step(200))[0].screen_lines.is_empty());
    r.with(|w| w.fail_capture.clear());
    assert_eq!(observed(&r.step(201))[0].screen_lines, vec!["x".to_owned()]);
}

#[test]
fn panes_created_and_removed_are_followed() {
    let mut r = rig(vec![pane("%1", "work", 11, "one", 100)]);
    r.step(200);
    r.with(|w| w.panes.push(pane("%2", "work", 12, "two", 210)));
    assert_eq!(observed(&r.step(211)).len(), 2);
    r.with(|w| w.panes.remove(0));
    let round = r.step(212);
    assert_eq!(observed(&round).len(), 1);
    // The removed id comes back with another process: nothing stale is reused.
    r.with(|w| w.panes.push(pane("%1", "work", 77, "reborn", 100)));
    assert_eq!(
        observed(&r.step(213))
            .iter()
            .find(|p| p.pane.as_str() == "%1")
            .unwrap()
            .screen_lines,
        vec!["reborn".to_owned()]
    );
}

#[test]
fn our_own_control_client_does_not_make_a_pane_focused() {
    let mut r = rig(vec![pane("%1", "work", 11, "x", 100)]);
    r.with(|w| w.control_visible = true);
    let round = r.control.observe(200).remove(0);
    assert!(!observed(&round)[0].focused, "tmux counts our own client");
    // A person attached to the same session still counts.
    r.with(|w| w.real_clients.push("work".into()));
    let round = r.control.observe(201).remove(0);
    assert!(observed(&round)[0].focused);
}

#[test]
fn a_broken_connection_falls_back_then_recovers_with_a_full_refresh() {
    let mut r = rig(vec![
        pane("%1", "work", 11, "one", 100),
        pane("%2", "work", 12, "two", 100),
    ]);
    r.step(200);
    r.step(201);
    assert_eq!(r.captures(), 2);

    r.with(|w| w.link_dead = true);
    // The round still carries a correct answer (from the sequential path)...
    r.with(|w| w.panes[0].screen = "changed while down".into());
    let round = r.step(202);
    assert_eq!(
        observed(&round)[0].screen_lines,
        vec!["changed while down".to_owned()]
    );
    let notes = r.control.take_notes();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("unavailable"), "{notes:?}");

    // ...and when the channel is back, everything is captured again before anything is skipped.
    r.with(|w| w.link_dead = false);
    let before = r.captures();
    r.step(203);
    assert_eq!(r.captures() - before, 2, "no pre-error belief may survive");
    let notes = r.control.take_notes();
    assert!(notes.iter().any(|n| n.contains("restored")), "{notes:?}");
    r.step(204);
    assert_eq!(r.captures() - before, 2);
}

#[test]
fn a_server_that_cannot_be_attached_is_observed_sequentially_without_hammering() {
    let mut r = rig(vec![pane("%1", "work", 11, "x", 100)]);
    r.with(|w| w.connect_fails = true);
    for t in 200..210 {
        r.step(t);
    }
    // One attempt, then a back-off: ten rounds did not mean ten attach attempts.
    assert_eq!(r.with(|w| w.connects), 1);
}

#[test]
fn a_dead_server_is_reported_as_no_server_not_as_an_empty_one() {
    let mut r = rig(vec![pane("%1", "work", 11, "x", 100)]);
    r.step(200);
    r.with(|w| w.no_server = true);
    let round = r.step(201);
    assert_eq!(
        round.outcome,
        ServerOutcome::Unavailable(Unavailable::NoServer)
    );
    r.with(|w| w.no_server = false);
    // The reference path answers immediately; the control path re-attaches after its back-off,
    // and in the meantime still agrees with it.
    assert_eq!(observed(&r.step(202)).len(), 1);
}

/// A deterministic sequence of everything a server can do; control and reference must agree
/// at every step.
#[test]
fn random_histories_agree_with_the_reference() {
    for seed in 1..=20u64 {
        let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut next = move |n: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % n
        };
        let mut r = rig(vec![
            pane("%1", "work", 11, "a", 100),
            pane("%2", "work", 12, "b", 100),
            pane("%3", "other", 13, "c", 100),
        ]);
        let mut now = 200;
        let mut next_id = 4;
        for step in 0..120 {
            let op = next(12);
            r.with(|w| {
                let n = w.panes.len().max(1) as u64;
                let i = next(n) as usize;
                match op {
                    0..=2 if !w.panes.is_empty() => {
                        w.panes[i].screen = format!("screen {step}");
                        w.panes[i].activity = now;
                    }
                    3 if !w.panes.is_empty() => w.panes[i].title = format!("title {step}"),
                    4 if !w.panes.is_empty() => {
                        w.panes[i].pid += 1000;
                        w.panes[i].screen = format!("reborn {step}");
                    }
                    5 => {
                        w.panes.push(pane(
                            &format!("%{next_id}"),
                            "work",
                            50 + next_id,
                            "new",
                            now,
                        ));
                        next_id += 1;
                    }
                    6 if !w.panes.is_empty() => {
                        w.panes.remove(i);
                    }
                    7 => w.link_dead = true,
                    8 => w.link_dead = false,
                    // A capture that starts failing matters when the pane is captured, which
                    // is when it changed; the reference cannot know it was skipped.
                    9 if !w.panes.is_empty() => {
                        w.panes[i].activity = now;
                        w.fail_capture = BTreeSet::from([w.panes[i].id.clone()]);
                    }
                    10 => w.fail_capture.clear(),
                    _ => {}
                }
            });
            now += next(4);
            if next(10) == 0 {
                now += 31;
            }
            r.step(now);
        }
    }
}
