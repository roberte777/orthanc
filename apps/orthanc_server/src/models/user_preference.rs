use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct UserPreference {
    pub user_id: i64,
    pub preferred_audio_language: Option<String>,
    pub preferred_subtitle_language: Option<String>,
    pub subtitles_enabled_default: bool,
    pub audio_normalize_default: bool,
    pub updated_at: String,
}

pub async fn load_preference(pool: &SqlitePool, user_id: i64) -> Result<Option<UserPreference>> {
    let pref =
        sqlx::query_as::<_, UserPreference>("SELECT * FROM user_preferences WHERE user_id = ?")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(pref)
}

pub async fn upsert_preference(
    pool: &SqlitePool,
    user_id: i64,
    preferred_audio_language: Option<&str>,
    preferred_subtitle_language: Option<&str>,
    subtitles_enabled_default: bool,
    audio_normalize_default: bool,
) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO user_preferences
            (user_id, preferred_audio_language, preferred_subtitle_language,
             subtitles_enabled_default, audio_normalize_default)
           VALUES (?, ?, ?, ?, ?)
           ON CONFLICT(user_id) DO UPDATE SET
             preferred_audio_language = excluded.preferred_audio_language,
             preferred_subtitle_language = excluded.preferred_subtitle_language,
             subtitles_enabled_default = excluded.subtitles_enabled_default,
             audio_normalize_default = excluded.audio_normalize_default"#,
    )
    .bind(user_id)
    .bind(preferred_audio_language)
    .bind(preferred_subtitle_language)
    .bind(subtitles_enabled_default)
    .bind(audio_normalize_default)
    .execute(pool)
    .await?;
    Ok(())
}
