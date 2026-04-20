-- Per-user audio & subtitle preferences, scoped to a show (for TV) or a movie.
-- Language codes (not stream_ids) are persisted because stream_ids are per-file
-- and differ across episodes. The server resolves language -> stream at playback.

CREATE TABLE IF NOT EXISTS user_track_preferences (
    user_id INTEGER NOT NULL,
    scope_media_item_id INTEGER NOT NULL, -- show_id for episodes; movie_id for films
    audio_language TEXT,                   -- ISO 639-2; NULL = no preference
    subtitle_language TEXT,                -- ISO 639-2; NULL when subtitles disabled
    subtitles_enabled BOOLEAN NOT NULL DEFAULT 0,
    audio_normalize BOOLEAN NOT NULL DEFAULT 0,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_id, scope_media_item_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (scope_media_item_id) REFERENCES media_items(id) ON DELETE CASCADE
);

CREATE INDEX idx_user_track_preferences_user ON user_track_preferences(user_id);

CREATE TRIGGER trg_user_track_preferences_updated_at
    AFTER UPDATE ON user_track_preferences FOR EACH ROW
    BEGIN
        UPDATE user_track_preferences SET updated_at = CURRENT_TIMESTAMP
        WHERE user_id = OLD.user_id AND scope_media_item_id = OLD.scope_media_item_id;
    END;
