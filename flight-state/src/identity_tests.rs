// SPDX-License-Identifier: MIT

use crate::{HostId, PaneId, PaneRef, ServerId, SessionId, SessionRef};
use std::collections::HashSet;

fn pane(host: &str, server: &str, pane: &str) -> PaneRef {
    PaneRef {
        host: HostId::new(host),
        server: ServerId::new(server),
        pane: PaneId::new(pane),
    }
}

#[test]
fn same_pane_id_on_different_hosts_is_distinct() {
    assert_ne!(
        pane("mini-a", "flight", "%1"),
        pane("mini-b", "flight", "%1")
    );
}

#[test]
fn same_pane_id_on_different_servers_is_distinct() {
    assert_ne!(
        pane("mini-a", "flight", "%1"),
        pane("mini-a", "other", "%1")
    );
}

#[test]
fn equal_refs_hash_together() {
    let set: HashSet<PaneRef> = [
        pane("h", "s", "%1"),
        pane("h", "s", "%1"),
        pane("h", "s", "%2"),
    ]
    .into();
    assert_eq!(set.len(), 2);
}

#[test]
fn session_ref_identity_is_not_the_name() {
    let r = |h: &str| SessionRef {
        host: HostId::new(h),
        server: ServerId::new("flight"),
        session: SessionId::new("$1"),
    };
    assert_ne!(r("a"), r("b"));
    assert_eq!(r("a"), r("a"));
}

#[test]
fn display_is_the_raw_value() {
    assert_eq!(PaneId::new("%7").to_string(), "%7");
}
