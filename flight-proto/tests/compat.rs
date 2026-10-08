// SPDX-License-Identifier: MIT

mod support;

use flight_proto::codec::{decode, encode, MAX_FRAME_BYTES};
use flight_proto::*;
use support::*;

fn hello_frame() -> NodeFrame {
    NodeFrame {
        body: Some(node_body::Body::Hello(node_hello())),
    }
}

#[test]
fn major_mismatch_is_rejected_and_minor_takes_the_lower() {
    let ours = ProtocolVersion { major: 1, minor: 3 };
    assert_eq!(
        ours.negotiate(ProtocolVersion { major: 2, minor: 0 }),
        Err(Reject::MajorMismatch { ours: 1, theirs: 2 })
    );
    assert_eq!(
        ours.negotiate(ProtocolVersion { major: 1, minor: 1 }),
        Ok(ProtocolVersion { major: 1, minor: 1 })
    );
}

#[test]
fn unknown_capabilities_are_negotiated_away() {
    let offered: Vec<String> = ["preview", "teleport", "kill", "preview"]
        .map(String::from)
        .to_vec();
    let accepted = capability::negotiate(&offered, &capability::KNOWN);
    assert_eq!(accepted, vec!["preview", "kill"]);
}

#[test]
fn unknown_trailing_field_is_ignored() {
    let mut bytes = encode(&hello_frame());
    // Field 99, length-delimited, "future".
    bytes.extend_from_slice(&[0x9a, 0x06, 0x06]);
    bytes.extend_from_slice(b"future");
    assert_eq!(decode::<NodeFrame>(&bytes), Ok(hello_frame()));
}

#[test]
fn newer_peer_with_extra_optional_field_still_decodes() {
    // The same PaneState, as a newer build that appended field 40.
    #[derive(Clone, PartialEq, prost::Message)]
    struct NewerPaneState {
        #[prost(message, optional, tag = "1")]
        pane_ref: Option<PaneRefMsg>,
        #[prost(enumeration = "AgentKindCode", tag = "2")]
        agent_kind: i32,
        #[prost(enumeration = "StateCode", tag = "3")]
        state: i32,
        #[prost(enumeration = "SourceCode", tag = "4")]
        source: i32,
        #[prost(string, tag = "9")]
        session: String,
        #[prost(string, tag = "40")]
        cost_estimate: String,
    }
    let older = pane_state("%1", StateCode::Permit);
    let newer = NewerPaneState {
        pane_ref: older.pane_ref.clone(),
        agent_kind: older.agent_kind,
        state: older.state,
        source: older.source,
        session: older.session.clone(),
        cost_estimate: "$1.20".into(),
    };
    let decoded: PaneState = decode(&encode(&newer)).expect("newer message decodes");
    assert_eq!(decoded.session, "work");
    assert_eq!(decoded.agent_state(), Ok(flight_state::AgentState::Permit));
}

#[test]
fn unknown_enum_value_rejects_the_message() {
    let mut p = pane_state("%1", StateCode::Permit);
    p.state = 99;
    let frame = NodeFrame {
        body: Some(node_body::Body::Delta(Delta {
            incarnation: inc(1).as_bytes().to_vec(),
            sequence: 1,
            change: Some(delta_change::Change::PaneUpsert(p)),
        })),
    };
    assert_eq!(
        decode::<NodeFrame>(&encode(&frame)),
        Err(Reject::UnknownEnum {
            field: "pane_state.state",
            value: 99
        })
    );
}

#[test]
fn unspecified_enum_value_is_not_a_default() {
    let mut p = pane_state("%1", StateCode::Permit);
    p.agent_kind = 0;
    assert!(matches!(p.validate(), Err(Reject::UnknownEnum { .. })));
}

#[test]
fn missing_oneof_body_is_rejected() {
    assert_eq!(
        decode::<NodeFrame>(&[]),
        Err(Reject::Missing("node_frame.body"))
    );
    assert_eq!(
        decode::<OrchestratorFrame>(&[]),
        Err(Reject::Missing("orchestrator_frame.body"))
    );
    assert_eq!(
        decode::<UiRequest>(&[]),
        Err(Reject::Missing("ui_request.body"))
    );
}

