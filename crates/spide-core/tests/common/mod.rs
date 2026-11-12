#![allow(dead_code)]
use std::path::PathBuf;

/// Compare `text` with `tests/golden/<name>`; `UPDATE_GOLDEN=1` (or a missing file) rewrites it.
pub fn golden(name: &str, text: &str) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    if std::env::var("UPDATE_GOLDEN").is_ok() || !path.exists() {
        std::fs::write(&path, text).unwrap();
    }
    let want = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text, want, "golden {name} differs (UPDATE_GOLDEN=1 to accept)");
}
