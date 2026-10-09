// SPDX-License-Identifier: MIT

mod support;
use flight_workspaces::*;
use support::*;

#[test]
fn a_missing_file_is_a_fresh_document_and_writes_nothing() {
    let dir = scratch("store-fresh");
    let (doc, how) = Store::new(&dir).load().unwrap();
    assert_eq!(how, Loaded::Fresh);
    assert_eq!(doc, Document::default());
    assert!(!dir.join("workspaces.toml").exists());
}

#[test]
fn save_then_load_round_trips_with_identities_intact() {
    let dir = scratch("store-roundtrip");
    let store = Store::new(&dir);
    let mut d = def("nga", "dev1", "/work/nga");
    d.root.record(&present(7));
    d.last_workspace_id = Some("w-1".to_owned());
    let doc = doc_with(vec![d]);
    store.save(&doc).unwrap();
    let (back, how) = store.load().unwrap();
    assert_eq!((back, how), (doc, Loaded::Existing));
}

#[test]
fn a_crash_before_the_rename_leaves_the_old_file_and_a_harmless_temporary() {
    let dir = scratch("store-crash");
    let store = Store::new(&dir);
    let doc = doc_with(vec![def("a", "h", "/a")]);
    store.save(&doc).unwrap();
    // What an interrupted save leaves: a torn sibling temporary, never the real file.
    std::fs::write(dir.join("workspaces.toml.tmp-1-0"), "schema_ver").unwrap();
    assert_eq!(store.load().unwrap().0, doc);
    assert_eq!(store.sweep_temporaries().unwrap(), 1);
    assert_eq!(store.load().unwrap().0, doc);
}

#[test]
fn saves_replace_whole_files_and_leave_no_temporaries() {
    let dir = scratch("store-replace");
    let store = Store::new(&dir);
    for n in 0..20 {
        store
            .save(&doc_with(vec![def(&format!("w{n}"), "h", "/a")]))
            .unwrap();
    }
    let names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
    assert_eq!(names.len(), 1, "{names:?}");
}

#[test]
fn an_unreadable_file_is_reported_and_never_overwritten() {
    let dir = scratch("store-corrupt");
    let store = Store::new(&dir);
    std::fs::write(store.path(), "schema_version = [oops").unwrap();
    assert!(matches!(
        store.load(),
        Err(StoreError::Load(LoadError::Corrupt(_)))
    ));
    assert!(store.save(&Document::default()).is_err());
    assert_eq!(
        std::fs::read_to_string(store.path()).unwrap(),
        "schema_version = [oops"
    );
    // Only the user's say-so moves it, and it is kept intact.
    let kept = store.quarantine().unwrap();
    assert_eq!(
        std::fs::read_to_string(kept).unwrap(),
        "schema_version = [oops"
    );
    store.save(&Document::default()).unwrap();
}

#[test]
fn a_file_from_a_newer_flight_is_neither_read_nor_overwritten() {
    let dir = scratch("store-newer");
    let store = Store::new(&dir);
    let text = "schema_version = 99\nactive_profile = \"x\"\n";
    std::fs::write(store.path(), text).unwrap();
    assert!(matches!(
        store.load(),
        Err(StoreError::Load(LoadError::TooNew { found: 99 }))
    ));
    assert!(matches!(
        store.save(&Document::default()),
        Err(StoreError::NewerOnDisk { found: 99 })
    ));
    assert_eq!(std::fs::read_to_string(store.path()).unwrap(), text);
}

#[test]
fn snapshots_are_kept_listed_restored_and_never_overwritten() {
    let dir = scratch("store-snap");
    let store = Store::new(&dir);
    let before = doc_with(vec![def("before", "h", "/b")]);
    store.save(&before).unwrap();
    store.snapshot("monday", &before).unwrap();
    assert!(matches!(
        store.snapshot("monday", &before),
        Err(StoreError::Exists)
    ));
    assert!(matches!(
        store.snapshot("../x", &before),
        Err(StoreError::BadLabel)
    ));
    let after = doc_with(vec![def("after", "h", "/a")]);
    store.save(&after).unwrap();
    let restored = store.restore_snapshot("monday", "pre-restore").unwrap();
    assert_eq!(restored, before);
    assert_eq!(store.load().unwrap().0, before);
    assert_eq!(store.load_snapshot("pre-restore").unwrap(), after);
    assert_eq!(store.list_snapshots(), vec!["monday", "pre-restore"]);
}

#[test]
fn duplicating_a_profile_mints_new_identities_and_drops_runtime_hints() {
    let mut d = def("nga", "h", "/a");
    d.last_workspace_id = Some("w-9".to_owned());
    let mut profile = Profile::new("work");
    profile.workspaces.push(d.clone());
    let copy = profile.duplicate("work-copy").unwrap();
    let c = &copy.workspaces[0];
    assert_ne!(c.key, d.key);
    assert_ne!(c.surfaces[0].key, d.surfaces[0].key);
    assert_eq!(
        (c.last_workspace_id.clone(), c.root.path.as_str()),
        (None, "/a")
    );
}

#[test]
fn nothing_runtime_or_secret_is_in_the_saved_text() {
    let mut d = def("nga", "h", "/a");
    d.surfaces[0].skip_permissions = true;
    let text = doc_with(vec![d]).to_toml().unwrap();
    for banned in [
        "token", "secret", "password", "env", "argv", "pid", "pane", "%",
    ] {
        assert!(!text.contains(banned), "{banned} in\n{text}");
    }
}
