# Orthanc

A modern, self-hosted media streaming server built in Rust - a Plex alternative focused on performance and flexibility.

## Overview

Orthanc is a complete media streaming solution that allows you to host and stream your personal video library across multiple devices. Built with Rust for the backend and Dioxus for cross-platform clients, Orthanc prioritizes performance, reliability, and a seamless user experience.

## Features

### Media Management
- **TV Shows Library**: Organize and browse your television series with season and episode management
- **Movies Library**: Comprehensive movie library with metadata support
- **Automatic Library Scanning**: Detect and catalog new media automatically

### Streaming & Transcoding
- **Software Transcoding**: CPU-based transcoding for broad compatibility
- **Hardware Transcoding**: GPU-accelerated transcoding for improved performance and efficiency
- **Adaptive Streaming**: Automatic quality adjustment based on network conditions
- **Multi-Format Support**: Play various video formats across different devices

### Cross-Platform Clients
Built with Dioxus for a unified codebase across platforms:
- **Web Client**: Browser-based access from any device
- **Mobile Apps**: Native mobile experience (iOS/Android)
- **Desktop Apps**: Native desktop applications

## Architecture

### Server (`orthanc_server`)
- High-performance Rust backend
- RESTful API for client communication
- Media library management and scanning
- Transcoding pipeline (software and hardware)
- User authentication and session management

### Client (`orthanc_client`)
- Dioxus-based cross-platform UI
- Multiple compilation targets:
  - Web (WASM)
  - iOS
  - Android
  - Desktop (Windows, macOS, Linux)

## Project Status

This project is currently in early development. The core architecture and initial features are being implemented.

## Technology Stack

- **Backend**: Rust
- **Frontend**: Dioxus (cross-platform UI framework)
- **Transcoding**: FFmpeg (with hardware acceleration support)
- **Database**: TBD
- **API**: REST

## Getting Started

### Prerequisites
- Rust (latest stable)
- FFmpeg (for transcoding)
- Optional: GPU drivers for hardware transcoding

### Building

```bash
# Clone the repository
git clone https://github.com/yourusername/orthanc.git
cd orthanc

# Build the server
cargo build --release -p orthanc_server

# Build the web client
cargo build --release -p orthanc_client --target wasm32-unknown-unknown
```

### Running

```bash
# Start the server
cargo run -p orthanc_server

# Development web client
cargo run -p orthanc_client
```

## Roadmap

- [ ] Core server implementation
- [ ] Media library scanning and indexing
- [ ] Basic video streaming
- [ ] Software transcoding
- [ ] Hardware transcoding support
- [ ] Web client UI
- [ ] User authentication
- [ ] Mobile app builds
- [ ] Metadata fetching (TMDB/TVDB integration)
- [ ] Subtitle support
- [ ] Multi-user support
- [ ] Watch history and resume playback

## Contributing

Contributions are welcome! This project is in early stages, so expect rapid changes.

## License

TBD

## Acknowledgments

Inspired by Plex, Jellyfin, and other self-hosted media solutions.
