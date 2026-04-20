use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct TrackPreference {
    pub user_id: i64,
    pub scope_media_item_id: i64,
    pub audio_language: Option<String>,
    pub subtitle_language: Option<String>,
    pub subtitles_enabled: bool,
    pub audio_normalize: bool,
    pub updated_at: String,
}

/// Walk the `parent_id` chain to find the root scope for a given media item.
/// Episodes roll up to their show; movies and shows return themselves.
pub async fn resolve_scope_media_item_id(pool: &SqlitePool, media_item_id: i64) -> Result<i64> {
    let mut current = media_item_id;
    for _ in 0..5 {
        let row: Option<(String, Option<i64>)> =
            sqlx::query_as("SELECT media_type, parent_id FROM media_items WHERE id = ?")
                .bind(current)
                .fetch_optional(pool)
                .await?;
        let Some((media_type, parent_id)) = row else {
            return Ok(current);
        };
        match (media_type.as_str(), parent_id) {
            ("episode" | "season", Some(pid)) => current = pid,
            _ => return Ok(current),
        }
    }
    Ok(current)
}

pub async fn load_preference(
    pool: &SqlitePool,
    user_id: i64,
    scope_media_item_id: i64,
) -> Result<Option<TrackPreference>> {
    let pref = sqlx::query_as::<_, TrackPreference>(
        "SELECT * FROM user_track_preferences WHERE user_id = ? AND scope_media_item_id = ?",
    )
    .bind(user_id)
    .bind(scope_media_item_id)
    .fetch_optional(pool)
    .await?;
    Ok(pref)
}

pub async fn upsert_preference(
    pool: &SqlitePool,
    user_id: i64,
    scope_media_item_id: i64,
    audio_language: Option<&str>,
    subtitle_language: Option<&str>,
    subtitles_enabled: bool,
    audio_normalize: bool,
) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO user_track_preferences
            (user_id, scope_media_item_id, audio_language, subtitle_language,
             subtitles_enabled, audio_normalize)
           VALUES (?, ?, ?, ?, ?, ?)
           ON CONFLICT(user_id, scope_media_item_id) DO UPDATE SET
             audio_language = excluded.audio_language,
             subtitle_language = excluded.subtitle_language,
             subtitles_enabled = excluded.subtitles_enabled,
             audio_normalize = excluded.audio_normalize"#,
    )
    .bind(user_id)
    .bind(scope_media_item_id)
    .bind(audio_language)
    .bind(subtitle_language)
    .bind(subtitles_enabled)
    .bind(audio_normalize)
    .execute(pool)
    .await?;
    Ok(())
}
