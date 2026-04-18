-- Metadata provider configuration per library
CREATE TABLE IF NOT EXISTS library_metadata_providers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    library_id INTEGER NOT NULL,
    provider TEXT NOT NULL CHECK(provider IN ('tmdb')),
    is_enabled BOOLEAN NOT NULL DEFAULT 1,
    priority INTEGER NOT NULL DEFAULT 0, -- Lower = higher priority
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,

    UNIQUE(library_id, provider),
    FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE
);

CREATE INDEX idx_library_metadata_providers_library ON library_metadata_providers(library_id);

-- Insert default TMDB provider for all existing libraries
INSERT INTO library_metadata_providers (library_id, provider, is_enabled, priority)
SELECT id, 'tmdb', 1, 0 FROM libraries;
