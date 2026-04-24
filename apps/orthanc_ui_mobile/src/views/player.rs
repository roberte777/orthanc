//! Mobile inline custom Netflix-style player.
//!
//! Architecture:
//! - HTML5 `<video>` rendered via rsx, `playsinline` so iOS keeps it inline
//!   instead of bouncing into AVPlayer fullscreen. WKWebView and Android
//!   WebView play HLS streams natively, so no HLS.js is needed.
//! - All JS interop goes through Dioxus's `document::eval()` (no `web_sys`,
//!   which doesn't apply to native mobile builds). A long-running eval
//!   establishes a JS → Rust state-push channel via `dioxus.send()`; commands
//!   in the other direction (play, pause, seek, setSrc) are fire-and-forget
//!   one-shot evals.
//! - Subtitles use `<track>` elements (delivery="vtt") attached imperatively
//!   so we can swap them when the user picks a different language. PGS/VobSub
//!   subtitles fall back to server-side burn-in via the stream-token API,
//!   matching the web player's behavior.
//! - Audio track switching re-requests a stream-token with a new
//!   `audio_stream_id` and swaps `<video>.src` — the WebView's
//!   `video.audioTracks` API is not implemented in WebKit/Android WebView, so
//!   server-side selection is the only path that works.

use dioxus::prelude::*;
use orthanc_core::api::{self, AudioTrack, SubtitleTrack};
use orthanc_core::auth::{AuthState, with_refresh};
use orthanc_core::formatters::format_time;
use orthanc_core::player_logic::{audio_label, mobile_capabilities, subtitle_label};
use serde_json::Value as Json;

const VIDEO_ID: &str = "orthanc-mobile-player";
const PROGRESS_SAVE_INTERVAL_S: f64 = 30.0;
const SKIP_SECONDS: f64 = 10.0;

