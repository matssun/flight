// SPDX-License-Identifier: MIT

mod support;
use flight_workspaces::*;
use support::*;

fn go() -> RecoveryPolicy {
    RecoveryPolicy {
        start_missing: true,
        ..RecoveryPolicy::default()
    }
}

struct Rig {
    fake: Fake,
    roots: Roots,
    doc: Document,
}

fn rig(defs: Vec<WorkspaceDefinition>) -> Rig {
    let roots = Roots::default();
    for d in &defs {
        roots.set(&d.root.path, present(5));
    }
    Rig {
        fake: Fake::default(),
        roots,
        doc: doc_with(defs),
    }
}

impl Rig {
    fn recover(&mut self, policy: &RecoveryPolicy) -> RecoveryReport {
        let mut exec = self.fake.clone();
        recover(
            &mut self.doc,
            &["h"],
            &self.fake,
            &self.roots,
            &mut exec,
            policy,
        )
    }

    fn items(&self) -> Vec<Item> {
        plan(
            self.doc.active().unwrap(),
            &[("h".to_owned(), self.fake.observe("h"))]
                .into_iter()
                .collect(),
            &self.roots,
            &go(),
        )
        .items
    }
}

#[test]
fn a_saved_workspace_that_is_not_running_is_only_reported_unless_starting_is_allowed() {
    let mut r = rig(vec![def("a", "h", "/a")]);
    let report = r.recover(&RecoveryPolicy::default());
    assert_eq!(report.items[0].health, Health::Stopped);
    assert_eq!(report.items[0].refusals, vec![Refusal::PolicyDoesNotStart]);
    assert_eq!(r.fake.running("h"), 0);
}

#[test]
fn restoring_twice_starts_it_once() {
    let mut r = rig(vec![def("a", "h", "/a")]);
    r.recover(&go());
    r.recover(&go());
    r.recover(&go());
    assert_eq!(r.fake.running("h"), 1);
    assert_eq!(r.fake.0.borrow().started, vec!["a"]);
}

#[test]
fn a_lost_reply_does_not_cause_a_second_workspace() {
    let mut r = rig(vec![def("a", "h", "/a")]);
    r.fake.0.borrow_mut().lose_reply = true;
    let first = r.recover(&go());
    assert!(matches!(first.done[0].2, Outcome::Unknown(_)));
    // The saved binding was not recorded from the lost reply, but the mark the start left is found.
    assert_eq!(
        r.doc.active().unwrap().workspaces[0].last_workspace_id,
        None
    );
    let second = r.recover(&go());
    assert_eq!(r.fake.running("h"), 1);
    assert_eq!(r.fake.0.borrow().started.len(), 1);
    assert!(second
        .done
        .iter()
        .all(|(_, a, _)| matches!(a, Action::Bind { .. })));
    assert!(r.doc.active().unwrap().workspaces[0]
        .last_workspace_id
        .is_some());
}

#[test]
fn a_workspace_that_appeared_between_planning_and_acting_is_not_started_again() {
    let mut r = rig(vec![def("a", "h", "/a")]);
    // Someone else (another UI) started it first: running, marked with our key.
    let key = r.doc.active().unwrap().workspaces[0].key.clone();
    let surfaces = r.doc.active().unwrap().workspaces[0].surfaces.clone();
    r.fake
        .0
        .borrow_mut()
        .running
        .entry("h".into())
        .or_default()
        .push(ObservedWorkspace {
            workspace_id: "w-x".into(),
            config_key: Some(key),
            root: "/a".into(),
            surfaces: surfaces
                .iter()
                .map(|s| ObservedSurface {
                    surface_id: format!("s-{}", s.key),
                    config_key: Some(s.key.clone()),
                    kind: s.kind,
                })
                .collect(),
        });
    r.recover(&go());
    assert_eq!(r.fake.running("h"), 1);
    assert!(r.fake.0.borrow().started.is_empty());
}

#[test]
fn a_running_workspace_is_reconnected_by_identity_even_after_its_runtime_id_changed() {
    let mut d = def("a", "h", "/a");
    d.last_workspace_id = Some("w-old".to_owned());
    let mut r = rig(vec![d]);
    // After a tmux restart the workspace is the same root under a new runtime id, unmarked.
    r.fake
        .0
        .borrow_mut()
        .running
        .entry("h".into())
        .or_default()
        .push(ObservedWorkspace {
            workspace_id: "w-new".into(),
            config_key: None,
            root: "/a/".into(),
            surfaces: vec![
                ObservedSurface {
                    surface_id: "s1".into(),
                    config_key: None,
                    kind: SurfaceKind::Agent,
                },
                ObservedSurface {
                    surface_id: "s2".into(),
                    config_key: None,
                    kind: SurfaceKind::Shell,
                },
            ],
        });
    let report = r.recover(&go());
    assert_eq!(report.items[0].health, Health::Running);
    assert!(r.fake.0.borrow().started.is_empty());
    assert_eq!(
        r.doc.active().unwrap().workspaces[0]
            .last_workspace_id
            .as_deref(),
        Some("w-new")
    );
    assert!(r.recover(&go()).done.is_empty(), "converged");
}

