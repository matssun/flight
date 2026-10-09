// SPDX-License-Identifier: MIT

//! Wire fixtures: the exact bytes of representative frames. A failure here means the wire
//! contract changed; that needs a deliberate version decision, not a quiet fixture update.
//! Regenerate with `FLIGHT_UPDATE_GOLDEN=1 cargo test -p flight-proto --test golden`.

mod support;

use flight_proto::codec::{decode, encode};
use flight_proto::*;
use std::path::PathBuf;
use support::*;

fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.hex"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<String>() + "\n"
}

fn unhex(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text
        .trim()
        .bytes()
        .filter_map(|c| (c as char).to_digit(16).map(|d| d as u8))
        .collect();
    digits
        .chunks(2)
        .filter_map(|p| match p {
            [h, l] => Some(h * 16 + l),
            _ => None,
        })
        .collect()
}

fn check<M>(name: &str, value: M)
where
    M: prost::Message + Default + PartialEq + std::fmt::Debug + Validate,
{
    let bytes = encode(&value);
    let file = path(name);
    if std::env::var_os("FLIGHT_UPDATE_GOLDEN").is_some() {
        std::fs::write(&file, hex(&bytes)).expect("write golden");
    }
    let expected = std::fs::read_to_string(&file).expect("golden file present");
    assert_eq!(
        hex(&bytes),
        format!("{}\n", expected.trim()),
        "{name}: encoding changed"
    );
    assert_eq!(
        decode::<M>(&unhex(&expected)),
        Ok(value),
        "{name}: decode changed"
    );
}

#[test]
fn node_hello_v1() {
    check(
        "node_hello",
        NodeFrame {
            body: Some(node_body::Body::Hello(node_hello())),
        },
    );
}

#[test]
fn node_snapshot_v1() {
    check("node_snapshot", node_snapshot_frame());
}

#[test]
fn node_delta_v1() {
    check(
        "node_delta",
        NodeFrame {
            body: Some(node_body::Body::Delta(Delta {
                incarnation: inc(3).as_bytes().to_vec(),
                sequence: 1,
                change: Some(delta_change::Change::PaneUpsert(pane_state(
                    "%1",
                    StateCode::Done,
                ))),
            })),
        },
    );
}

#[test]
fn orchestrator_request_v1() {
    check(
        "orchestrator_get_preview",
        OrchestratorFrame {
            body: Some(orchestrator_body::Body::Request(Request {
                request_id: 11,
                command: Some(Command {
                    kind: Some(command_kind::Kind::GetPreview(command_kind::GetPreview {
                        pane_ref: pane_state("%1", StateCode::Busy).pane_ref,
                        lines: 40,
                    })),
                }),
            })),
        },
    );
}

#[test]
fn orchestrator_reveal_pane_v1() {
    check(
        "orchestrator_reveal_pane",
        OrchestratorFrame {
            body: Some(orchestrator_body::Body::Request(Request {
                request_id: 12,
                command: Some(Command {
                    kind: Some(command_kind::Kind::RevealPane(command_kind::RevealPane {
                        pane_ref: pane_state("%1", StateCode::Busy).pane_ref,
                        expected_pid: 4242,
                    })),
                }),
            })),
        },
    );
}

#[test]
fn pane_state_with_a_pid_v1() {
    let mut pane = pane_state("%1", StateCode::Permit);
    pane.pid = 4242;
    check(
        "node_pane_with_pid",
        NodeFrame {
            body: Some(node_body::Body::Delta(Delta {
                incarnation: inc(1).as_bytes().to_vec(),
                sequence: 1,
                change: Some(delta_change::Change::PaneUpsert(pane)),
            })),
        },
    );
}

#[test]
fn ui_fleet_snapshot_v1() {
    check(
        "ui_fleet_snapshot",
        UiEvent {
            body: Some(ui_event_body::Body::Snapshot(FleetSnapshot {
                incarnation: inc(1).as_bytes().to_vec(),
                nodes: vec![NodeView {
                    saved: vec![],
                    node_id: "node-ab12".into(),
                    display_name: "mini-2".into(),
                    status: NodeStatusCode::Online as i32,
                    servers: vec![],
                    panes: vec![pane_state("%1", StateCode::Permit)],
                }],
            })),
        },
    );
}

fn open_terminal(id: Vec<u8>) -> Command {
    Command {
        kind: Some(command_kind::Kind::OpenTerminal(
            command_kind::OpenTerminal {
                pane_ref: pane_state("%1", StateCode::Busy).pane_ref,
                expected_pid: 4242,
                cols: 120,
                rows: 40,
                term: "xterm-256color".to_owned(),
                terminal_id: id,
            },
        )),
    }
}

#[test]
fn orchestrator_open_terminal_v1() {
    check(
        "orchestrator_open_terminal",
        OrchestratorFrame {
            body: Some(orchestrator_body::Body::Request(Request {
                request_id: 13,
                command: Some(open_terminal((1..=16).collect())),
            })),
        },
    );
}

#[test]
fn ui_open_terminal_v1() {
    check(
        "ui_open_terminal",
        UiRequest {
            body: Some(ui_request_body::Body::Command(Request {
                request_id: 14,
                command: Some(open_terminal(Vec::new())),
            })),
        },
    );
}

#[test]
fn terminal_opened_v1() {
    check(
        "ui_terminal_opened",
        UiEvent {
            body: Some(ui_event_body::Body::Response(Response {
                request_id: 14,
                result: Some(response_result::Result::Terminal(TerminalOpened {
                    terminal_id: (1..=16).collect(),
                })),
            })),
        },
    );
}

#[test]
fn terminal_frames_v1() {
    check("terminal_attach", TerminalFrame::attach((1..=16).collect()));
    check(
        "terminal_data",
        TerminalFrame::data(b"ls\r\x1b[0m".to_vec()),
    );
    check(
        "terminal_resize",
        TerminalFrame {
            body: Some(terminal_body::Body::Resize(TerminalResize {
                cols: 100,
                rows: 30,
            })),
        },
    );
    check(
        "terminal_exit",
        TerminalFrame {
            body: Some(terminal_body::Body::Exit(TerminalExit {
                reason: ExitReasonCode::ClientExited as i32,
                status: 0,
            })),
        },
    );
}
