-- Global per-user playback defaults. Applies when no per-media
-- user_track_preferences row exists for a given show/movie.

CREATE TABLE IF NOT EXISTS user_preferences (
    user_id INTEGER PRIMARY KEY,
    preferred_audio_language TEXT,           -- ISO 639-2; NULL = no preference
    preferred_subtitle_language TEXT,        -- ISO 639-2; NULL when subtitles disabled
    subtitles_enabled_default BOOLEAN NOT NULL DEFAULT 0,
    audio_normalize_default BOOLEAN NOT NULL DEFAULT 0,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TRIGGER trg_user_preferences_updated_at
    AFTER UPDATE ON user_preferences FOR EACH ROW
    BEGIN
        UPDATE user_preferences SET updated_at = CURRENT_TIMESTAMP
        WHERE user_id = OLD.user_id;
    END;

-- One-time cleanup: remove dead admin settings that the server never reads.
-- These will no longer appear in the UI after the default_settings() rewrite.
DELETE FROM settings WHERE key IN (
    'allow_guest_access',
    'transcoding_enabled',
    'default_quality'
);
