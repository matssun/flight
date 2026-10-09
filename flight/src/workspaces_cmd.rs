// SPDX-License-Identifier: MIT

//! `flight workspaces`: look at, keep, share and take back the workspaces a node has saved.
//! Offline tooling for the node's `workspaces.toml`; nothing here starts a process, creates a
//! directory or touches a repository.

use crate::args::{config_dir, Args};
use crate::roles::node_dir;
use flight_transport::identity_dir;
use flight_trust::Identity;
use flight_workspaces::{export, import, Document, Store, StoreError};
use std::path::Path;

pub const USAGE: &str = "usage: flight workspaces <command> [--config-dir DIR]

  list                          the saved workspaces of the active profile
  undo                          take back the last change to the saved file
  snapshot save LABEL           keep a named copy of the saved state
  snapshot list
  snapshot restore LABEL        make a snapshot the saved state (the current one is kept
                                as 'before-LABEL')
  profile list
  profile use NAME              switch the active profile
  profile duplicate FROM TO     copy a profile under new identities
  export FILE [--profile NAME]  write a profile as a shareable file (no runtime ids, no
                                host, no recorded directory identity, no secrets)
  import FILE [--as NAME] [--host HOST]
                                add a shared file as a new profile. Imported workspaces start
                                nothing until you trust them; permission-free agents lose that
                                flag; every identity is new.

Changes to the saved file need the node to be stopped (it keeps the file in use while it
runs); listing, snapshot save/list and export work while it runs.";

pub fn run(args: &[String]) -> Result<(), String> {
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    let parsed = Args::parse(args, &["--config-dir", "--profile", "--as", "--host"], &[])?;
    let dir = node_dir(&config_dir(&parsed)?);
    let store = Store::new(&dir);
    let words: Vec<&str> = parsed.positional.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["list"] => list(&store),
        ["undo"] => edit(&store, |_, store| {
            let previous = store.previous().map_err(|e| e.to_string())?;
            Ok((
                Some(previous),
                "Restored the state before the last change.".to_owned(),
            ))
        }),
        ["snapshot", "save", label] => {
            let (doc, _) = store.load().map_err(explain)?;
            let path = store.snapshot(label, &doc).map_err(explain)?;
            println!("Kept {}.", path.display());
            Ok(())
        }
        ["snapshot", "list"] => {
            for label in store.list_snapshots() {
                println!("{label}");
            }
            Ok(())
        }
        ["snapshot", "restore", label] => edit(&store, |_, store| {
            let before = format!("before-{label}");
            // The restore saves for itself (keeping the current state first); nothing more to save.
            store.restore_snapshot(label, &before).map_err(explain)?;
            Ok((
                None,
                format!("Restored {label}. The state before is kept as {before}."),
            ))
        }),
        ["profile", "list"] => {
            let (doc, _) = store.load().map_err(explain)?;
            for p in &doc.profiles {
                let mark = if p.name == doc.active_profile {
                    "*"
                } else {
                    " "
                };
                println!("{mark} {} ({} workspaces)", p.name, p.workspaces.len());
            }
            Ok(())
        }
        ["profile", "use", name] => edit(&store, |mut doc, _| {
            if !doc.use_profile(name) {
                return Err(format!("no profile {name:?}"));
            }
            Ok((Some(doc), format!("Now using {name}.")))
        }),
        ["profile", "duplicate", from, to] => edit(&store, |mut doc, _| {
            doc.duplicate_profile(from, to).map_err(|e| e.to_string())?;
            Ok((
                Some(doc),
                format!("Copied {from} as {to}, with new identities."),
            ))
        }),
        ["export", file] => {
            let (doc, _) = store.load().map_err(explain)?;
            let name = parsed.value("--profile").unwrap_or(&doc.active_profile);
            let profile = doc
                .profile(name)
                .ok_or_else(|| format!("no profile {name:?}"))?;
            let text = export(profile).map_err(|e| e.to_string())?;
            std::fs::write(file, text).map_err(|e| format!("cannot write {file}: {e}"))?;
            println!(
                "Wrote {} workspaces of {name} to {file}.",
                profile.workspaces.len()
            );
            Ok(())
        }
        ["import", file] => {
            let text =
                std::fs::read_to_string(file).map_err(|e| format!("cannot read {file}: {e}"))?;
            let host = match parsed.value("--host") {
                Some(h) => h.to_owned(),
                None => this_node(&dir)?,
            };
            let name = parsed.value("--as").map(str::to_owned).unwrap_or_else(|| {
                Path::new(file).file_stem().map_or_else(
                    || "imported".to_owned(),
                    |s| s.to_string_lossy().into_owned(),
                )
            });
            edit(&store, |mut doc, _| {
                let (profile, report) = import(&text, &host, &name).map_err(|e| e.to_string())?;
                doc.add_profile(profile).map_err(|e| e.to_string())?;
                let mut said = format!(
                    "Imported {} workspaces as profile {name}. They start nothing until you trust them.",
                    report.workspaces
                );
                if report.prompts_restored > 0 {
                    said.push_str(&format!(
                        " {} agent(s) saved without permission prompts will ask again.",
                        report.prompts_restored
                    ));
                }
                for skipped in &report.skipped {
                    said.push_str(&format!("\nSkipped {skipped}"));
                }
                Ok((Some(doc), said))
            })
        }
        _ => Err(USAGE.to_owned()),
    }
}

fn list(store: &Store) -> Result<(), String> {
    let (doc, _) = store.load().map_err(explain)?;
    let Some(profile) = doc.active() else {
        return Err("the saved file has no active profile".to_owned());
    };
    println!("profile {}", profile.name);
    for w in &profile.workspaces {
        let origin = if w.origin == flight_workspaces::Origin::Imported {
            "  (imported, not trusted)"
        } else {
            ""
        };
        println!("  {}  {}  {}{origin}", w.name, w.root.path, w.key);
    }
    Ok(())
}

/// Change the saved file under the store's lock: load, let `change` produce the new state and a
/// line to print, save.
fn edit(
    store: &Store,
    change: impl FnOnce(Document, &Store) -> Result<(Option<Document>, String), String>,
) -> Result<(), String> {
    let _lock = store.lock().map_err(|e| match e {
        StoreError::Locked => {
            "the node is running and keeps the saved workspaces in use; stop it first".to_owned()
        }
        other => other.to_string(),
    })?;
    let (doc, _) = store.load().map_err(explain)?;
    let (new, said) = change(doc, store)?;
    if let Some(new) = new {
        store.save(&new).map_err(explain)?;
    }
    println!("{said}");
    Ok(())
}

fn explain(e: StoreError) -> String {
    e.to_string()
}

/// The host label this node's workspaces carry: its identity's host id.
fn this_node(node_dir: &Path) -> Result<String, String> {
    let identity = Identity::load(&identity_dir(node_dir)).map_err(|_| {
        "this machine has not joined an orchestrator yet; pass --host HOST".to_owned()
    })?;
    Ok(identity.fingerprint().host_id().to_string())
}
