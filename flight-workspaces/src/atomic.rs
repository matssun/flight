// SPDX-License-Identifier: MIT

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

pub(crate) const TMP_MARK: &str = ".tmp-";

/// Replace `path` with `bytes` so that a crash at any point leaves either the old content or
/// the new, never a mixture: write a sibling temporary file, flush it to disk, rename it over
/// the target, then flush the directory so the rename itself survives power loss.
pub(crate) fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("no file name"))?
        .to_string_lossy();
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp: PathBuf = dir.join(format!("{name}{TMP_MARK}{}-{n}", std::process::id()));
    let result = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)?;
        std::fs::File::open(dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}
