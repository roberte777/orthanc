//! Subtitle cache cleanup: remove orphaned cached VTTs and enforce a size cap.

use std::collections::HashSet;
use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tracing::{debug, info, warn};

/// A single cached file with enough metadata to decide eviction order.
struct CacheEntry {
    path: PathBuf,
    stream_id: Option<i64>,
    size: u64,
    mtime: SystemTime,
}

/// List every cached `<id>.vtt` under `cache_dir`. Entries with
/// unparseable names are also included (they'll be sweeper candidates).
pub fn list_cache_entries(cache_dir: &Path) -> Vec<(PathBuf, Option<i64>, Metadata)> {
    let Ok(dir) = std::fs::read_dir(cache_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in dir.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let stream_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<i64>().ok())
            .filter(|_| {
                path.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("vtt"))
                    .unwrap_or(false)
            });
        out.push((path, stream_id, meta));
    }
    out
}

/// Remove cached files whose stream_id is not in `live_ids`, plus any
/// files with unparseable names (tmp files, etc.).
///
/// Returns the number of files removed.
pub fn sweep_orphans(cache_dir: &Path, live_ids: &HashSet<i64>) -> usize {
    let mut removed = 0usize;
    for (path, stream_id, _meta) in list_cache_entries(cache_dir) {
        let keep = match stream_id {
            Some(id) => live_ids.contains(&id),
            None => false, // temp files, foreign files — treat as orphans
        };
        if !keep {
            if let Err(e) = std::fs::remove_file(&path) {
                warn!("Failed to remove orphan subtitle cache {:?}: {}", path, e);
            } else {
                removed += 1;
            }
        }
    }
    removed
}

/// Enforce a size cap by LRU-evicting oldest files. Files whose stream id
/// is in `protected` are never evicted (useful if you want to keep an
/// in-progress session's cache alive).
///
/// Returns the number of files removed.
pub fn enforce_size_cap(cache_dir: &Path, max_bytes: u64, protected: &HashSet<i64>) -> usize {
    let entries = list_cache_entries(cache_dir);
    let total: u64 = entries.iter().map(|(_, _, m)| m.len()).sum();
    if total <= max_bytes {
        return 0;
    }

    let mut candidates: Vec<CacheEntry> = entries
        .into_iter()
        .map(|(path, stream_id, meta)| {
            let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            CacheEntry { path, stream_id, size: meta.len(), mtime }
        })
        .filter(|c| match c.stream_id {
            Some(id) => !protected.contains(&id),
            None => true,
        })
        .collect();
    // Oldest first
    candidates.sort_by_key(|c| c.mtime);

    let mut removed = 0usize;
    let mut running = total;
    for c in &candidates {
        if running <= max_bytes {
            break;
        }
        if let Err(e) = std::fs::remove_file(&c.path) {
            warn!("Failed to evict subtitle cache {:?}: {}", c.path, e);
            continue;
        }
        running = running.saturating_sub(c.size);
        removed += 1;
    }
    removed
}

/// Full sweep: remove orphans then enforce size cap. Logs counts.
pub fn full_sweep(cache_dir: &Path, live_ids: &HashSet<i64>, max_bytes: u64) {
    let orphans = sweep_orphans(cache_dir, live_ids);
    let evicted = enforce_size_cap(cache_dir, max_bytes, &HashSet::new());
    if orphans > 0 || evicted > 0 {
        info!(
            "Subtitle cache sweep: removed {} orphan(s), evicted {} file(s) for size cap",
            orphans, evicted
        );
    } else {
        debug!("Subtitle cache sweep: clean");
    }
}

/// Periodic loop: sweep every `interval`, using the DB to source live ids.
/// Reads the `subtitle_cache_max_mb` admin setting each tick so an admin change
/// takes effect on the next sweep without needing a restart. Falls back to
/// `default_max_bytes` if the setting is unset or unparseable.
pub async fn run_cleanup_loop(
    cache_dir: PathBuf,
    db: sqlx::SqlitePool,
    default_max_bytes: u64,
    interval: std::time::Duration,
) {
    let mut ticker = tokio::time::interval(interval);
    // Wait one interval before first sweep (avoid racing startup work).
    ticker.tick().await;
    loop {
        ticker.tick().await;
        let live_ids = match load_live_ids(&db).await {
            Ok(ids) => ids,
            Err(e) => {
                warn!("Subtitle cache sweep: failed to load live ids: {}", e);
                continue;
            }
        };
        let max_bytes = crate::api::settings::read_setting(&db, "subtitle_cache_max_mb")
            .await
            .and_then(|s| s.parse::<u64>().ok())
            .map(|mb| mb.saturating_mul(1_024 * 1_024))
            .unwrap_or(default_max_bytes);
        let cache_dir = cache_dir.clone();
        let _ = tokio::task::spawn_blocking(move || {
            full_sweep(&cache_dir, &live_ids, max_bytes)
        })
        .await;
    }
}

