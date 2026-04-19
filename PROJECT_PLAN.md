# Orthanc Server Implementation Plan

This document outlines the implementation steps for building the Orthanc media streaming server.

## Phase 1: Foundation & Core Infrastructure

### 1.1 Project Setup
- [x] Initialize Rust workspace
- [x] Create server and client packages
- [x] Set up logging framework (tracing/tracing-subscriber)
- [x] Configure error handling (thiserror/anyhow)

### 1.2 Database Setup
- [ ] Choose database (SQLite for simplicity, PostgreSQL for scalability)
- [ ] Set up database connection pool
- [ ] Create migration system (sqlx, diesel, or sea-orm)
- [ ] Design initial schema:
  - Users table
  - Libraries table
  - Movies table
  - TV Shows table
  - Seasons table
  - Episodes table
  - Media files table
  - Transcoding jobs table

### 1.3 Web Server Foundation
- [ ] Choose web framework (axum, actix-web, or rocket)
- [ ] Set up basic HTTP server
- [ ] Configure CORS for client access
- [ ] Implement request logging middleware
- [ ] Set up static file serving
- [ ] Create health check endpoint

## Phase 2: Authentication & User Management

### 2.1 User System
- [x] Implement user registration
- [x] Implement password hashing (argon2 or bcrypt)
- [x] Create user login endpoint
- [x] Implement JWT or session-based authentication
- [x] Add authentication middleware
- [x] Create user profile endpoints
- [x] Add user settings storage

### 2.2 Authorization
- [x] Implement role-based access control (admin, user)
- [x] Add library-level permissions
- [x] Protect API endpoints with auth middleware

## Phase 3: Media Library Management

### 3.1 Library Setup
- [x] Create library configuration API
- [x] Implement library CRUD endpoints
- [x] Add library type support (Movies, TV Shows)
- [x] Store library paths and settings
- [x] Validate library paths exist and are accessible

### 3.2 File System Scanning
- [x] Implement directory walker
- [x] Identify video file types
- [x] Extract file metadata (size, creation date, etc.)
- [x] Parse filenames for metadata hints
- [x] Implement TV show naming detection (S01E01 format)
- [x] Implement movie naming detection
- [x] Handle multi-file movies (parts, discs)
- [x] Store file paths and basic info in database

### 3.3 Metadata Fetching
- [ ] Per library, allow enable / disable/ priority order for metadata sources
- [ ] Integrate with TMDB API for movies
- [ ] Integrate with TMDB/TVDB API for TV shows
- [ ] Fetch posters, backdrops, and thumbnails
- [ ] Download and cache artwork locally
- [ ] Store metadata in database
- [ ] Implement metadata refresh functionality. when a new item is added, on a schedule, manually
- [ ] Handle manual metadata override
- [ ] different kinds of refresh:
A standard refresh only updates missing fields and leaves your local edits alone.
A full/replace refresh (sometimes called "Refresh All Metadata" in Plex or "Replace all metadata" in Jellyfin) wipes existing metadata and re-pulls everything from scratch — useful if something got badly mangled, but it'll overwrite manual edits like custom posters or edited descriptions.
- [ ] refresh per item (movie, tv show, season, episode)

### 3.4 Library API
maybe done?
- [ ] Create endpoint to list libraries
- [ ] Create endpoint to list movies in library
- [ ] Create endpoint to list TV shows in library
- [ ] Create endpoint to get show seasons/episodes
- [ ] Implement search across libraries
- [ ] Add filtering and sorting options
- [ ] Implement pagination for large libraries

## Phase 4: Video Streaming

### 4.1 Direct Streaming
- [ ] Implement custom player
- [ ] Implement direct file streaming endpoint
- [ ] Support HTTP range requests for seeking
- [ ] Add MIME type detection
- [ ] Implement bandwidth throttling options
- [ ] Add concurrent stream limits

### 4.2 Software Transcoding
- [ ] Integrate FFmpeg library (ffmpeg-next or direct CLI)
- [ ] Implement transcoding profiles (quality presets)
- [ ] Create transcoding job queue
- [ ] Implement HLS (HTTP Live Streaming) generation
- [ ] Generate multiple quality variants
- [ ] Create m3u8 playlist files
- [ ] Implement segment cleanup
- [ ] Add transcoding progress tracking
- [ ] Make sure to support remux, audio transcode, and full video transcoding

