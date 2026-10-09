// SPDX-License-Identifier: MIT

use std::path::{Path, PathBuf};

/// Claude Code's name for a project's directory under `projects/`: the working directory with
/// every character that is not an ASCII letter or digit replaced by `-` (documented). `None`
/// when the name cannot be worked out with certainty: a path with other characters than ASCII
/// (the provider counts those in its own units), or one long enough that the provider shortens
/// it with a hash.
pub(crate) fn project_dir_name(root: &str) -> Option<String> {
    if !root.is_ascii() || root.len() > 150 {
        return None;
    }
    Some(
        root.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect(),
    )
}

/// Where the provider keeps its data: `CLAUDE_CONFIG_DIR` if the node has it, else `~/.claude`.
pub(crate) fn config_dir(
    env_override: Option<&std::ffi::OsStr>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    match env_override.filter(|v| !v.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir)),
        None => home.map(|h| h.join(".claude")),
    }
}

/// The transcript of session `token` for the directory `root`, if it is there and has
/// something in it.
pub(crate) fn find(config: &Path, root: &str, token: &str) -> Result<PathBuf, String> {
    let project = project_dir_name(root).ok_or_else(|| {
        "the directory's name cannot be matched to the provider's records with certainty".to_owned()
    })?;
    let path = config
        .join("projects")
        .join(project)
        .join(format!("{token}.jsonl"));
    match std::fs::metadata(&path) {
        Ok(m) if m.is_file() && m.len() > 0 => Ok(path),
        Ok(_) => Err("the saved conversation is empty".to_owned()),
        Err(_) => Err("no saved conversation was found for this directory".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_project_name_is_the_path_with_everything_but_letters_and_digits_replaced() {
        // Observed from the installed CLI: a path with `/`, `-`, `_` and a leading dot.
        assert_eq!(
            project_dir_name("/private/tmp/claude-501/-Users-mats_dev/.x").as_deref(),
            Some("-private-tmp-claude-501--Users-mats-dev--x")
        );
        assert_eq!(project_dir_name("/work/ä"), None);
        assert_eq!(project_dir_name(&format!("/{}", "a".repeat(200))), None);
    }

    #[test]
    fn the_transcript_must_exist_for_that_directory_and_have_content() {
        let base = std::env::temp_dir().join(format!("flight-transcript-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("projects").join("-work-a");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(find(&base, "/work/a", "t1")
            .unwrap_err()
            .contains("no saved"));
        std::fs::write(dir.join("t1.jsonl"), "").unwrap();
        assert!(find(&base, "/work/a", "t1").unwrap_err().contains("empty"));
        std::fs::write(dir.join("t1.jsonl"), "{}\n").unwrap();
        assert!(find(&base, "/work/a", "t1").is_ok());
        // The same token for another directory is not this conversation.
        assert!(find(&base, "/work/b", "t1").is_err());
        let _ = std::fs::remove_dir_all(&base);
    }
}
