// SPDX-License-Identifier: MIT

use crate::migrate::Step;
use crate::{Document, LoadError, Loaded, Store};

/// A stand-in for a past schema: workspaces were a flat top-level list with a `dir`.
const V1_FILE: &str = r#"
schema_version = 1

[[workspace]]
key = "c-0000000000000001"
name = "nga"
host = "dev1"
dir = "/work/nga"
"#;

fn flat_to_profiles(t: &mut toml::Table) -> Result<(), String> {
    let list = t.remove("workspace").ok_or("no workspace list")?;
    let mut items = list.as_array().cloned().ok_or("not a list")?;
    for w in &mut items {
        let w = w.as_table_mut().ok_or("not a table")?;
        let dir = w.remove("dir").ok_or("no dir")?;
        let mut root = toml::Table::new();
        root.insert("path".to_owned(), dir);
        w.insert("root".to_owned(), toml::Value::Table(root));
    }
    let mut profile = toml::Table::new();
    profile.insert("name".to_owned(), "default".into());
    profile.insert("workspace".to_owned(), toml::Value::Array(items));
    t.insert("active_profile".to_owned(), "default".into());
    t.insert(
        "profile".to_owned(),
        toml::Value::Array(vec![profile.into()]),
    );
    Ok(())
}

const STEPS: &[Step] = &[flat_to_profiles];

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tmp"))
        .join(format!("mig-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn an_old_schema_is_migrated_with_its_content_kept() {
    let doc = Document::from_toml_with(V1_FILE, STEPS).unwrap();
    assert_eq!(doc.schema_version, 2);
    let w = &doc.active().unwrap().workspaces[0];
    assert_eq!(
        (w.name.as_str(), w.root.path.as_str()),
        ("nga", "/work/nga")
    );
    assert_eq!(w.key.as_str(), "c-0000000000000001");
}

#[test]
fn migrating_on_load_keeps_the_original_and_is_done_once() {
    let dir = scratch("load");
    std::fs::write(dir.join("workspaces.toml"), V1_FILE).unwrap();
    let store = Store::with_steps(&dir, STEPS);
    let (_, how) = store.load().unwrap();
    assert_eq!(how, Loaded::Migrated { from: 1 });
    assert_eq!(
        std::fs::read_to_string(dir.join("workspaces.v1.bak")).unwrap(),
        V1_FILE
    );
    let (_, again) = store.load().unwrap();
    assert_eq!(again, Loaded::Existing);
}

#[test]
fn a_failing_step_changes_nothing_on_disk() {
    fn broken(_: &mut toml::Table) -> Result<(), String> {
        Err("boom".to_owned())
    }
    const BAD: &[Step] = &[broken];
    let dir = scratch("fail");
    std::fs::write(dir.join("workspaces.toml"), V1_FILE).unwrap();
    let err = Store::with_steps(&dir, BAD).load().unwrap_err();
    assert!(err.to_string().contains("boom"));
    assert_eq!(
        std::fs::read_to_string(dir.join("workspaces.toml")).unwrap(),
        V1_FILE
    );
    assert!(!dir.join("workspaces.v1.bak").exists());
}

#[test]
fn a_newer_schema_is_refused_not_read() {
    let text = "schema_version = 99\nactive_profile = \"default\"\n";
    assert_eq!(
        Document::from_toml(text),
        Err(LoadError::TooNew { found: 99 })
    );
}
