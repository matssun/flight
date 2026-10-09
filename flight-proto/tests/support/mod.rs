// SPDX-License-Identifier: MIT

use flight_proto::*;

pub fn pane_state(pane: &str, state: StateCode) -> PaneState {
    PaneState {
        pane_ref: Some(PaneRefMsg {
            host: "node-ab12".into(),
            server: "flight".into(),
            pane: pane.into(),
        }),
        agent_kind: AgentKindCode::Claude as i32,
        state: state as i32,
        source: SourceCode::Scrape as i32,
        rule_id: "permit.do-you-want".into(),
        why: "permit.do-you-want".into(),
        changed_at: 1_700_000_050,
        session: "work".into(),
        window: "agent".into(),
        path: "/home/u/proj".into(),
        command: "claude".into(),
        pid: 0,
        ..Default::default()
    }
}

pub fn node_hello() -> NodeHello {
    NodeHello {
        version: Some(CURRENT_VERSION),
        node_id: "node-ab12".into(),
        display_name: "mini-2".into(),
        capabilities: vec!["preview".into(), "kill".into()],
        servers: vec!["flight".into()],
    }
}

pub fn node_snapshot_frame() -> NodeFrame {
    NodeFrame {
        body: Some(node_body::Body::Snapshot(Snapshot {
            saved: vec![],
            incarnation: inc(3).as_bytes().to_vec(),
            panes: vec![
                pane_state("%1", StateCode::Permit),
                pane_state("%2", StateCode::Busy),
            ],
            servers: vec![ServerStatus {
                server: "flight".into(),
                availability: AvailabilityCode::Available as i32,
                detail: String::new(),
            }],
        })),
    }
}

pub fn inc(n: u8) -> Incarnation {
    Incarnation::from_bytes([n; Incarnation::LEN])
}
