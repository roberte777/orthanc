# Orthanc Server

Backend server for the Orthanc streaming platform - movies and TV shows only.

## Features

- **SQLite Database** with SQLx ORM
- **Connection Pooling** for efficient database access
- **Automatic Migrations** on startup
- **WAL Mode** enabled for better concurrent access
- **Structured Logging** with tracing
- **Future-proof Schema** designed for media streaming

## Database Schema

The initial schema includes comprehensive tables for:

- **Users & Authentication**: JWT-based authentication with refresh tokens, password reset, and device session management
- **Media Libraries**: Configurable sources for movies and TV shows with multiple paths
- **Media Items**: Movies, TV shows, seasons, and episodes
- **Metadata**: Genres, people (actors/directors), and credits
- **Media Streams**: Video, audio, and subtitle tracks
- **Playback Tracking**: Watch history and resume points
- **Collections**: User-created playlists
- **Images**: Posters, backdrops, and thumbnails
- **Transcoding**: Profiles for adaptive streaming
- **Background Tasks**: Job queue for scanning and processing

## Quick Start

```bash
# Run the server
cargo run -p orthanc_server

# Run with custom log level
RUST_LOG=debug cargo run -p orthanc_server

# Run tests
cargo test -p orthanc_server
```

## Configuration

Configuration is via environment variables (see `.env.example` in the workspace root):

- `DATABASE_URL`: SQLite database path (default: `sqlite:./orthanc.db`)
- `DATABASE_MAX_CONNECTIONS`: Maximum pool size (default: 10)
- `DATABASE_MIN_CONNECTIONS`: Minimum idle connections (default: 2)
- `DATABASE_CONNECT_TIMEOUT`: Connection timeout in seconds (default: 30)
- `DATABASE_MAX_LIFETIME`: Max connection lifetime in seconds (default: 1800)
- `DATABASE_ENABLE_WAL`: Enable WAL mode (default: true)

## Database Management

```bash
# Create a new migration
sqlx migrate add -r <migration_name>

# Run migrations manually
sqlx migrate run --database-url sqlite:./orthanc.db

# Revert last migration
sqlx migrate revert --database-url sqlite:./orthanc.db

# Check migration status
sqlx migrate info --database-url sqlite:./orthanc.db
```

**Note**: Migrations run automatically on server startup, so manual migration management is optional.

## Project Structure

```
apps/orthanc_server/
├── src/
│   ├── main.rs          # Server entry point
│   └── db.rs            # Database connection and utilities
├── migrations/          # SQL migration files
│   └── 20260417000001_initial_schema.sql
└── README.md            # This file
```

## Development Status

Currently in Phase 1 (Foundation & Core Infrastructure):
- ✅ Database setup complete
- ✅ Migration system working
- ✅ Connection pooling configured
- 🚧 Web server (Axum) - planned
- 🚧 REST API - planned
- 🚧 Authentication - planned
- 🚧 Media library scanning - planned
- 🚧 FFmpeg transcoding - planned

## Future Enhancements

- **Axum Web Server**: REST API for media operations
- **JWT Authentication Implementation**:
  - Login/logout endpoints
  - Access token generation (short-lived, stateless)
  - Refresh token rotation (long-lived, stored in DB)
  - Device management UI
  - Password reset flow
- **Media Scanning**: Automatic library detection and metadata fetching
- **Transcoding**: FFmpeg integration for adaptive streaming
- **Real-time Updates**: WebSocket support for live updates
- **Search**: Full-text search across media items
- **Recommendations**: Content recommendation engine