async fn load_live_ids(db: &sqlx::SqlitePool) -> Result<HashSet<i64>, sqlx::Error> {
    let rows: Vec<(i64,)> =
        sqlx::query_as("SELECT id FROM media_streams WHERE stream_type = 'subtitle'")
            .fetch_all(db)
            .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;
    use tempfile::tempdir;

    fn write(path: &Path, bytes: &[u8]) {
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn sweeps_orphans_only() {
        let dir = tempdir().unwrap();
        write(&dir.path().join("1.vtt"), b"WEBVTT\n");
        write(&dir.path().join("2.vtt"), b"WEBVTT\n");
        write(&dir.path().join("3.vtt"), b"WEBVTT\n");

        let live: HashSet<i64> = [1i64, 3].into_iter().collect();
        let n = sweep_orphans(dir.path(), &live);
        assert_eq!(n, 1);
        assert!(dir.path().join("1.vtt").exists());
        assert!(!dir.path().join("2.vtt").exists());
        assert!(dir.path().join("3.vtt").exists());
    }

    #[test]
    fn sweeps_unparseable_names() {
        let dir = tempdir().unwrap();
        write(&dir.path().join("tmp.abc.vtt"), b"x"); // non-numeric stem
        write(&dir.path().join("10.vtt"), b"ok");
        write(&dir.path().join("readme.txt"), b"also not a vtt");

        let live: HashSet<i64> = [10i64].into_iter().collect();
        let n = sweep_orphans(dir.path(), &live);
        // Removes tmp.abc.vtt and readme.txt (both have no valid stream_id mapping)
        assert_eq!(n, 2);
        assert!(dir.path().join("10.vtt").exists());
    }

    #[test]
    fn size_cap_evicts_oldest() {
        let dir = tempdir().unwrap();
        // Three files totalling 3072 bytes, cap = 2048, expect oldest gone.
        let p1 = dir.path().join("1.vtt");
        let p2 = dir.path().join("2.vtt");
        let p3 = dir.path().join("3.vtt");
        write(&p1, &vec![0u8; 1024]);
        // Ensure distinct mtimes
        std::thread::sleep(Duration::from_millis(20));
        write(&p2, &vec![0u8; 1024]);
        std::thread::sleep(Duration::from_millis(20));
        write(&p3, &vec![0u8; 1024]);

        let protected: HashSet<i64> = HashSet::new();
        let n = enforce_size_cap(dir.path(), 2048, &protected);
        assert_eq!(n, 1);
        assert!(!p1.exists(), "oldest should be evicted");
        assert!(p2.exists());
        assert!(p3.exists());
    }

    #[test]
    fn size_cap_respects_protected_set() {
        let dir = tempdir().unwrap();
        let p1 = dir.path().join("1.vtt");
        let p2 = dir.path().join("2.vtt");
        let p3 = dir.path().join("3.vtt");
        write(&p1, &vec![0u8; 1024]);
        std::thread::sleep(Duration::from_millis(20));
        write(&p2, &vec![0u8; 1024]);
        std::thread::sleep(Duration::from_millis(20));
        write(&p3, &vec![0u8; 1024]);

        // Protect the oldest — the next oldest should be evicted instead.
        let protected: HashSet<i64> = [1i64].into_iter().collect();
        let n = enforce_size_cap(dir.path(), 2048, &protected);
        assert_eq!(n, 1);
        assert!(p1.exists(), "protected id should survive");
        assert!(!p2.exists(), "second-oldest should be evicted");
        assert!(p3.exists());
    }

    #[test]
    fn size_cap_noop_when_under() {
        let dir = tempdir().unwrap();
        write(&dir.path().join("1.vtt"), &vec![0u8; 500]);
        let n = enforce_size_cap(dir.path(), 10_000, &HashSet::new());
        assert_eq!(n, 0);
    }

    #[test]
    fn full_sweep_runs_both_passes() {
        let dir = tempdir().unwrap();
        write(&dir.path().join("1.vtt"), &vec![0u8; 2000]);
        std::thread::sleep(Duration::from_millis(20));
        write(&dir.path().join("2.vtt"), &vec![0u8; 2000]);
        write(&dir.path().join("99.vtt"), b"orphan");

        let live: HashSet<i64> = [1i64, 2].into_iter().collect();
        full_sweep(dir.path(), &live, 2500);
        // 99.vtt orphaned → removed
        assert!(!dir.path().join("99.vtt").exists());
        // cap 2500 < 4000: oldest (1.vtt) evicted
        assert!(!dir.path().join("1.vtt").exists());
        assert!(dir.path().join("2.vtt").exists());
    }
}
