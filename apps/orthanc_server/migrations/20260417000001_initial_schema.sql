-- Initial schema for Orthanc media server
-- This migration creates the foundational tables for movies and TV shows streaming,
-- user authentication, and playback functionality

-- ============================================================================
-- Users & Authentication
-- ============================================================================

-- Users table for authentication and authorization
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE,
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    display_name TEXT,
    is_admin BOOLEAN NOT NULL DEFAULT 0,
    is_active BOOLEAN NOT NULL DEFAULT 1,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_login_at TIMESTAMP
);

CREATE INDEX idx_users_username ON users(username);
CREATE INDEX idx_users_email ON users(email);

CREATE TRIGGER trg_users_updated_at
    AFTER UPDATE ON users FOR EACH ROW
    BEGIN UPDATE users SET updated_at = CURRENT_TIMESTAMP WHERE id = OLD.id; END;

-- User sessions for JWT refresh token management
-- Access tokens are stateless JWTs, but we track refresh tokens for device management and revocation
CREATE TABLE IF NOT EXISTS user_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    refresh_token_hash TEXT NOT NULL UNIQUE, -- Hashed refresh token for security
    device_name TEXT, -- User-friendly device name (e.g., "Chrome on Windows", "iPhone 12")
    device_id TEXT, -- Unique device identifier
    ip_address TEXT, -- Last known IP address
    user_agent TEXT, -- Browser/client user agent string

    -- Session lifecycle
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP NOT NULL, -- Refresh token expiration
    last_used_at TIMESTAMP, -- Last time refresh token was used
    is_revoked BOOLEAN NOT NULL DEFAULT 0, -- Manual revocation flag

    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_user_sessions_refresh_token_hash ON user_sessions(refresh_token_hash);
CREATE INDEX idx_user_sessions_user_id ON user_sessions(user_id);
CREATE INDEX idx_user_sessions_device_id ON user_sessions(device_id);

