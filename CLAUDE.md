# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Orthanc is a self-hosted streaming server for movies and TV shows (Plex alternative) built with Rust. The project consists of:
- **orthanc_server**: Backend server for movie/TV show management, streaming, and transcoding
- **orthanc_ui**: Cross-platform UI client built with Dioxus (supports web, desktop, mobile)

The project is in early development. See PROJECT_PLAN.md for the full roadmap.

## Workspace Structure

This is a Cargo workspace with two main applications:
- `apps/orthanc_server/`: Backend server application
- `apps/orthanc_ui/`: Dioxus-based frontend application

Shared dependencies are defined in the workspace root `Cargo.toml` with version control managed at the workspace level.

## Development Commands

### Server Development
```bash
# Run the server (development)
cargo run -p orthanc_server

# Build the server (release)
cargo build --release -p orthanc_server

# Run server tests
cargo test -p orthanc_server

# Run a specific test
cargo test -p orthanc_server test_name
```

### Database Management
The server uses SQLx with SQLite and migrations. The `sqlx-cli` tool is available in the nix shell.

```bash
# Create a new migration
sqlx migrate add -r <migration_name>

# Run all pending migrations
sqlx migrate run --database-url sqlite:./orthanc.db

# Revert the last migration
sqlx migrate revert --database-url sqlite:./orthanc.db

# Check migration status
sqlx migrate info --database-url sqlite:./orthanc.db

# The server automatically runs migrations on startup
cargo run -p orthanc_server
```

**Important**: Migrations are located in `apps/orthanc_server/migrations/`. When creating new migrations, make sure you're in the `apps/orthanc_server` directory or specify the `--source` flag.

### UI Development
The UI uses Dioxus and requires the `dx` CLI for development.

```bash
# Serve the UI for web (development)
dx serve --platform web

# Serve for desktop
dx serve --platform desktop

# Build for web (WASM target)
cargo build --release -p orthanc_ui --target wasm32-unknown-unknown --features web

# Build for desktop
cargo build --release -p orthanc_ui --features desktop
```

### Workspace Commands
```bash
# Build entire workspace
cargo build

# Run all tests across workspace
cargo test

# Check all code
cargo check

# Format code
cargo fmt

# Run clippy linter
cargo clippy --all-targets --all-features
```

### Adding dependencies

To add a dependency, first call `cargo add <dependency> -p orthanc_server`. This
will resolve to the newest version. Then look at the `orthan_server` `Cargo.toml`.
Remove the dependency from there, add it to the workspace `Cargo.toml`, and then
add the dependency to the `Cargo.toml` of the project that needs it using
`<dependency>.workspace = true`.

## Architecture Notes

### Server (orthanc_server)
- Uses tracing for logging with `RUST_LOG` environment variable support
- Default log level: `orthanc_server=debug,info`
- SQLx with SQLite for database (WAL mode enabled for better concurrency)
- Database connection pooling configured (default: 10 max connections, 2 min connections)
- Automatic migration runner on startup
- Database module at `src/db.rs` handles all database operations
- Planned features: Axum web server, media library scanning, FFmpeg transcoding

### UI (orthanc_ui)
- Built with Dioxus 0.7+ with router and fullstack features
- Supports multiple platforms via Cargo features: `web`, `desktop`, `mobile`, `server`
- Uses automatic Tailwind CSS (no manual setup required in Dioxus 0.7+)
- Router-based navigation defined in `src/main.rs` with `Route` enum
- Components organized in `src/components/`, views in `src/views/`
- Assets in `assets/` directory, referenced via `asset!` macro

### Code Organization
- **orthanc_server structure**:
  - `src/main.rs`: Server entry point, initializes database and logging
  - `src/db.rs`: Database module with connection pooling, migrations, and utilities
  - `migrations/`: SQL migration files (managed by SQLx)

- **orthanc_ui structure**:
  - `src/main.rs`: App entry point and route definitions
  - `src/components/`: Reusable UI components (Hero, Echo, etc.)
  - `src/views/`: Route-specific views (Home, Blog, Navbar)
  - `assets/`: Static assets (CSS, icons, images)

## Technology Stack

- **Language**: Rust (2024 edition)
- **Backend**: Tokio async runtime, planned Axum web framework
- **Frontend**: Dioxus 0.7.5 with router
- **Database**: SQLx 0.8 with SQLite (WAL mode, connection pooling)
- **Transcoding**: FFmpeg (planned)
- **Logging**: tracing + tracing-subscriber
- **Error handling**: anyhow for applications, thiserror for libraries

## Current Development Phase

The project is in Phase 1 (Foundation & Core Infrastructure) according to PROJECT_PLAN.md. Completed:
- ✅ Rust workspace setup
- ✅ Server and client packages created
- ✅ Logging framework configured
- ✅ Error handling framework set up
- ✅ Database setup with SQLx and SQLite
- ✅ Migration system configured
- ✅ Comprehensive initial schema (users, media items, libraries, playback tracking, etc.)

Next immediate steps involve:
- Web server foundation (Axum)
- Basic API endpoints
- Authentication system

## Important Conventions

### Dependencies
- Use workspace dependencies for shared packages (tokio, serde, tracing, etc.)
- Version and metadata managed at workspace level in root Cargo.toml
- Package-specific Cargo.toml files use `workspace = true` for shared config

### Logging
- Server uses structured tracing with `tracing` and `tracing-subscriber`
- Control log levels via `RUST_LOG` environment variable
- Example: `RUST_LOG=orthanc_server=debug,info cargo run -p orthanc_server`

### Database
- SQLite with WAL (Write-Ahead Logging) mode enabled for better concurrent access
- Connection pooling configured via `DbConfig` in `src/db.rs`
- Database URL and pool settings configurable via environment variables (see `.env.example`)
- Migrations run automatically on server startup
- Migration files located in `apps/orthanc_server/migrations/`
- Use `sqlx migrate` commands for manual migration management
- Default database location: `./orthanc.db` (relative to workspace root)

### Database Schema
The initial schema includes tables for:
- **Users & Authentication**: users, user_sessions (JWT refresh tokens), password_reset_tokens
- **Media Libraries**: libraries, library_paths (movies and TV shows only)
- **Media Items**: media_items (movies, TV shows, seasons, episodes), media_streams, images
- **Metadata**: genres, people, media_credits, media_genres
- **Playback**: playback_sessions (watch history, resume points)
- **Transcoding**: transcoding_profiles
- **Collections**: user-created playlists/collections
- **System**: settings, tasks (background jobs)

All tables include proper indexes for performance and foreign key constraints for data integrity.

### Authentication Architecture
- **JWT-based authentication**: Stateless access tokens with refresh token management
- **Refresh tokens**: Stored in `user_sessions` table with device tracking
- **Session management**: Track devices, IP addresses, and allow users to revoke sessions
- **Password reset**: Secure token-based password recovery via `password_reset_tokens`

### Dioxus Assets
- Reference assets using `asset!` macro: `asset!("/assets/file.ext")`
- Assets are automatically processed and bundled appropriately per platform
- CSS files in assets are automatically minified

### Features and Platform Targets
- The orthanc_ui package uses feature flags for platform-specific builds
- Always specify the appropriate feature when building: `--features web|desktop|mobile|server`
- The `dx serve` command handles feature selection automatically based on `--platform` flag

## Testing Strategy

Currently no tests implemented. When adding tests:
- Unit tests should live alongside code in modules
- Integration tests go in `tests/` directory
- Use `cargo test -p <package>` to test individual packages
- Server tests will eventually need database fixtures and test data