#[test]
fn empty_identity_fields_are_rejected() {
    let mut p = pane_state("%1", StateCode::Busy);
    if let Some(r) = p.pane_ref.as_mut() {
        r.host.clear();
    }
    assert_eq!(p.validate(), Err(Reject::Empty("pane_ref.host")));
    let mut hello = node_hello();
    hello.node_id.clear();
    assert_eq!(hello.validate(), Err(Reject::Empty("node_hello.node_id")));
}

#[test]
fn incarnation_must_be_exactly_sixteen_bytes() {
    for bad in [vec![], vec![1u8; 15], vec![1u8; 17]] {
        let d = Delta {
            incarnation: bad,
            sequence: 1,
            change: Some(delta_change::Change::PaneRemoved(
                pane_state("%1", StateCode::Busy).pane_ref.expect("ref"),
            )),
        };
        assert_eq!(d.validate(), Err(Reject::OutOfRange("incarnation")));
    }
}

#[test]
fn delta_sequence_starts_at_one_and_needs_a_change() {
    let d = Delta {
        incarnation: inc(1).as_bytes().to_vec(),
        sequence: 0,
        change: None,
    };
    assert_eq!(d.validate(), Err(Reject::OutOfRange("delta.sequence")));
}

#[test]
fn preview_line_bounds_are_enforced() {
    let req = |lines| Request {
        request_id: 1,
        command: Some(Command {
            kind: Some(command_kind::Kind::GetPreview(command_kind::GetPreview {
                pane_ref: pane_state("%1", StateCode::Busy).pane_ref,
                lines,
            })),
        }),
    };
    assert!(req(40).validate().is_ok());
    assert!(req(0).validate().is_err());
    assert!(req(MAX_PREVIEW_LINES + 1).validate().is_err());
}

#[test]
fn commands_report_their_target_host() {
    let kill = Command {
        kind: Some(command_kind::Kind::KillPane(command_kind::KillPane {
            pane_ref: pane_state("%1", StateCode::Busy).pane_ref,
        })),
    };
    assert_eq!(kill.target_host(), Some("node-ab12"));
    assert_eq!(Command { kind: None }.target_host(), None);
}

fn reveal(pid: u32) -> Command {
    Command {
        kind: Some(command_kind::Kind::RevealPane(command_kind::RevealPane {
            pane_ref: pane_state("%1", StateCode::Busy).pane_ref,
            expected_pid: pid,
        })),
    }
}

#[test]
fn a_reveal_must_name_the_pane_process_it_is_about() {
    assert!(reveal(4242).validate().is_ok());
    assert_eq!(
        reveal(0).validate(),
        Err(Reject::OutOfRange("reveal_pane.expected_pid"))
    );
    assert_eq!(reveal(1).target_host(), Some("node-ab12"));
}

#[test]
fn the_retired_unguarded_switch_is_not_a_command_any_more() {
    // What a peer that still sent tag 2 (`SwitchPane { pane_ref }`) puts on the wire: a command
    // whose oneof has no variant here. It decodes to no kind and is refused, never reinterpreted
    // as a reveal.
    let pane = encode(
        &pane_state("%1", StateCode::Busy)
            .pane_ref
            .unwrap_or_default(),
    );
    // SwitchPane { pane_ref = field 1 }, carried as Command's field 2.
    let mut switch = vec![0x0a, u8::try_from(pane.len()).unwrap_or(0)];
    switch.extend(&pane);
    let mut bytes = vec![0x12, u8::try_from(switch.len()).unwrap_or(0)];
    bytes.extend(&switch);
    let decoded: Result<Command, _> = decode(&bytes);
    assert_eq!(decoded, Err(Reject::Missing("command.kind")));
}

#[test]
fn a_pane_state_without_a_pid_decodes_with_pid_zero() {
    // Frozen older shape: no field 14.
    #[derive(Clone, PartialEq, prost::Message)]
    struct OlderPaneState {
        #[prost(message, optional, tag = "1")]
        pane_ref: Option<PaneRefMsg>,
        #[prost(enumeration = "AgentKindCode", tag = "2")]
        agent_kind: i32,
        #[prost(enumeration = "StateCode", tag = "3")]
        state: i32,
        #[prost(enumeration = "SourceCode", tag = "4")]
        source: i32,
        #[prost(string, tag = "9")]
        session: String,
    }
    let base = pane_state("%1", StateCode::Permit);
    let older = OlderPaneState {
        pane_ref: base.pane_ref.clone(),
        agent_kind: base.agent_kind,
        state: base.state,
        source: base.source,
        session: base.session.clone(),
    };
    let decoded: PaneState = decode(&encode(&older)).expect("older message decodes");
    assert_eq!(decoded.pid, 0);
}