#[component]
pub fn Player(id: i64) -> Element {
    let auth = use_context::<Signal<AuthState>>();
    let nav = use_navigator();

    // ── Stream state ──
    let mut stream_url = use_signal(|| None::<String>);
    let mut stream_token = use_signal(String::new);
    let mut transcode_session_id = use_signal(|| None::<String>);
    let mut title = use_signal(String::new);
    let mut duration = use_signal(|| 0.0_f64);
    let mut current_time = use_signal(|| 0.0_f64);
    let mut is_playing = use_signal(|| false);
    let mut is_buffering = use_signal(|| true);
    let mut error_msg = use_signal(|| None::<String>);
    let mut last_save_time = use_signal(|| 0.0_f64);
    let mut resume_time = use_signal(|| 0.0_f64);

    // ── Tracks ──
    let mut subtitles = use_signal(Vec::<SubtitleTrack>::new);
    let mut selected_subtitle_id = use_signal(|| None::<i64>);
    let mut burned_subtitle_id = use_signal(|| None::<i64>);
    let mut audio_tracks = use_signal(Vec::<AudioTrack>::new);
    let mut selected_audio_id = use_signal(|| None::<i64>);

    // ── UI state ──
    let mut controls_visible = use_signal(|| true);
    let mut subtitle_menu_open = use_signal(|| false);
    let mut audio_menu_open = use_signal(|| false);

    // ── Initial load: progress + stream-token ──
    use_effect(move || {
        spawn(async move {
            let r = with_refresh(
                auth,
                |token| async move { api::get_progress(&token, id).await },
            )
            .await;
            let resume = match r {
                Ok(p) if !p.is_completed && p.position_seconds > 0 => p.position_seconds as f64,
                _ => 0.0,
            };
            resume_time.set(resume);

            let caps = mobile_capabilities();
            let token_result = with_refresh(auth, move |token| {
                let vc = caps.video.clone();
                let ac = caps.audio.clone();
                let ct = caps.containers.clone();
                async move {
                    api::get_stream_token(&token, id, vc, ac, ct, resume, None, None, false).await
                }
            })
            .await;

            match token_result {
                Ok(resp) => {
                    let full_url = format!("{}{}", api::base_url(), resp.stream_url);
                    stream_url.set(Some(full_url));
                    stream_token.set(resp.token.clone());
                    transcode_session_id.set(resp.transcode_session_id.clone());
                    title.set(resp.title.clone());
                    if let Some(d) = resp.duration_seconds
                        && d > 0
                    {
                        duration.set(d as f64);
                    }
                    subtitles.set(resp.subtitles.clone());
                    burned_subtitle_id.set(resp.burned_subtitle_id);
                    let auto_subtitle = resp.selected_subtitle_id.or_else(|| {
                        if resp.burned_subtitle_id.is_some() {
                            None
                        } else {
                            resp.subtitles
                                .iter()
                                .find(|s| s.is_default && s.delivery == "vtt")
                                .map(|s| s.id)
                        }
                    });
                    selected_subtitle_id.set(auto_subtitle);
                    audio_tracks.set(resp.audio_tracks.clone());
                    selected_audio_id.set(resp.selected_audio_stream_id);
                }
                Err(e) => error_msg.set(Some(e)),
            }
        });
    });

    // ── State-push bridge ──
    //
    // Long-running JS that subscribes to the video element's events and pushes
    // {currentTime, duration, isPlaying, isBuffering} up to Rust via
    // dioxus.send. Polls every 500ms as a fallback in case events miss
    // (some Android WebViews swallow ontimeupdate when backgrounded).
    use_future(move || async move {
        let js = format!(
            r#"
            (async function() {{
                const VIDEO_ID = '{VIDEO_ID}';
                let video = null;
                // Wait up to 5s for the video element to appear in the DOM.
                for (let i = 0; i < 50 && !video; i++) {{
                    video = document.getElementById(VIDEO_ID);
                    if (!video) await new Promise(r => setTimeout(r, 100));
                }}
                if (!video) return;

                const push = () => dioxus.send({{
                    t: video.currentTime || 0,
                    d: video.duration || 0,
                    p: !video.paused,
                    b: video.readyState < 3,
                }});

                push();
                video.addEventListener('timeupdate', push);
                video.addEventListener('play', push);
                video.addEventListener('pause', push);
                video.addEventListener('loadedmetadata', push);
                video.addEventListener('waiting', push);
                video.addEventListener('canplay', push);
                video.addEventListener('seeked', push);
                setInterval(push, 500);
            }})();
            "#
        );
        let mut eval = document::eval(&js);
        loop {
            match eval.recv::<Json>().await {
                Ok(state) => {
                    if let Some(t) = state.get("t").and_then(Json::as_f64) {
                        current_time.set(t);
                        if t - last_save_time() >= PROGRESS_SAVE_INTERVAL_S {
                            last_save_time.set(t);
                            let pos = t as i32;
                            if pos > 0 {
                                spawn(async move {
                                    let _ = with_refresh(auth, move |token| async move {
                                        api::update_progress(&token, id, pos).await
                                    })
                                    .await;
                                });
                            }
                        }
                    }
                    if let Some(d) = state.get("d").and_then(Json::as_f64)
                        && d > 0.0
                    {
                        duration.set(d);
                    }
                    if let Some(p) = state.get("p").and_then(Json::as_bool) {
                        is_playing.set(p);
                    }
                    if let Some(b) = state.get("b").and_then(Json::as_bool) {
                        is_buffering.set(b);
                    }
                }
                Err(_) => break, // channel closed (component unmounted)
            }
        }
    });

    // ── Resume seek once metadata loads ──
    //
    // Fired in a separate eval so it runs unconditionally even if the bridge
    // above hasn't subscribed yet. Idempotent: only fires once because of the
    // `{ once: true }` listener option.
    use_effect(move || {
        let resume = resume_time();
        if resume <= 0.0 {
            return;
        }
        let js = format!(
            r#"(function() {{
                var v = document.getElementById('{VIDEO_ID}');
                if (!v) return;
                var seek = function() {{ v.currentTime = {resume}; }};
                if (v.readyState >= 1) seek();
                else v.addEventListener('loadedmetadata', seek, {{ once: true }});
            }})();"#
        );
        let _ = document::eval(&js);
    });

    // Cleanup on unmount: stop the transcode session if we started one.
    use_drop(move || {
        if let Some(sid) = transcode_session_id.peek().clone() {
            let token = stream_token.peek().clone();
            spawn(async move {
                let _ = api::stop_transcode(&token, &sid).await;
            });
        }
        let pos = *current_time.peek() as i32;
        if pos > 0 {
            spawn(async move {
                let _ = with_refresh(auth, move |token| async move {
                    api::update_progress(&token, id, pos).await
                })
                .await;
            });
        }
    });

    // Build subtitle URL for the active VTT selection. Burn-in tracks bake
    // into the video stream itself and don't need a <track> element.
    let active_subtitle_url = selected_subtitle_id().and_then(|sid| {
        let subs = subtitles.read();
        let track = subs.iter().find(|s| s.id == sid)?;
        if track.delivery != "vtt" {
            return None;
        }
        Some(api::subtitle_url(sid, &stream_token(), 0.0))
    });

    // ── JS command helpers (fire-and-forget) ──
    let send_play_pause = move || {
        let js = format!(
            r#"(function() {{
                var v = document.getElementById('{VIDEO_ID}');
                if (!v) return;
                if (v.paused) v.play(); else v.pause();
            }})();"#
        );
        let _ = document::eval(&js);
    };

    let send_seek_relative = move |delta: f64| {
        let js = format!(
            r#"(function() {{
                var v = document.getElementById('{VIDEO_ID}');
                if (!v) return;
                var t = Math.max(0, Math.min((v.duration || 0), v.currentTime + {delta}));
                v.currentTime = t;
            }})();"#
        );
        let _ = document::eval(&js);
    };

    let send_seek_absolute = move |target: f64| {
        let js = format!(
            r#"(function() {{
                var v = document.getElementById('{VIDEO_ID}');
                if (!v) return;
                v.currentTime = {target};
            }})();"#
        );
        let _ = document::eval(&js);
    };

    // Switch subtitle: cycle through available VTT tracks. Burn-in tracks are
    // intentionally excluded from the cycle — switching to one would require
    // restarting the transcode with a new burn target. The menu shows them
    // disabled or omitted for v1.
    let cycle_subtitle = move || {
        let subs = subtitles.read().clone();
        let next = api::cycle_subtitle_selection(selected_subtitle_id(), &subs);
        selected_subtitle_id.set(next);
    };

    // Switch audio: re-fetch stream-token with new audio_stream_id, swap
    // video.src, restore playback position. Closes the menu.
    let switch_audio = move |new_id: i64| {
        if Some(new_id) == selected_audio_id() {
            audio_menu_open.set(false);
            return;
        }
        let current_pos = current_time();
        let burn_id = burned_subtitle_id();
        let caps = mobile_capabilities();

        spawn(async move {
            let r = with_refresh(auth, move |token| {
                let vc = caps.video.clone();
                let ac = caps.audio.clone();
                let ct = caps.containers.clone();
                async move {
                    api::get_stream_token(
                        &token,
                        id,
                        vc,
                        ac,
                        ct,
                        current_pos,
                        burn_id,
                        Some(new_id),
                        false,
                    )
                    .await
                }
            })
            .await;

            if let Ok(resp) = r {
                stream_url.set(Some(format!("{}{}", api::base_url(), resp.stream_url)));
                stream_token.set(resp.token.clone());
                transcode_session_id.set(resp.transcode_session_id.clone());
                selected_audio_id.set(resp.selected_audio_stream_id);
                let target = current_pos;
                let js = format!(
                    r#"(function() {{
                        var v = document.getElementById('{VIDEO_ID}');
                        if (!v) return;
                        v.addEventListener('loadedmetadata', function() {{ v.currentTime = {target}; v.play(); }}, {{ once: true }});
                    }})();"#
                );
                let _ = document::eval(&js);
            }
        });
        audio_menu_open.set(false);
    };

    rsx! {
        div {
            class: "fullscreen-player",
            onclick: move |_| {
                controls_visible.set(!controls_visible());
                subtitle_menu_open.set(false);
                audio_menu_open.set(false);
            },

            video {
                id: "{VIDEO_ID}",
                class: "player-video",
                src: stream_url().unwrap_or_default(),
                playsinline: true,
                "webkit-playsinline": "true",
                preload: "metadata",
                autoplay: true,
                onerror: move |_| {
                    error_msg.set(Some("Playback error".into()));
                    is_buffering.set(false);
                },
                if let Some(url) = active_subtitle_url.as_ref() {
                    track {
                        kind: "subtitles",
                        src: "{url}",
                        default: true,
                        srclang: subtitles.read()
                            .iter()
                            .find(|s| Some(s.id) == selected_subtitle_id())
                            .and_then(|s| s.language.clone())
                            .unwrap_or_default(),
                    }
                }
            }

            if is_buffering() {
                div { class: "player-spinner",
                    div { class: "spinner-circle" }
                }
            }

            if let Some(err) = error_msg() {
                div { class: "player-error",
                    div { class: "error-msg", "{err}" }
                    button {
                        class: "btn btn-secondary",
                        onclick: move |_| { nav.go_back(); },
                        "Back"
                    }
                }
            }

            div {
                class: if controls_visible() {
                    "player-controls visible"
                } else {
                    "player-controls"
                },
                onclick: move |e: Event<MouseData>| { e.stop_propagation(); },

                // Top bar: back, title, audio/sub toggles
                div { class: "player-top",
                    button {
                        class: "icon-btn",
                        onclick: move |_| { nav.go_back(); },
                        "←"
                    }
                    div { class: "player-title", "{title}" }
                    div { class: "player-top-actions",
                        if !audio_tracks.read().is_empty() {
                            button {
                                class: "icon-btn",
                                onclick: move |_| {
                                    let was = audio_menu_open();
                                    audio_menu_open.set(!was);
                                    subtitle_menu_open.set(false);
                                },
                                "🎵"
                            }
                        }
                        if !subtitles.read().is_empty() {
                            button {
                                class: "icon-btn",
                                onclick: move |_| {
                                    let was = subtitle_menu_open();
                                    subtitle_menu_open.set(!was);
                                    audio_menu_open.set(false);
                                },
                                "💬"
                            }
                        }
                    }
                }

                // Centre: skip back / play-pause / skip forward
                div { class: "player-centre",
                    button {
                        class: "skip-btn",
                        onclick: move |_| (send_seek_relative.clone())(-SKIP_SECONDS),
                        "⏪ 10"
                    }
                    button {
                        class: "play-btn",
                        onclick: move |_| (send_play_pause.clone())(),
                        if is_playing() { "⏸" } else { "▶" }
                    }
                    button {
                        class: "skip-btn",
                        onclick: move |_| (send_seek_relative.clone())(SKIP_SECONDS),
                        "10 ⏩"
                    }
                }

                // Bottom: time labels + scrub bar
                div { class: "player-bottom",
                    div { class: "time-labels",
                        span { "{format_time(current_time())}" }
                        span { class: "time-divider", "/" }
                        span { "{format_time(duration())}" }
                    }
                    input {
                        class: "progress-slider",
                        r#type: "range",
                        min: "0",
                        max: "{duration()}",
                        step: "0.5",
                        value: "{current_time()}",
                        oninput: move |e: Event<FormData>| {
                            if let Ok(t) = e.value().parse::<f64>() {
                                current_time.set(t);
                                (send_seek_absolute.clone())(t);
                            }
                        },
                    }
                }

                if audio_menu_open() {
                    div { class: "track-menu",
                        h3 { class: "track-menu-title", "Audio" }
                        for t in audio_tracks.read().iter() {
                            {
                                let track_id = t.id;
                                rsx! {
                                    button {
                                        key: "{t.id}",
                                        class: if Some(t.id) == selected_audio_id() {
                                            "track-row track-row-active"
                                        } else {
                                            "track-row"
                                        },
                                        onclick: move |_| (switch_audio.clone())(track_id),
                                        "{audio_label(t)}"
                                    }
                                }
                            }
                        }
                    }
                }

                if subtitle_menu_open() {
                    div { class: "track-menu",
                        h3 { class: "track-menu-title", "Subtitles" }
                        button {
                            class: if selected_subtitle_id().is_none() {
                                "track-row track-row-active"
                            } else {
                                "track-row"
                            },
                            onclick: move |_| {
                                selected_subtitle_id.set(None);
                                subtitle_menu_open.set(false);
                            },
                            "Off"
                        }
                        for t in subtitles.read().iter() {
                            {
                                let track_id = t.id;
                                let is_vtt = t.delivery == "vtt";
                                rsx! {
                                    button {
                                        key: "{t.id}",
                                        class: if Some(t.id) == selected_subtitle_id() {
                                            "track-row track-row-active"
                                        } else if !is_vtt {
                                            "track-row track-row-disabled"
                                        } else {
                                            "track-row"
                                        },
                                        disabled: !is_vtt,
                                        onclick: move |_| {
                                            if is_vtt {
                                                selected_subtitle_id.set(Some(track_id));
                                                subtitle_menu_open.set(false);
                                            }
                                        },
                                        "{subtitle_label(t)}"
                                        if !is_vtt {
                                            span { class: "track-row-note",
                                                " (image-based — needs server burn-in)"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // Suppress unused warning for cycle helper — keeping the
                        // utility around for future double-tap-to-cycle gesture.
                        { let _ = cycle_subtitle; rsx!{} }
                    }
                }
            }
        }
    }
}
