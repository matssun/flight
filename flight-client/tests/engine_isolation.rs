// SPDX-License-Identifier: MIT

//! The terminal emulator library is named in one source file and one manifest. Nothing else in
//! the workspace may depend on which library it is (ADR-011).

use std::fs;
use std::path::{Path, PathBuf};

const ADAPTER: &str = "flight-client/src/screens/emulator.rs";
const MANIFEST: &str = "flight-client/Cargo.toml";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if matches!(name.to_str(), Some("target" | "ref" | ".git")) {
            continue;
        }
        if path.is_dir() {
            files(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs" | "toml")
        ) {
            out.push(path);
        }
    }
}

#[test]
fn only_the_adapter_and_its_manifest_name_the_emulator_library() {
    let root = workspace();
    let mut all = Vec::new();
    files(&root, &mut all);
    let mut offenders = Vec::new();
    for path in all {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if rel == ADAPTER || rel == MANIFEST || rel == "Cargo.lock" {
            continue;
        }
        // The one place that is allowed to describe the rule is this test.
        if rel == "flight-client/tests/engine_isolation.rs" {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        if text.contains("vt100") {
            offenders.push(rel);
        }
    }
    assert!(
        offenders.is_empty(),
        "these files name the emulator library: {offenders:?}"
    );
}
