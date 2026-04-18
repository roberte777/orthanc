-- Drop and recreate the provider CHECK to allow 'tvdb'
-- SQLite doesn't support ALTER CHECK, so we recreate the table
CREATE TABLE library_metadata_providers_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    library_id INTEGER NOT NULL,
    provider TEXT NOT NULL CHECK(provider IN ('tmdb', 'anidb', 'tvdb')),
    is_enabled BOOLEAN NOT NULL DEFAULT 1,
    priority INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,

    UNIQUE(library_id, provider),
    FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE
);

INSERT INTO library_metadata_providers_new (id, library_id, provider, is_enabled, priority, created_at)
SELECT id, library_id, provider, is_enabled, priority, created_at
FROM library_metadata_providers;

DROP TABLE library_metadata_providers;
ALTER TABLE library_metadata_providers_new RENAME TO library_metadata_providers;

CREATE INDEX idx_library_metadata_providers_library_v3 ON library_metadata_providers(library_id);

-- Add TVDB as disabled by default for all existing libraries
INSERT OR IGNORE INTO library_metadata_providers (library_id, provider, is_enabled, priority)
SELECT id, 'tvdb', 0, 20 FROM libraries;