### 4.3 Hardware Transcoding
- [ ] Detect available hardware encoders
  - NVIDIA NVENC
  - Intel Quick Sync
  - AMD AMF
  - Apple VideoToolbox (macOS)
- [ ] Implement hardware encoder selection
- [ ] Add fallback to software encoding
- [ ] Configure hardware-specific FFmpeg options
- [ ] Benchmark and optimize settings

### 4.4 Adaptive Streaming
- [ ] Implement quality selection based on client request
- [ ] Add bandwidth detection
- [ ] Create adaptive bitrate ladder
- [ ] Optimize segment duration

## Phase 5: Playback Features

### 5.1 Subtitle Support
- [ ] Extract embedded subtitles from video files
- [ ] Support external subtitle files (.srt, .ass, .vtt)
- [ ] Convert subtitles to WebVTT for streaming
- [ ] Implement subtitle track selection API
- [ ] Add subtitle burning (hardcoded) option

### 5.2 Audio Track Support
- [ ] Detect multiple audio tracks
- [ ] Allow audio track selection
- [ ] Support audio transcoding
- [ ] Handle audio normalization

### 5.3 Watch History
- [ ] Track playback progress per user
- [ ] Store timestamp for resume playback
- [ ] Mark items as watched
- [ ] Create "Continue Watching" endpoint
- [ ] Add "Up Next" suggestions

## Phase 6: Performance & Optimization

### 6.1 Caching
- [ ] Cache metadata API responses
- [ ] Cache artwork/thumbnails
- [ ] Implement transcoding cache
- [ ] Add cache invalidation logic
- [ ] Configure cache size limits

### 6.2 Background Jobs
- [ ] Set up job queue system (tokio tasks or dedicated library)
- [ ] Implement periodic library scanning
- [ ] Add automatic metadata refresh
- [ ] Schedule transcoding cache cleanup
- [ ] Monitor job failures and retries

### 6.3 Monitoring
- [ ] Add performance metrics collection
- [ ] Monitor transcoding queue length
- [ ] Track streaming sessions
- [ ] Log errors and warnings
- [ ] Create admin dashboard data endpoints

## Phase 7: Advanced Features

### 7.1 Multi-User Features
- [ ] Implement per-user watch history
- [ ] Add user-specific recommendations
- [ ] Create shared/personal libraries
- [ ] Add parental controls/content ratings

### 7.2 Collections & Organization
- [ ] Create custom collections
- [ ] Implement movie/show favorites
- [ ] Add watchlists
- [ ] Support custom tags

### 7.3 Notifications
- [ ] Notify when new media is added
- [ ] Alert on transcoding failures
- [ ] Send library scan completion notices

## Phase 8: Deployment & Documentation

### 8.1 Deployment
- [ ] Create Docker image
- [ ] Write docker-compose.yml
- [ ] Add environment variable configuration
- [ ] Create systemd service file
- [ ] Write installation guide

### 8.2 Documentation
- [ ] API documentation (OpenAPI/Swagger)
- [ ] User guide
- [ ] Admin guide
- [ ] Configuration reference
- [ ] Troubleshooting guide

## Technical Decisions to Make

### Immediate Decisions Needed
1. **Web Framework**: axum (modern, tokio-based) vs actix-web (mature, fast) vs rocket (ergonomic)
2. **Database**: SQLite (simple, embedded) vs PostgreSQL (powerful, scalable)
3. **ORM**: SQLx (compile-time checked) vs Diesel (type-safe) vs SeaORM (async, modern)
4. **FFmpeg Integration**: ffmpeg-next (Rust bindings) vs direct CLI calls

### Later Decisions
- Video codec preferences (H.264, H.265/HEVC, AV1)
- Container format for streaming (HLS vs DASH)
- Thumbnail generation strategy
- Cache storage (filesystem vs Redis)

## Success Criteria

### Minimum Viable Product (MVP)
- [ ] User can create an account and login
- [ ] User can add a movie library
- [ ] Server scans library and fetches metadata
- [ ] User can browse movies via API
- [ ] User can stream a video (direct or transcoded)
- [ ] Basic TV show support with seasons/episodes

### Production Ready
- All MVP features plus:
- [ ] Hardware transcoding support
- [ ] Subtitle support
- [ ] Watch history and resume
- [ ] Stable performance under load
- [ ] Docker deployment available
- [ ] Basic admin interface or CLI tools