-- Password reset tokens for secure password recovery
CREATE TABLE IF NOT EXISTS password_reset_tokens (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMP NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    used_at TIMESTAMP, -- When the token was used (NULL if not used yet)
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_password_reset_tokens_token_hash ON password_reset_tokens(token_hash);
CREATE INDEX idx_password_reset_tokens_user_id ON password_reset_tokens(user_id);

-- ============================================================================
-- Media Libraries
-- ============================================================================

-- Media libraries (collections of media files from specific paths)
CREATE TABLE IF NOT EXISTS libraries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    library_type TEXT NOT NULL CHECK(library_type IN ('movies', 'tv_shows')),
    description TEXT,
    is_enabled BOOLEAN NOT NULL DEFAULT 1,
    scan_interval_minutes INTEGER, -- How often to auto-scan, NULL = manual only
    last_scan_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER trg_libraries_updated_at
    AFTER UPDATE ON libraries FOR EACH ROW
    BEGIN UPDATE libraries SET updated_at = CURRENT_TIMESTAMP WHERE id = OLD.id; END;

-- Library-level user access permissions
-- Admins bypass this; non-admin users can only see libraries they have a row in this table for
CREATE TABLE IF NOT EXISTS library_users (
    library_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (library_id, user_id),
    FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- Library paths (each library can have multiple filesystem paths)
CREATE TABLE IF NOT EXISTS library_paths (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    library_id INTEGER NOT NULL,
    path TEXT NOT NULL,
    is_enabled BOOLEAN NOT NULL DEFAULT 1,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE
);

CREATE INDEX idx_library_paths_library_id ON library_paths(library_id);

-- ============================================================================
-- Media Items
-- ============================================================================

-- Media items (movies, TV shows, seasons, episodes)
-- Uses single-table hierarchy with parent_id for TV show -> season -> episode.
-- library_id is required for top-level items (movies, tv_shows) and NULL for
-- child items (seasons, episodes) which inherit the library from their parent.
CREATE TABLE IF NOT EXISTS media_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    library_id INTEGER, -- Required for movies/tv_shows, NULL for seasons/episodes
    media_type TEXT NOT NULL CHECK(media_type IN ('movie', 'tv_show', 'season', 'episode')),
    title TEXT NOT NULL,
    sort_title TEXT, -- For proper alphabetical sorting
    original_title TEXT,
    description TEXT,
    release_date DATE,
    duration_seconds INTEGER,
    file_path TEXT, -- Physical file location (NULL for containers like TV shows/seasons)
    file_size_bytes INTEGER,
    mime_type TEXT,
    container_format TEXT, -- e.g., "mkv", "mp4", "avi"

    -- Metadata
    rating REAL, -- 0.0 to 10.0
    content_rating TEXT, -- e.g., "PG-13", "R", "TV-MA"
    tagline TEXT,

    -- External IDs for metadata providers
    imdb_id TEXT,
    tmdb_id TEXT,
    tvdb_id TEXT,

    -- Hierarchy (for TV shows and seasons)
    parent_id INTEGER, -- References another media_item (TV show -> season -> episode)
    season_number INTEGER,
    episode_number INTEGER,

    -- File metadata
    file_hash TEXT, -- SHA256 or similar for deduplication
    date_added TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    date_modified TIMESTAMP,
    last_scanned_at TIMESTAMP,

    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,

    -- Top-level items must belong to a library; child items inherit from parent
    CHECK (media_type IN ('season', 'episode') OR library_id IS NOT NULL),

    FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE,
    FOREIGN KEY (parent_id) REFERENCES media_items(id) ON DELETE CASCADE
);

CREATE INDEX idx_media_items_library_id ON media_items(library_id);
CREATE INDEX idx_media_items_media_type ON media_items(media_type);
CREATE INDEX idx_media_items_parent_id ON media_items(parent_id);
CREATE INDEX idx_media_items_file_path ON media_items(file_path);
CREATE INDEX idx_media_items_imdb_id ON media_items(imdb_id);
CREATE INDEX idx_media_items_tmdb_id ON media_items(tmdb_id);
CREATE INDEX idx_media_items_sort_title ON media_items(sort_title);
CREATE INDEX idx_media_items_release_date ON media_items(release_date);
CREATE INDEX idx_media_items_date_added ON media_items(date_added);

-- Prevent duplicate seasons within a show or episodes within a season
CREATE UNIQUE INDEX idx_media_items_unique_season
    ON media_items(parent_id, season_number) WHERE media_type = 'season';
CREATE UNIQUE INDEX idx_media_items_unique_episode
    ON media_items(parent_id, episode_number) WHERE media_type = 'episode';

CREATE TRIGGER trg_media_items_updated_at
    AFTER UPDATE ON media_items FOR EACH ROW
    BEGIN UPDATE media_items SET updated_at = CURRENT_TIMESTAMP WHERE id = OLD.id; END;

-- Media streams (video, audio, subtitle tracks within media files)
CREATE TABLE IF NOT EXISTS media_streams (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    media_item_id INTEGER NOT NULL,
    stream_index INTEGER NOT NULL, -- Index within the file
    stream_type TEXT NOT NULL CHECK(stream_type IN ('video', 'audio', 'subtitle')),
    codec TEXT,
    language TEXT, -- ISO 639-2 language code
    title TEXT, -- Stream title (e.g., "English", "Director's Commentary")
    is_default BOOLEAN NOT NULL DEFAULT 0,
    is_forced BOOLEAN NOT NULL DEFAULT 0,

    -- Video-specific
    width INTEGER,
    height INTEGER,
    aspect_ratio TEXT,
    frame_rate REAL,
    bit_depth INTEGER,
    color_space TEXT,

    -- Audio-specific
    channels INTEGER,
    sample_rate INTEGER,
    bit_rate INTEGER,

    -- Subtitle-specific
    is_external BOOLEAN NOT NULL DEFAULT 0, -- True if subtitle is a separate file
    external_file_path TEXT,

    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,

    FOREIGN KEY (media_item_id) REFERENCES media_items(id) ON DELETE CASCADE
);

CREATE INDEX idx_media_streams_media_item_id ON media_streams(media_item_id);
CREATE INDEX idx_media_streams_type ON media_streams(stream_type);

-- ============================================================================
-- Metadata (Genres, People, Credits, Images)
-- ============================================================================

-- Genres
CREATE TABLE IF NOT EXISTS genres (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Media items to genres (many-to-many)
CREATE TABLE IF NOT EXISTS media_genres (
    media_item_id INTEGER NOT NULL,
    genre_id INTEGER NOT NULL,
    PRIMARY KEY (media_item_id, genre_id),
    FOREIGN KEY (media_item_id) REFERENCES media_items(id) ON DELETE CASCADE,
    FOREIGN KEY (genre_id) REFERENCES genres(id) ON DELETE CASCADE
);

-- People (actors, directors, writers, etc.)
CREATE TABLE IF NOT EXISTS people (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    biography TEXT,
    birth_date DATE,
    death_date DATE,
    profile_image_url TEXT,
    imdb_id TEXT,
    tmdb_id TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_people_name ON people(name);
CREATE INDEX idx_people_imdb_id ON people(imdb_id);

CREATE TRIGGER trg_people_updated_at
    AFTER UPDATE ON people FOR EACH ROW
    BEGIN UPDATE people SET updated_at = CURRENT_TIMESTAMP WHERE id = OLD.id; END;

-- Media credits (cast and crew)
CREATE TABLE IF NOT EXISTS media_credits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    media_item_id INTEGER NOT NULL,
    person_id INTEGER NOT NULL,
    role_type TEXT NOT NULL CHECK(role_type IN ('actor', 'director', 'writer', 'producer', 'composer', 'cinematographer', 'editor')),
    character_name TEXT, -- For actors
    credit_order INTEGER, -- For sorting (e.g., billing order)
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (media_item_id) REFERENCES media_items(id) ON DELETE CASCADE,
    FOREIGN KEY (person_id) REFERENCES people(id) ON DELETE CASCADE
);

CREATE INDEX idx_media_credits_media_item_id ON media_credits(media_item_id);
CREATE INDEX idx_media_credits_person_id ON media_credits(person_id);

-- Images (posters, backdrops, thumbnails)
CREATE TABLE IF NOT EXISTS images (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    media_item_id INTEGER,
    person_id INTEGER,
    image_type TEXT NOT NULL CHECK(image_type IN ('poster', 'backdrop', 'thumbnail', 'profile', 'screenshot', 'logo')),
    url TEXT,
    file_path TEXT, -- Local cached copy
    width INTEGER,
    height INTEGER,
    aspect_ratio REAL,
    is_primary BOOLEAN NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,

    -- Every image must belong to something
    CHECK (media_item_id IS NOT NULL OR person_id IS NOT NULL),

    FOREIGN KEY (media_item_id) REFERENCES media_items(id) ON DELETE CASCADE,
    FOREIGN KEY (person_id) REFERENCES people(id) ON DELETE CASCADE
);

CREATE INDEX idx_images_media_item_id ON images(media_item_id);
CREATE INDEX idx_images_person_id ON images(person_id);

-- ============================================================================
-- Playback & Watch History
-- ============================================================================

-- User media progress (canonical resume point per user+media_item)
-- One row per user+item. Use this for "continue watching" and "mark as watched".
CREATE TABLE IF NOT EXISTS user_media_progress (
    user_id INTEGER NOT NULL,
    media_item_id INTEGER NOT NULL,
    playback_position_seconds INTEGER NOT NULL DEFAULT 0,
    is_completed BOOLEAN NOT NULL DEFAULT 0,
    completed_at TIMESTAMP,
    last_updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_id, media_item_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (media_item_id) REFERENCES media_items(id) ON DELETE CASCADE
);

CREATE INDEX idx_user_media_progress_user_updated
    ON user_media_progress(user_id, last_updated_at);

-- Playback sessions (full history log of every viewing session)
CREATE TABLE IF NOT EXISTS playback_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    media_item_id INTEGER NOT NULL,
    started_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    ended_at TIMESTAMP,
    duration_watched_seconds INTEGER, -- How long they actually watched in this session

    -- Playback context
    client_name TEXT, -- e.g., "Web", "Android", "iOS"
    client_version TEXT,

    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (media_item_id) REFERENCES media_items(id) ON DELETE CASCADE
);

CREATE INDEX idx_playback_sessions_user_id ON playback_sessions(user_id);
CREATE INDEX idx_playback_sessions_media_item_id ON playback_sessions(media_item_id);

-- ============================================================================
-- Transcoding
-- ============================================================================

-- Transcoding profiles for adaptive streaming
CREATE TABLE IF NOT EXISTS transcoding_profiles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    container_format TEXT NOT NULL, -- e.g., "mp4", "hls", "dash"

    -- Video encoding
    video_codec TEXT, -- e.g., "h264", "h265", "vp9"
    video_bitrate_kbps INTEGER,
    video_width INTEGER,
    video_height INTEGER,
    video_frame_rate REAL,

    -- Audio encoding
    audio_codec TEXT, -- e.g., "aac", "opus", "mp3"
    audio_bitrate_kbps INTEGER,
    audio_channels INTEGER,
    audio_sample_rate INTEGER,

    is_default BOOLEAN NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER trg_transcoding_profiles_updated_at
    AFTER UPDATE ON transcoding_profiles FOR EACH ROW
    BEGIN UPDATE transcoding_profiles SET updated_at = CURRENT_TIMESTAMP WHERE id = OLD.id; END;

-- ============================================================================
-- Collections
-- ============================================================================

-- Collections (user-defined playlists, favorites, watchlists)
CREATE TABLE IF NOT EXISTS collections (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER, -- NULL = shared/public collection
    name TEXT NOT NULL,
    description TEXT,
    collection_type TEXT NOT NULL CHECK(collection_type IN ('playlist', 'favorites', 'watchlist')) DEFAULT 'playlist',
    is_public BOOLEAN NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_collections_user_id ON collections(user_id);

CREATE TRIGGER trg_collections_updated_at
    AFTER UPDATE ON collections FOR EACH ROW
    BEGIN UPDATE collections SET updated_at = CURRENT_TIMESTAMP WHERE id = OLD.id; END;

-- Collection items (many-to-many)
CREATE TABLE IF NOT EXISTS collection_items (
    collection_id INTEGER NOT NULL,
    media_item_id INTEGER NOT NULL,
    item_order INTEGER NOT NULL DEFAULT 0,
    added_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (collection_id, media_item_id),
    FOREIGN KEY (collection_id) REFERENCES collections(id) ON DELETE CASCADE,
    FOREIGN KEY (media_item_id) REFERENCES media_items(id) ON DELETE CASCADE
);

-- ============================================================================
-- System
-- ============================================================================

-- Server settings/configuration
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    value_type TEXT NOT NULL CHECK(value_type IN ('string', 'integer', 'boolean', 'json')),
    description TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER trg_settings_updated_at
    AFTER UPDATE ON settings FOR EACH ROW
    BEGIN UPDATE settings SET updated_at = CURRENT_TIMESTAMP WHERE key = OLD.key; END;

-- Background tasks/jobs (for library scanning, transcoding, etc.)
CREATE TABLE IF NOT EXISTS tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_type TEXT NOT NULL CHECK(task_type IN ('library_scan', 'metadata_refresh', 'transcode', 'thumbnail_generation', 'cleanup')),
    status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'cancelled')) DEFAULT 'pending',
    priority INTEGER NOT NULL DEFAULT 0,

    -- Task context
    library_id INTEGER,
    media_item_id INTEGER,
    transcoding_profile_id INTEGER, -- Which profile to use for transcode tasks

    -- Progress tracking
    progress_percentage INTEGER DEFAULT 0,
    current_step TEXT,
    error_message TEXT,

    scheduled_at TIMESTAMP,
    started_at TIMESTAMP,
    completed_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,

    FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE,
    FOREIGN KEY (media_item_id) REFERENCES media_items(id) ON DELETE CASCADE,
    FOREIGN KEY (transcoding_profile_id) REFERENCES transcoding_profiles(id) ON DELETE SET NULL
);

CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_tasks_task_type ON tasks(task_type);
CREATE INDEX idx_tasks_library_id ON tasks(library_id);
