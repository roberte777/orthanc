# orthanc_ui

Netflix-style web client for Orthanc, built with Dioxus 0.7.

## Configuration

The UI reads `assets/config.json` at startup to determine the server URL. Edit this file to point to your Orthanc server before building or serving:

```json
{
  "api_url": "http://localhost:8080"
}
```

The default is `http://localhost:8081`. If the file is missing or malformed, the app will display an error and refuse to load. If the server at the configured URL is unreachable, the login page will show an error.

> **Note:** The server (`orthanc_server`) defaults to port `8080`. Make sure the port in `config.json` matches `SERVER_ADDR` in your server's `.env`.

## Development

```bash
# Serve the UI (web)
dx serve --platform web

# In a separate terminal, run the server
cargo run -p orthanc_server
```

## Project structure

```
assets/
  config.json          # Server URL configuration
  styling/main.css     # Global styles
src/
  main.rs              # App entry point and route definitions
  api/mod.rs           # REST API client
  state.rs             # Global auth state
  views/               # Route-specific pages
    login.rs
    setup.rs           # First-run admin account creation
    app_shell.rs       # Authenticated layout with navbar
    home.rs
    settings.rs
    admin_users.rs
    admin_settings.rs
```

## Building

```bash
# Web (WASM)
dx build --platform web --release

# Desktop
dx build --platform desktop --release
```