#[test]
fn oversized_frames_are_refused_before_parsing() {
    let big = vec![0u8; MAX_FRAME_BYTES + 1];
    assert!(matches!(
        decode::<NodeFrame>(&big),
        Err(Reject::TooLarge { .. })
    ));
}

#[test]
fn truncation_and_bit_flips_never_panic() {
    let bytes = encode(&node_snapshot_frame());
    for cut in 0..bytes.len() {
        let _ = decode::<NodeFrame>(bytes.get(..cut).unwrap_or(&[]));
    }
    for i in 0..bytes.len() {
        for bit in 0..8 {
            let mut copy = bytes.clone();
            if let Some(b) = copy.get_mut(i) {
                *b ^= 1 << bit;
            }
            let _ = decode::<NodeFrame>(&copy);
        }
    }
}

#[test]
fn pane_ref_round_trips_through_the_domain_type() {
    let msg = pane_state("%7", StateCode::Idle).pane_ref.expect("ref");
    let domain = flight_state::PaneRef::try_from(&msg).expect("valid");
    assert_eq!(PaneRefMsg::from(&domain), msg);
}

#[test]
fn every_domain_state_has_a_wire_code_that_round_trips() {
    for s in flight_state::AgentState::ALL {
        let mut p = pane_state("%1", StateCode::Idle);
        p.state = PaneState::state_code(s) as i32;
        assert_eq!(p.agent_state(), Ok(s));
    }
}

#[test]
fn every_message_kind_round_trips() {
    let frames: Vec<NodeFrame> = vec![
        hello_frame(),
        NodeFrame {
            body: Some(node_body::Body::Heartbeat(Heartbeat { seq: 9 })),
        },
        node_snapshot_frame(),
        NodeFrame {
            body: Some(node_body::Body::Response(Response {
                request_id: 4,
                result: Some(response_result::Result::Preview(Preview {
                    text: "hi\n".into(),
                    captured_at: 5,
                })),
            })),
        },
        NodeFrame {
            body: Some(node_body::Body::Response(Response {
                request_id: 5,
                result: Some(response_result::Result::Error(ErrorInfo {
                    kind: ErrorKindCode::Unsupported as i32,
                    message: "nope".into(),
                })),
            })),
        },
    ];
    for f in frames {
        assert_eq!(decode::<NodeFrame>(&encode(&f)), Ok(f));
    }
}

fn open(f: impl FnOnce(&mut command_kind::OpenTerminal)) -> Command {
    let mut open = command_kind::OpenTerminal {
        pane_ref: pane_state("%1", StateCode::Busy).pane_ref,
        expected_pid: 4242,
        cols: 80,
        rows: 24,
        term: "xterm-256color".to_owned(),
        terminal_id: Vec::new(),
    };
    f(&mut open);
    Command {
        kind: Some(command_kind::Kind::OpenTerminal(open)),
    }
}

#[test]
fn an_open_terminal_is_validated_at_the_boundary() {
    assert!(open(|_| {}).validate().is_ok());
    assert!(open(|o| o.terminal_id = vec![7; TERMINAL_ID_LEN])
        .validate()
        .is_ok());
    type Mutation = Box<dyn FnOnce(&mut command_kind::OpenTerminal)>;
    let bad: Vec<(Mutation, &str)> = vec![
        (
            Box::new(|o| o.expected_pid = 0),
            "open_terminal.expected_pid",
        ),
        (Box::new(|o| o.cols = 0), "open_terminal.size"),
        (Box::new(|o| o.rows = 0), "open_terminal.size"),
        (
            Box::new(|o| o.cols = MAX_TERMINAL_DIM + 1),
            "open_terminal.size",
        ),
        (Box::new(|o| o.rows = u32::MAX), "open_terminal.size"),
        (Box::new(|o| o.term = String::new()), "open_terminal.term"),
        (
            Box::new(|o| o.term = "x".repeat(MAX_TERM_LEN + 1)),
            "open_terminal.term",
        ),
        (
            Box::new(|o| o.term = "xterm;rm".to_owned()),
            "open_terminal.term",
        ),
        (
            Box::new(|o| o.term = "a b".to_owned()),
            "open_terminal.term",
        ),
        (
            Box::new(|o| o.term = "x\n".to_owned()),
            "open_terminal.term",
        ),
        (
            Box::new(|o| o.terminal_id = vec![1; 15]),
            "open_terminal.terminal_id",
        ),
    ];
    for (mutate, what) in bad {
        assert_eq!(
            open(mutate).validate(),
            Err(Reject::OutOfRange(what)),
            "{what}"
        );
    }
}

