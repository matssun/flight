// SPDX-License-Identifier: MIT

//! Saved workspaces on the wire (ADR-008): compatible with peers that predate them, bounded,
//! and refused when malformed.

use flight_proto::{
    capability, delta_change, Delta, Reject, SavedHealthCode, SavedRootCode, SavedWorkspace,
    SavedWorkspaces, Snapshot, Validate, MAX_DETAIL_LEN, MAX_SAVED,
};
use prost::Message;

fn good() -> SavedWorkspace {
    SavedWorkspace {
        config_key: "c-0123456789abcdef".to_owned(),
        name: "nga".to_owned(),
        root: "/work/nga".to_owned(),
        health: SavedHealthCode::Stopped as i32,
        root_state: SavedRootCode::Missing as i32,
        detail: "gone".to_owned(),
        workspace_id: String::new(),
        imported: false,
    }
}

fn inc() -> Vec<u8> {
    vec![7; 16]
}

#[test]
fn a_snapshot_from_a_node_that_predates_saved_workspaces_decodes_with_none() {
    // Fields 1 to 3 only, as an older node encodes it.
    let old = Snapshot {
        incarnation: inc(),
        panes: vec![],
        servers: vec![],
        saved: vec![],
    };
    let bytes = old.encode_to_vec();
    let back = Snapshot::decode(bytes.as_slice()).unwrap();
    assert!(back.saved.is_empty());
    back.validate().unwrap();
}

#[test]
fn an_old_peer_ignores_the_new_snapshot_field() {
    // Field 4 of a Snapshot is skipped by a decoder that does not know it: re-encode the
    // message without the field and compare what an old decoder would keep.
    let new = Snapshot {
        incarnation: inc(),
        panes: vec![],
        servers: vec![],
        saved: vec![good()],
    };
    let mut bytes = new.encode_to_vec();
    assert!(!bytes.is_empty());
    #[derive(Clone, PartialEq, prost::Message)]
    struct OldSnapshot {
        #[prost(bytes = "vec", tag = "1")]
        incarnation: Vec<u8>,
    }
    let old = OldSnapshot::decode(bytes.as_slice()).unwrap();
    assert_eq!(old.incarnation, inc());
    bytes.clear();
}

#[test]
fn a_well_formed_report_validates_and_round_trips() {
    let delta = Delta {
        incarnation: inc(),
        sequence: 1,
        change: Some(delta_change::Change::Saved(SavedWorkspaces {
            items: vec![good()],
        })),
    };
    delta.validate().unwrap();
    assert_eq!(
        Delta::decode(delta.encode_to_vec().as_slice()).unwrap(),
        delta
    );
}

#[test]
fn malformed_entries_are_refused() {
    type Break = Box<dyn Fn(&mut SavedWorkspace)>;
    let cases: Vec<(&str, Break)> = vec![
        ("key", Box::new(|s| s.config_key = "not valid".into())),
        ("empty key", Box::new(|s| s.config_key.clear())),
        ("name", Box::new(|s| s.name.clear())),
        ("long name", Box::new(|s| s.name = "n".repeat(65))),
        (
            "root",
            Box::new(|s| s.root = "r".repeat(flight_proto::MAX_DIR_LEN + 1)),
        ),
        (
            "detail",
            Box::new(|s| s.detail = "d".repeat(MAX_DETAIL_LEN + 1)),
        ),
        (
            "runtime id",
            Box::new(|s| s.workspace_id = "has space".into()),
        ),
        ("health 0", Box::new(|s| s.health = 0)),
        ("health 99", Box::new(|s| s.health = 99)),
        ("root state 0", Box::new(|s| s.root_state = 0)),
        ("root state 99", Box::new(|s| s.root_state = 99)),
    ];
    for (what, break_it) in cases {
        let mut s = good();
        break_it(&mut s);
        let r: Result<(), Reject> = s.validate();
        assert!(r.is_err(), "{what} should be refused");
    }
}

#[test]
fn the_list_is_bounded() {
    let many = SavedWorkspaces {
        items: vec![good(); MAX_SAVED + 1],
    };
    assert!(many.validate().is_err());
    let ok = SavedWorkspaces {
        items: vec![good(); MAX_SAVED],
    };
    assert!(ok.validate().is_ok());
}

#[test]
fn the_capability_is_known_and_negotiated_away_by_an_old_orchestrator() {
    assert!(capability::KNOWN.contains(&"saved_workspaces_v1"));
    let offered = vec!["saved_workspaces_v1".to_owned(), "terminal_v1".to_owned()];
    let old_orchestrator_knows = ["terminal_v1"];
    assert_eq!(
        capability::negotiate(&offered, &old_orchestrator_knows),
        vec!["terminal_v1".to_owned()]
    );
}

mod saved_action {
    use flight_proto::{command_kind::Kind, command_kind::SavedAction, Command, SavedActionCode};
    use flight_proto::{Reject, Validate};

    fn command(action: SavedActionCode, root: &str) -> Command {
        Command {
            kind: Some(Kind::SavedAction(SavedAction {
                host: "node-a".to_owned(),
                config_key: "c-0123456789abcdef".to_owned(),
                action: action as i32,
                root: root.to_owned(),
            })),
        }
    }

    #[test]
    fn it_names_its_host_and_is_valid_for_every_action() {
        for a in [
            SavedActionCode::Retry,
            SavedActionCode::Remove,
            SavedActionCode::Restore,
            SavedActionCode::AcceptRoot,
            SavedActionCode::Trust,
        ] {
            let c = command(a, "");
            assert_eq!(c.target_host(), Some("node-a"));
            c.validate().unwrap();
        }
        command(SavedActionCode::SetRoot, "~/dev/nga")
            .validate()
            .unwrap();
    }

    #[test]
    fn a_root_goes_only_with_set_root_and_must_be_a_directory_path() {
        let bad: Vec<(SavedActionCode, &str)> = vec![
            (SavedActionCode::SetRoot, ""),
            (SavedActionCode::SetRoot, "relative/dir"),
            (SavedActionCode::SetRoot, "/bad\ndir"),
            (SavedActionCode::Restore, "/surprise"),
            (SavedActionCode::Remove, "/surprise"),
        ];
        for (a, root) in bad {
            let r: Result<(), Reject> = command(a, root).validate();
            assert!(r.is_err(), "{a:?} {root:?}");
        }
    }

    #[test]
    fn unknown_or_unspecified_actions_and_bad_keys_are_refused() {
        for action in [0, 99] {
            let mut c = command(SavedActionCode::Retry, "");
            if let Some(Kind::SavedAction(s)) = c.kind.as_mut() {
                s.action = action;
            }
            assert!(c.validate().is_err());
        }
        let mut c = command(SavedActionCode::Retry, "");
        if let Some(Kind::SavedAction(s)) = c.kind.as_mut() {
            s.config_key = "has space".to_owned();
        }
        assert!(c.validate().is_err());
        let mut c = command(SavedActionCode::Retry, "");
        if let Some(Kind::SavedAction(s)) = c.kind.as_mut() {
            s.host.clear();
        }
        assert!(c.validate().is_err());
    }
}
