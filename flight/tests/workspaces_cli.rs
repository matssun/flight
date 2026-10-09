// SPDX-License-Identifier: MIT

//! `flight workspaces` against the real binary and a private config directory: keeping,
//! sharing and taking back saved workspaces, and refusing to edit a file a running node holds.
//! Nothing here uses tmux or the default config directory.

use flight_workspaces::{ConfigKey, Document, Origin, Store};
use std::path::PathBuf;
use std::process::{Command, Output};

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Self {
        let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("cli-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }

    fn node(&self) -> PathBuf {
        self.0.join("node")
    }

    fn store(&self) -> Store {
        Store::new(self.node())
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_flight"))
            .arg("workspaces")
            .args(args)
            .args(["--config-dir", self.0.to_str().unwrap()])
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn fails(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(!out.status.success(), "{args:?} should fail");
        String::from_utf8_lossy(&out.stderr).into_owned()
    }

    fn seed(&self, names: &[&str]) {
        let mut doc = Document::default();
        for n in names {
            doc.active_mut().unwrap().workspaces.push(workspace(n));
        }
        self.store().save(&doc).unwrap();
    }

    fn saved(&self) -> Document {
        self.store().load().unwrap().0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn workspace(name: &str) -> flight_workspaces::WorkspaceDefinition {
    flight_workspaces::WorkspaceDefinition {
        key: ConfigKey::mint().unwrap(),
        name: name.to_owned(),
        host: "h1".to_owned(),
        root: flight_workspaces::RootSpec::new(format!("/work/{name}")),
        surfaces: vec![flight_workspaces::SurfaceSpec {
            key: ConfigKey::mint().unwrap(),
            kind: flight_workspaces::SurfaceKind::Agent,
            provider: Some("claude".to_owned()),
            skip_permissions: true,
            last_surface_id: None,
        }],
        origin: Origin::Local,
        last_workspace_id: Some("w-1".to_owned()),
    }
}

#[test]
fn list_shows_what_is_saved() {
    let d = Dir::new("list");
    d.seed(&["nga", "api"]);
    let out = d.ok(&["list"]);
    assert!(
        out.contains("nga") && out.contains("/work/api") && out.contains("c-"),
        "{out}"
    );
}

#[test]
fn snapshots_are_kept_listed_and_restored_with_the_current_state_kept() {
    let d = Dir::new("snap");
    d.seed(&["one"]);
    d.ok(&["snapshot", "save", "monday"]);
    d.seed(&["two"]);
    assert_eq!(d.ok(&["snapshot", "list"]).trim(), "monday");
    d.fails(&["snapshot", "save", "monday"]);
    d.ok(&["snapshot", "restore", "monday"]);
    assert_eq!(d.saved().active().unwrap().workspaces[0].name, "one");
    let labels = d.ok(&["snapshot", "list"]);
    assert!(labels.contains("before-monday"), "{labels}");
}

#[test]
fn undo_takes_back_the_last_change() {
    let d = Dir::new("undo");
    d.seed(&["keep"]);
    d.ok(&["profile", "duplicate", "default", "copy"]);
    assert_eq!(d.saved().profiles.len(), 2);
    d.ok(&["undo"]);
    assert_eq!(d.saved().profiles.len(), 1);
}

#[test]
fn profiles_can_be_copied_and_switched() {
    let d = Dir::new("profiles");
    d.seed(&["a"]);
    d.ok(&["profile", "duplicate", "default", "other"]);
    d.ok(&["profile", "use", "other"]);
    let listed = d.ok(&["profile", "list"]);
    assert!(
        listed.contains("* other") && listed.contains("  default"),
        "{listed}"
    );
    assert!(d.fails(&["profile", "use", "nope"]).contains("no profile"));
    assert!(d
        .fails(&["profile", "duplicate", "default", "other"])
        .contains("exists"));
}

#[test]
fn export_then_import_carries_the_declaration_and_nothing_trusted() {
    let d = Dir::new("share");
    d.seed(&["nga"]);
    let file = d.0.join("nga.toml");
    d.ok(&["export", file.to_str().unwrap()]);
    let shared = std::fs::read_to_string(&file).unwrap();
    assert!(
        !shared.contains("w-1") && !shared.contains("h1"),
        "{shared}"
    );

    let other = Dir::new("receive");
    let said = other.ok(&[
        "import",
        file.to_str().unwrap(),
        "--host",
        "h2",
        "--as",
        "from-d",
    ]);
    assert!(said.contains("start nothing until you trust"), "{said}");
    assert!(said.contains("permission prompts"), "{said}");
    let doc = other.saved();
    let w = &doc.profile("from-d").unwrap().workspaces[0];
    assert_eq!((w.host.as_str(), w.origin), ("h2", Origin::Imported));
    assert!(w.surfaces.iter().all(|s| !s.skip_permissions));
    // It did not become the active profile, and the listing marks it.
    assert_eq!(doc.active_profile, "default");
    other.ok(&["profile", "use", "from-d"]);
    assert!(other.ok(&["list"]).contains("imported, not trusted"));
}

#[test]
fn an_import_without_a_joined_node_asks_for_a_host_and_a_bad_file_changes_nothing() {
    let d = Dir::new("badimport");
    d.seed(&["a"]);
    let file = d.0.join("x.toml");
    std::fs::write(&file, "not toml [").unwrap();
    let before = std::fs::read_to_string(d.store().path()).unwrap();
    assert!(d
        .fails(&["import", file.to_str().unwrap()])
        .contains("--host"));
    assert!(!d
        .fails(&["import", file.to_str().unwrap(), "--host", "h"])
        .is_empty());
    assert_eq!(std::fs::read_to_string(d.store().path()).unwrap(), before);
}

#[test]
fn changes_are_refused_while_a_node_holds_the_file_but_looking_and_sharing_are_not() {
    let d = Dir::new("locked");
    d.seed(&["nga"]);
    let _node = d.store().lock().unwrap();
    let e = d.fails(&["profile", "use", "default"]);
    assert!(e.contains("node is running"), "{e}");
    d.fails(&["undo"]);
    d.ok(&["list"]);
    d.ok(&["snapshot", "save", "while-running"]);
    let file = d.0.join("out.toml");
    d.ok(&["export", file.to_str().unwrap()]);
}

#[test]
fn an_unusable_saved_file_is_reported_and_left_alone() {
    let d = Dir::new("corrupt");
    std::fs::create_dir_all(d.node()).unwrap();
    std::fs::write(d.store().path(), "garbage = [").unwrap();
    assert!(d.fails(&["list"]).contains("cannot be read"));
    assert!(!d
        .fails(&["profile", "duplicate", "default", "x"])
        .is_empty());
    assert_eq!(
        std::fs::read_to_string(d.store().path()).unwrap(),
        "garbage = ["
    );
}
