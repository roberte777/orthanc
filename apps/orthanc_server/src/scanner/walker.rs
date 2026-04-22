use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct FileInfo {
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<String>,
    pub created: Option<String>,
}

/// Recursively walk a directory and return all video files.
pub fn walk_directory(root: &Path) -> Vec<FileInfo> {
    let mut files = Vec::new();
    walk_recursive(root, &mut files);
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

fn walk_recursive(dir: &Path, files: &mut Vec<FileInfo>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!("Cannot read directory {}: {}", dir.display(), e);
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_recursive(&path, files);
        } else if path.is_file() && super::is_video_file(&path) {
            let metadata = std::fs::metadata(&path).ok();
            let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
            let modified = metadata.as_ref().and_then(|m| m.modified().ok()).map(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                dt.format("%Y-%m-%d %H:%M:%S").to_string()
            });
            let created = metadata.as_ref().and_then(|m| m.created().ok()).map(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                dt.format("%Y-%m-%d %H:%M:%S").to_string()
            });

            files.push(FileInfo {
                path,
                size,
                modified,
                created,
            });
        }
    }
}
