# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Orthanc is a self-hosted streaming server for movies and TV shows (Plex alternative) built with Rust. The project consists of:
- **orthanc_server**: Backend server for movie/TV show management, streaming, and transcoding
- **orthanc_ui**: Web/desktop UI client built with Dioxus
- **orthanc_ui_mobile**: Netflix-style mobile UI client (iOS + Android) built with Dioxus
- **orthanc_core**: Shared library — API client, auth state, storage trait, theme tokens, formatters

The project is in early development. See PROJECT_PLAN.md for the full roadmap.

## Workspace Structure

```
orthanc/
├── apps/
│   ├── orthanc_server/       # Axum backend
│   ├── orthanc_ui/           # Dioxus web/desktop UI
│   └── orthanc_ui_mobile/    # Dioxus mobile UI (iOS + Android)
└── crates/
    └── orthanc_core/         # Shared library (no UI components)
```

Both UI crates depend on `orthanc_core` for the API client, DTOs, auth state, and the `TokenStorage` trait. Each UI crate provides its own `TokenStorage` impl: web uses `localStorage`, mobile uses the platform Keychain (with a JSON file fallback).

Shared dependencies are defined in the workspace root `Cargo.toml` with version control managed at the workspace level. Internal crates are also exposed via workspace deps (e.g. `orthanc_core = { workspace = true }`).

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

### UI Development (web/desktop)
The UI uses Dioxus and requires the `dx` CLI for development.

```bash
# Serve the UI for web (development) — from inside the orthanc_ui crate
cd apps/orthanc_ui && dx serve --platform web

# Serve for desktop
cd apps/orthanc_ui && dx serve --platform desktop

# Build for web (WASM target)
cargo build --release -p orthanc_ui --target wasm32-unknown-unknown --features web

# Build for desktop
cargo build --release -p orthanc_ui --features desktop
```

### Mobile UI Development (iOS + Android)
The mobile crate is a separate Dioxus app with Netflix-style chrome — bottom-tab nav, inline custom player with native HLS, touch-driven scrubbing.

```bash
# Fastest dev loop: run the mobile UI in a desktop window. No simulator
# needed; touch gestures degrade to clicks but layout/state behave the same.
cd apps/orthanc_ui_mobile && dx serve --platform desktop --no-default-features --features desktop

# Run on iOS simulator (requires Xcode + xcrun simctl on the host)
xcrun simctl boot "iPhone 15"  # one-time, then leave it booted
cd apps/orthanc_ui_mobile && dx serve --platform ios

# Run on Android emulator
cd apps/orthanc_ui_mobile && dx serve --platform android

# Bundle for distribution
dx bundle --release --platform ios     # produces .ipa
dx bundle --release --platform android # produces .apk / .aab
```

iOS development requires Xcode + Command Line Tools installed on the host (Apple does not allow Xcode in nix). Android dev gets its NDK + SDK from the nix dev shell — see `flake.nix`. The first `dx serve --platform android` triggers a multi-GB SDK download.

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

### UI (orthanc_ui — web/desktop)
- Built with Dioxus 0.7+ with router and fullstack features
- Cargo features: `web` (default), `desktop`, `mobile`, `server` — only `web` is actively built today
- Hand-written CSS in `assets/styling/main.css` (the workspace does not use Tailwind despite earlier docs claiming so)
- Uses HLS.js (loaded via CDN in `Dioxus.toml`) for adaptive streaming + MediaSource codec probing
- Router-based navigation defined in `src/main.rs` with `Route` enum
- Imports API client / auth state / formatters from `orthanc_core` — there is a thin `mod api` shim in `src/api/mod.rs` and `mod state` in `src/state.rs` that re-export from `orthanc_core` so existing `crate::api::...` paths keep working
- Provides a `WebStorage` (`src/storage.rs`) `TokenStorage` impl backed by `localStorage`

### UI (orthanc_ui_mobile — iOS + Android)
- Separate Dioxus crate per the official Dioxus team recommendation for platform-divergent UIs (bottom-tabs vs sidebar, custom touch player vs HLS.js, gestures vs keyboard)
- Cargo features: `mobile` (default), `desktop` (for fast dev-loop iteration in a desktop window)
- Bottom-tab nav (`src/components/bottom_nav.rs`) wraps Home + Profile in `MobileShell`; Detail and Player are full-screen routes outside the shell
- Inline custom Netflix-style player (`src/views/player.rs`) — uses HTML5 `<video playsinline>` with native HLS in WKWebView/Android WebView (no HLS.js). All JS interop goes through `document::eval()` (no `web_sys` — that's web-only)
- Subtitles: VTT delivery via `<track>` elements when the server returns `delivery: "vtt"`; falls back to server-side burn-in for image-based PGS/VobSub (delivery: "burn_required")
- Audio track switching re-fetches `stream-token` with a new `audio_stream_id` because `<video>.audioTracks` is not implemented in WebKit / Android WebView
- Token storage via the `keyring` crate (iOS Keychain, macOS Keychain) with a JSON file fallback for Android (Android Keystore integration is a v2 polish item via the `android-keyring` companion crate)
- Server URL captured at first launch via `/server-config` and persisted via the `Storage` trait; `App` reads it at startup and calls `orthanc_core::api::set_base_url`
- iOS Info.plist customised in `Dioxus.toml` `[ios.plist]` with `NSAllowsLocalNetworking = true` (App Store-friendly LAN HTTP exception) and supported orientations (portrait + landscape for player)
- Android cleartext HTTP enabled in `Dioxus.toml` `[android.application]`

### Code Organization
- **orthanc_server structure**:
  - `src/main.rs`: Server entry point, initializes database and logging
  - `src/db.rs`: Database module with connection pooling, migrations, and utilities
  - `migrations/`: SQL migration files (managed by SQLx)

- **orthanc_ui structure** (web/desktop):
  - `src/main.rs`: App entry point, route definitions, provides `WebStorage` + `AuthState` via context
  - `src/storage.rs`: `WebStorage` impl of `orthanc_core::storage::TokenStorage`
  - `src/components/`: Reusable UI components
  - `src/views/`: Route-specific views (login, setup, home, browse_movies, browse_shows, player, settings, admin_*)
  - `assets/`: Static assets including `styling/main.css`

- **orthanc_ui_mobile structure**:
  - `src/main.rs`: App entry point, mobile `Route` enum, provides `KeyringStorage` via context
  - `src/storage.rs`: `KeyringStorage` impl of `orthanc_core::storage::TokenStorage`
  - `src/components/`: `bottom_nav`, `media_card`, `horizontal_row`, `hero_section`
  - `src/views/`: `mobile_shell` (layout), `server_config`, `login`, `home`, `detail`, `player`, `profile`
  - `assets/styling/mobile.css`: Mobile-specific Netflix-themed styles
  - `Dioxus.toml`: iOS/Android bundle config (Info.plist merge, cleartext HTTP, orientations)

- **orthanc_core structure** (shared library):
  - `src/api/`: API client, all DTO types — pure `reqwest`, no platform deps
  - `src/auth.rs`: `AuthState`, `with_refresh` (reads `Storage` from Dioxus context), `save_auth` / `clear_auth`
  - `src/storage.rs`: `TokenStorage` trait + `Storage` type alias (`Arc<dyn TokenStorage>`)
  - `src/formatters.rs`: `format_size`, `format_year`, `format_runtime`, `format_time`
  - `src/player_logic.rs`: `subtitle_label`, `audio_label`, `mobile_capabilities` (per-OS hardcoded codec lists)
  - `src/theme.rs`: Netflix color/spacing constants

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
