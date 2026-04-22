//! Validate that a filesystem path from the database sits under a known
//! library root. Defense-in-depth against traversal or DB poisoning.

use std::path::{Path, PathBuf};

/// Validate `path` against a list of known library roots. Returns the
/// canonical path on success, or an error describing why it was rejected.
///
/// Both the candidate path and each root are canonicalized so that symlinks
/// are resolved and `..` components normalized before the prefix check.
pub fn validate_under_roots(path: &str, roots: &[PathBuf]) -> Result<PathBuf, &'static str> {
    let candidate = Path::new(path);
    let canonical = std::fs::canonicalize(candidate).map_err(|_| "path missing or unreadable")?;

    for root in roots {
        // Some roots may be missing on disk at validation time; skip those.
        let root_canonical = match std::fs::canonicalize(root) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if canonical.starts_with(&root_canonical) {
            return Ok(canonical);
        }
    }
    Err("path is not inside any configured library root")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn accepts_path_inside_root() {
        let root_dir = tempdir().unwrap();
        let file = root_dir.path().join("movie.srt");
        fs::write(&file, b"").unwrap();

        let ok = validate_under_roots(file.to_str().unwrap(), &[root_dir.path().to_path_buf()]);
        assert!(ok.is_ok(), "expected Ok, got {:?}", ok);
    }

    #[test]
    fn accepts_nested_path_inside_root() {
        let root_dir = tempdir().unwrap();
        let deep = root_dir.path().join("a/b/c/movie.srt");
        fs::create_dir_all(deep.parent().unwrap()).unwrap();
        fs::write(&deep, b"").unwrap();

        let ok = validate_under_roots(deep.to_str().unwrap(), &[root_dir.path().to_path_buf()]);
        assert!(ok.is_ok());
    }

    #[test]
    fn rejects_path_outside_roots() {
        let root_dir = tempdir().unwrap();
        let outside_dir = tempdir().unwrap();
        let outside = outside_dir.path().join("evil.srt");
        fs::write(&outside, b"").unwrap();

        let err = validate_under_roots(outside.to_str().unwrap(), &[root_dir.path().to_path_buf()]);
        assert!(err.is_err(), "expected error, got {:?}", err);
    }

    #[test]
    fn rejects_missing_path() {
        let root_dir = tempdir().unwrap();
        let err = validate_under_roots(
            &format!("{}/nope.srt", root_dir.path().display()),
            &[root_dir.path().to_path_buf()],
        );
        assert!(err.is_err());
    }

    #[test]
    fn traversal_that_lands_inside_root_is_ok() {
        // Canonicalization resolves `..`, so `.../a/../movie.srt` → `.../movie.srt`
        let root_dir = tempdir().unwrap();
        let sub = root_dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        let file = root_dir.path().join("movie.srt");
        fs::write(&file, b"").unwrap();
        let traversal = format!("{}/../movie.srt", sub.display());

        let ok = validate_under_roots(&traversal, &[root_dir.path().to_path_buf()]);
        assert!(ok.is_ok(), "expected Ok, got {:?}", ok);
    }

    #[test]
    fn symlink_escaping_root_rejected() {
        // Only meaningful when we can create symlinks (unix).
        #[cfg(unix)]
        {
            let root_dir = tempdir().unwrap();
            let outside_dir = tempdir().unwrap();
            let target = outside_dir.path().join("outside.srt");
            fs::write(&target, b"").unwrap();
            let link = root_dir.path().join("link.srt");
            std::os::unix::fs::symlink(&target, &link).unwrap();

            let err =
                validate_under_roots(link.to_str().unwrap(), &[root_dir.path().to_path_buf()]);
            assert!(err.is_err(), "expected error, got {:?}", err);
        }
    }
}