#[test]
fn a_missing_surface_is_replaced_in_the_existing_workspace_once() {
    let mut r = rig(vec![def("a", "h", "/a")]);
    r.recover(&go());
    // The shell's window is closed; the agent keeps running.
    let shell = r.fake.0.borrow_mut().running.get_mut("h").unwrap()[0]
        .surfaces
        .pop()
        .unwrap();
    assert_eq!(shell.kind, SurfaceKind::Shell);
    let report = r.recover(&go());
    assert_eq!(report.items[0].health, Health::Partial);
    r.recover(&go());
    assert_eq!(r.fake.running("h"), 1);
    assert_eq!(r.fake.0.borrow().running["h"][0].surfaces.len(), 2);
    assert_eq!(r.fake.0.borrow().started, vec!["a", "a:Shell"]);
}

#[test]
fn an_unreachable_host_blocks_only_its_workspaces_and_keeps_their_definitions() {
    let mut r = rig(vec![def("a", "h", "/a"), def("b", "far", "/b")]);
    r.roots.set("/b", present(6));
    r.fake.host_up("far", false);
    let before = r.doc.clone();
    let report = r.recover(&go());
    let by_name = |n: &str| {
        let key = &r
            .doc
            .active()
            .unwrap()
            .workspaces
            .iter()
            .find(|w| w.name == n)
            .unwrap()
            .key;
        report.items.iter().find(|i| &i.key == key).unwrap().clone()
    };
    assert!(matches!(
        by_name("b").health,
        Health::Blocked(Blocker::HostUnreachable(_))
    ));
    assert!(by_name("b").actions.is_empty());
    assert_eq!(by_name("a").actions.len(), 1);
    assert_eq!(
        r.doc.active().unwrap().workspaces[1],
        before.active().unwrap().workspaces[1]
    );
    // The host returns: it is started then, once.
    r.fake.host_up("far", true);
    r.recover(&go());
    r.recover(&go());
    assert_eq!(r.fake.running("far"), 1);
}

#[test]
fn every_kind_of_unavailable_root_blocks_starting_and_keeps_the_definition() {
    let cases = [
        RootState::Missing,
        RootState::NotADirectory,
        RootState::PermissionDenied,
        RootState::Unverified {
            reason: "unmounted?".into(),
        },
    ];
    for state in cases {
        let mut r = rig(vec![def("a", "h", "/a")]);
        r.roots.set("/a", state.clone());
        let before = r.doc.clone();
        for _ in 0..3 {
            let report = r.recover(&go());
            assert_eq!(report.items[0].root, RootCheck::Unavailable(state.clone()));
            assert!(matches!(
                report.items[0].health,
                Health::Blocked(Blocker::Root(_))
            ));
            assert!(report.done.is_empty());
        }
        assert_eq!(r.doc, before, "repeated failures change nothing saved");
        assert_eq!(r.fake.running("h"), 0);
    }
}

#[test]
fn a_changed_root_is_not_started_in_until_the_user_accepts_it() {
    let mut r = rig(vec![def("a", "h", "/a")]);
    r.doc.active_mut().unwrap().workspaces[0]
        .root
        .record(&present(5));
    r.roots.set("/a", present(99));
    let report = r.recover(&go());
    assert!(matches!(
        report.items[0].health,
        Health::Blocked(Blocker::Root(RootCheck::Changed(_)))
    ));
    assert_eq!(r.fake.running("h"), 0);
    r.doc.active_mut().unwrap().workspaces[0]
        .root
        .record(&present(99));
    r.recover(&go());
    assert_eq!(r.fake.running("h"), 1);
}

#[test]
fn a_missing_root_that_reappears_is_started_after_it_is_verified() {
    let mut r = rig(vec![def("a", "h", "/a")]);
    r.roots.set("/a", RootState::Missing);
    r.recover(&go());
    assert_eq!(r.fake.running("h"), 0);
    r.roots.set("/a", present(5));
    r.recover(&go());
    assert_eq!(r.fake.running("h"), 1);
}

#[test]
fn removing_a_saved_workspace_forgets_the_reference_only_and_it_stays_forgotten() {
    let mut r = rig(vec![def("a", "h", "/a")]);
    r.recover(&go());
    let key = r.doc.active().unwrap().workspaces[0].key.clone();
    let removed = r.doc.active_mut().unwrap().remove(&key).unwrap();
    assert!(r.doc.active().unwrap().workspaces.is_empty());
    assert_eq!(r.fake.running("h"), 1, "the running workspace is untouched");
    let again = r.recover(&go());
    assert!(
        again.recorded.is_empty(),
        "a dismissed workspace is not re-recorded"
    );
    assert!(r.doc.active().unwrap().workspaces.is_empty());
    assert!(removed.last_workspace_id.is_some());
}

