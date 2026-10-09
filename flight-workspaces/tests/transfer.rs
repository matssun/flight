// SPDX-License-Identifier: MIT

//! Sharing and keeping arrangements: export carries only the declaration; import treats the
//! file as untrusted; profiles and the previous generation let a mistake be taken back.

mod support;
use flight_workspaces::*;
use support::*;

fn profile() -> Profile {
    let mut d = def("nga", "dev1", "/work/nga");
    d.root.record(&present(7));
    d.last_workspace_id = Some("w-1".to_owned());
    d.surfaces[0].last_surface_id = Some("s-1".to_owned());
    d.surfaces[0].skip_permissions = true;
    let mut p = Profile::new("work");
    p.workspaces.push(d);
    p.dismissed.push("w-gone".to_owned());
    p
}

#[test]
fn an_export_holds_the_declaration_and_nothing_about_a_machine_or_a_moment() {
    let text = export(&profile()).unwrap();
    for banned in [
        "w-1",
        "s-1",
        "w-gone",
        "dev1",
        "ino",
        "dev =",
        "birth",
        "dismissed",
    ] {
        assert!(!text.contains(banned), "{banned} in\n{text}");
    }
    assert!(text.contains("/work/nga") && text.contains("nga"));
    assert!(text.contains(PORTABLE_HOST));
}

#[test]
fn an_import_is_imported_new_and_local_to_the_host_it_lands_on() {
    let original = profile();
    let (imported, report) = import(&export(&original).unwrap(), "host-b", "shared").unwrap();
    let (a, b) = (&original.workspaces[0], &imported.workspaces[0]);
    assert_eq!(
        (b.name.as_str(), b.root.path.as_str()),
        ("nga", "/work/nga")
    );
    assert_eq!(b.host, "host-b");
    assert_eq!(b.origin, Origin::Imported);
    assert_ne!(
        a.key, b.key,
        "the file cannot choose or collide with an identity"
    );
    assert_ne!(a.surfaces[0].key, b.surfaces[0].key);
    assert!(b.last_workspace_id.is_none() && b.root.identity.is_none());
    assert!(b.surfaces.iter().all(|s| s.last_surface_id.is_none()));
    // Permission-free agents lose the flag; the report says so.
    assert!(b.surfaces.iter().all(|s| !s.skip_permissions));
    assert_eq!((report.workspaces, report.prompts_restored), (1, 1));
}

#[test]
fn importing_twice_makes_two_sets_of_identities() {
    let text = export(&profile()).unwrap();
    let (a, _) = import(&text, "h", "one").unwrap();
    let (b, _) = import(&text, "h", "two").unwrap();
    assert_ne!(a.workspaces[0].key, b.workspaces[0].key);
}

#[test]
fn a_file_that_is_hostile_or_broken_is_refused_or_skipped_not_trusted() {
    // Too big.
    assert!(import(&"#".repeat(MAX_IMPORT_BYTES + 1), "h", "x").is_err());
    // Not TOML, newer schema, no profile.
    assert!(matches!(import("[[", "h", "x"), Err(LoadError::Corrupt(_))));
    assert!(matches!(
        import("schema_version = 99\nactive_profile = \"d\"\n", "h", "x"),
        Err(LoadError::TooNew { found: 99 })
    ));
    assert!(import("schema_version = 1\nactive_profile = \"d\"\n", "h", "x").is_err());
    // A definition with a bad name or a relative root is skipped with a reason.
    let text = r#"
schema_version = 1
active_profile = "d"
[[profile]]
name = "d"
[[profile.workspace]]
key = "c-1"
name = "ok"
host = "-"
[profile.workspace.root]
path = "/fine"
[[profile.workspace]]
key = "c-2"
name = "bad name!"
host = "-"
[profile.workspace.root]
path = "/fine"
[[profile.workspace]]
key = "c-3"
name = "relative"
host = "-"
[profile.workspace.root]
path = "not/absolute"
"#;
    let (p, report) = import(text, "h", "x").unwrap();
    assert_eq!(p.workspaces.len(), 1);
    assert_eq!(report.skipped.len(), 2);
}

#[test]
fn an_imported_profile_starts_nothing_even_in_a_pass_that_may_start_things() {
    let (imported, _) = import(&export(&profile()).unwrap(), "h", "shared").unwrap();
    let roots = Roots::default();
    roots.set("/work/nga", present(1));
    let fake = Fake::default();
    let mut doc = Document::default();
    doc.add_profile(imported).unwrap();
    doc.use_profile("shared");
    let go = RecoveryPolicy {
        start_missing: true,
        ..RecoveryPolicy::default()
    };
    let mut exec = fake.clone();
    // Even a pass that is allowed to start things starts no import.
    let report = recover(&mut doc, &["h"], &fake, &roots, &mut exec, &go);
    assert_eq!(report.items[0].refusals, vec![Refusal::ImportedNotTrusted]);
    assert_eq!(fake.running("h"), 0);
}

#[test]
fn profiles_are_named_unique_switchable_and_copies_get_new_identities() {
    let mut doc = Document::default();
    doc.active_mut()
        .unwrap()
        .workspaces
        .push(def("a", "h", "/a"));
    assert_eq!(
        doc.add_profile(Profile::new("bad name")),
        Err(ProfileError::BadName)
    );
    assert_eq!(
        doc.add_profile(Profile::new(DEFAULT_PROFILE)),
        Err(ProfileError::Exists)
    );
    doc.duplicate_profile(DEFAULT_PROFILE, "copy").unwrap();
    assert_eq!(
        doc.duplicate_profile("nope", "x"),
        Err(ProfileError::Unknown)
    );
    let (a, b) = (
        &doc.profile(DEFAULT_PROFILE).unwrap().workspaces[0],
        &doc.profile("copy").unwrap().workspaces[0],
    );
    assert_ne!(a.key, b.key);
    assert!(doc.use_profile("copy") && !doc.use_profile("nope"));
    assert_eq!(doc.active_profile, "copy");
}

#[test]
fn the_previous_generation_takes_back_one_mistake() {
    let dir = scratch("transfer-prev");
    let store = Store::new(&dir);
    assert!(matches!(store.previous(), Err(StoreError::NotFound)));
    let before = doc_with(vec![def("keep", "h", "/k")]);
    store.save(&before).unwrap();
    let mut after = before.clone();
    let key = after.active().unwrap().workspaces[0].key.clone();
    after.active_mut().unwrap().remove(&key);
    store.save(&after).unwrap();
    assert!(store
        .load()
        .unwrap()
        .0
        .active()
        .unwrap()
        .workspaces
        .is_empty());
    assert_eq!(store.previous().unwrap(), before);
}

#[test]
fn only_one_holder_at_a_time_and_a_dropped_lock_is_free() {
    let dir = scratch("transfer-lock");
    let store = Store::new(&dir);
    let first = store.lock().unwrap();
    assert!(matches!(Store::new(&dir).lock(), Err(StoreError::Locked)));
    assert!(matches!(
        Store::new(&dir).lock_waiting(std::time::Duration::from_millis(60)),
        Err(StoreError::Locked)
    ));
    drop(first);
    store.lock().unwrap();
}