#[test]
fn a_ui_cannot_choose_the_terminal_id_and_a_node_must_be_given_one() {
    let ui = |id: Vec<u8>| UiRequest {
        body: Some(ui_request_body::Body::Command(Request {
            request_id: 1,
            command: Some(open(|o| o.terminal_id = id)),
        })),
    };
    assert!(ui(Vec::new()).validate().is_ok());
    assert_eq!(
        ui(vec![7; TERMINAL_ID_LEN]).validate(),
        Err(Reject::Mismatch("open_terminal.terminal_id"))
    );
    let node = |id: Vec<u8>| OrchestratorFrame {
        body: Some(orchestrator_body::Body::Request(Request {
            request_id: 1,
            command: Some(open(|o| o.terminal_id = id)),
        })),
    };
    assert!(node(vec![7; TERMINAL_ID_LEN]).validate().is_ok());
    assert_eq!(
        node(Vec::new()).validate(),
        Err(Reject::Missing("open_terminal.terminal_id"))
    );
}

#[test]
fn terminal_frames_are_bounded_and_directional() {
    use Origin::{Node, Ui};
    let data = |n: usize| TerminalFrame::data(vec![0; n]);
    assert!(data(MAX_TERMINAL_DATA).validate_from(Ui).is_ok());
    assert_eq!(
        data(MAX_TERMINAL_DATA + 1).validate(),
        Err(Reject::TooLarge {
            len: MAX_TERMINAL_DATA + 1,
            max: MAX_TERMINAL_DATA
        })
    );
    assert!(TerminalFrame::attach(vec![0; 15]).validate().is_err());
    assert!(TerminalFrame::attach(vec![0; TERMINAL_ID_LEN])
        .validate()
        .is_ok());
    let resize = |cols, rows| TerminalFrame {
        body: Some(terminal_body::Body::Resize(TerminalResize { cols, rows })),
    };
    assert!(resize(80, 24).validate_from(Ui).is_ok());
    assert!(resize(0, 24).validate().is_err());
    assert!(resize(80, MAX_TERMINAL_DIM + 1).validate().is_err());
    assert!(
        resize(80, 24).validate_from(Node).is_err(),
        "a node cannot resize"
    );
    let exit = TerminalFrame {
        body: Some(terminal_body::Body::Exit(TerminalExit {
            reason: ExitReasonCode::NodeLost as i32,
            status: 0,
        })),
    };
    assert!(exit.validate_from(Node).is_ok());
    assert!(
        exit.validate_from(Ui).is_err(),
        "a UI cannot exit a terminal"
    );
    let unknown = TerminalFrame {
        body: Some(terminal_body::Body::Exit(TerminalExit {
            reason: 99,
            status: 0,
        })),
    };
    assert!(unknown.validate().is_err());
    let close = TerminalFrame {
        body: Some(terminal_body::Body::Close(TerminalClose {})),
    };
    assert!(close.validate_from(Ui).is_ok() && close.validate_from(Node).is_err());
    assert!(TerminalFrame::default().validate().is_err());
}

#[test]
fn terminal_syntax_helpers_agree_with_the_rules() {
    for ok in ["xterm", "xterm-256color", "screen.xterm_new", "a"] {
        assert!(valid_term(ok), "{ok}");
    }
    for bad in ["", "a b", "a;b", "é", &"x".repeat(33)] {
        assert!(!valid_term(bad), "{bad:?}");
    }
}
