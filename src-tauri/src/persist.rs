//! Small helpers for the JSON files in the app config dir (`settings.json`, `usage.json`).

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Writes `contents` to `path` atomically: writes a temp file in the same directory, flushes it
/// to disk and renames it over the target, so a crash never leaves a half-written file. Creates
/// the parent directory if needed.
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = with_suffix(path, ".tmp");
    let result = (|| -> io::Result<()> {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// `path` with `suffix` appended to its file name (`usage.json` → `usage.json.bak`).
pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_atomically_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("file.json");

        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
        assert!(!with_suffix(&path, ".tmp").exists());
    }
}
