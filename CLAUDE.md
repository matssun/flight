<!-- SPDX-License-Identifier: Apache-2.0 -->

# Flight — code base standards

Rust workspace. One workspace: this directory. `ref/` holds read-only upstream clones (sesh, tmux-sessionizer, fleet), is gitignored, and is never edited.

Adopted from mcp-re:

- One main type per file; `mod.rs` re-exports.
- 200-line file and 60-line function thresholds are design-review triggers, not automatic splits.
- Narrowest visibility that works; never widen for a test.
- No unchecked arithmetic; no `unwrap`/`expect`/indexing in production code.
- SPDX header on every file.

Build: `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`. Bazel is planned, not yet set up.

Licensing: any copied/ported upstream code needs its MIT notice in `THIRD_PARTY.md`.