#[test]
fn a_running_workspace_nobody_saved_is_recorded_once() {
    let mut r = rig(vec![def("anchor", "h", "/anchor")]);
    r.roots.set("/anchor", present(1));
    r.fake
        .0
        .borrow_mut()
        .running
        .entry("h".into())
        .or_default()
        .push(ObservedWorkspace {
            workspace_id: "legacy.h.work.3".into(),
            config_key: None,
            root: "/work/nga".into(),
            surfaces: vec![ObservedSurface {
                surface_id: "s".into(),
                config_key: None,
                kind: SurfaceKind::Shell,
            }],
        });
    let first = r.recover(&RecoveryPolicy::default());
    assert_eq!(first.recorded, vec!["legacy.h.work.3"]);
    let second = r.recover(&RecoveryPolicy::default());
    assert!(second.recorded.is_empty());
    assert_eq!(r.doc.active().unwrap().workspaces.len(), 2);
}

#[test]
fn two_candidates_are_ambiguous_and_nothing_is_guessed() {
    let mut r = rig(vec![def("a", "h", "/a")]);
    for id in ["w-1", "w-2"] {
        r.fake
            .0
            .borrow_mut()
            .running
            .entry("h".into())
            .or_default()
            .push(ObservedWorkspace {
                workspace_id: id.into(),
                config_key: None,
                root: "/a".into(),
                surfaces: vec![],
            });
    }
    let report = r.recover(&go());
    assert!(matches!(
        report.items[0].health,
        Health::Blocked(Blocker::Ambiguous(_))
    ));
    assert!(report.done.is_empty() && report.recorded.is_empty());
    assert_eq!(r.fake.running("h"), 2);
}

#[test]
fn trust_gates_imported_definitions_and_unprompted_agents() {
    let mut imported = def("imp", "h", "/i");
    imported.origin = Origin::Imported;
    let mut risky = def("risky", "h", "/r");
    risky.surfaces[0].skip_permissions = true;
    let mut r = rig(vec![imported, risky]);
    let report = r.recover(&go());
    assert_eq!(report.items[0].refusals, vec![Refusal::ImportedNotTrusted]);
    assert_eq!(
        report.items[1].refusals,
        vec![Refusal::SkipPermissionsNotTrusted]
    );
    assert_eq!(r.fake.running("h"), 0);
    let allow = RecoveryPolicy {
        start_imported: true,
        start_skip_permissions: true,
        ..go()
    };
    r.recover(&allow);
    assert_eq!(r.fake.running("h"), 2);
}

#[test]
fn importing_drops_runtime_state_and_marks_everything_untrusted() {
    let mut d = def("a", "h", "/a");
    d.last_workspace_id = Some("w-1".into());
    d.root.record(&present(3));
    let mut p = Profile::new("shared");
    p.workspaces.push(d);
    let imported = p.into_imported();
    let w = &imported.workspaces[0];
    assert_eq!(
        (w.origin, w.last_workspace_id.clone(), w.root.identity),
        (Origin::Imported, None, None)
    );
}

#[test]
fn resumption_is_distinct_from_replacement_and_only_for_capable_providers() {
    let mut r = rig(vec![def("a", "h", "/a")]);
    r.recover(&go());
    r.fake.0.borrow_mut().running.get_mut("h").unwrap()[0]
        .surfaces
        .remove(0);
    let policy = RecoveryPolicy {
        resumable_providers: vec!["claude".into()],
        ..go()
    };
    let item = &r.items();
    assert!(
        matches!(item[0].actions[0], Action::StartSurface { .. }),
        "no resumption without the policy"
    );
    let planned = plan(
        r.doc.active().unwrap(),
        &[("h".to_owned(), r.fake.observe("h"))]
            .into_iter()
            .collect(),
        &r.roots,
        &policy,
    );
    assert!(matches!(
        planned.items[0].actions[0],
        Action::ResumeAgent { .. }
    ));
}

#[test]
fn changing_the_root_keeps_the_identity_and_reverifies() {
    let mut r = rig(vec![def("a", "h", "/a")]);
    let key = r.doc.active().unwrap().workspaces[0].key.clone();
    r.doc.active_mut().unwrap().workspaces[0]
        .root
        .record(&present(5));
    assert!(r.doc.active_mut().unwrap().set_root(&key, "/moved"));
    let w = &r.doc.active().unwrap().workspaces[0];
    assert_eq!(
        (w.key.clone(), w.root.path.as_str(), w.root.identity),
        (key, "/moved", None)
    );
}

#[test]
fn a_hand_made_workspace_is_recorded_even_when_nothing_is_saved_yet() {
    let mut r = rig(vec![]);
    r.fake
        .0
        .borrow_mut()
        .running
        .entry("h".into())
        .or_default()
        .push(ObservedWorkspace {
            workspace_id: "w-hand".into(),
            config_key: None,
            root: "/hand".into(),
            surfaces: vec![],
        });
    let report = r.recover(&RecoveryPolicy::default());
    assert_eq!(report.recorded, vec!["w-hand"]);
    assert_eq!(r.doc.active().unwrap().workspaces.len(), 1);
}
