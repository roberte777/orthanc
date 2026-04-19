use dioxus::prelude::*;
use web_sys::wasm_bindgen::JsCast;

use crate::api;
use crate::state::{with_refresh, AuthState};

fn get_video() -> Option<web_sys::HtmlVideoElement> {
    let window = web_sys::window()?;
    let document = window.document()?;
    let el = document.get_element_by_id("orthanc-player")?;
    el.dyn_into::<web_sys::HtmlVideoElement>().ok()
}

/// Probe which codecs/containers the browser supports via MediaSource.isTypeSupported().
fn probe_client_capabilities() -> (Vec<String>, Vec<String>, Vec<String>) {
    let js = r#"(function() {
        var ms = window.MediaSource || window.WebKitMediaSource;
        if (!ms) return JSON.stringify({v:[],a:[],c:[]});
        var video = [];
        var audio = [];
        var containers = [];
        var videoTests = {
            'h264': 'video/mp4; codecs="avc1.640029"',
            'hevc': 'video/mp4; codecs="hvc1.1.6.L93.B0"',
            'vp9':  'video/webm; codecs="vp9"',
            'av1':  'video/mp4; codecs="av01.0.08M.08"',
        };
        var audioTests = {
            'aac':    'audio/mp4; codecs="mp4a.40.2"',
            'opus':   'audio/webm; codecs="opus"',
            'mp3':    'audio/mpeg',
            'flac':   'audio/flac',
            'vorbis': 'audio/webm; codecs="vorbis"',
            'ac3':    'audio/mp4; codecs="ac-3"',
            'eac3':   'audio/mp4; codecs="ec-3"',
        };
        var containerTests = {
            'mp4':  'video/mp4; codecs="avc1.640029,mp4a.40.2"',
            'webm': 'video/webm; codecs="vp9,opus"',
            'mov':  'video/mp4; codecs="avc1.640029,mp4a.40.2"',
            'm4v':  'video/mp4; codecs="avc1.640029,mp4a.40.2"',
        };
        for (var k in videoTests) {
            try { if (ms.isTypeSupported(videoTests[k])) video.push(k); } catch(e) {}
        }
        for (var k in audioTests) {
            try { if (ms.isTypeSupported(audioTests[k])) audio.push(k); } catch(e) {}
        }
        for (var k in containerTests) {
            try { if (ms.isTypeSupported(containerTests[k])) containers.push(k); } catch(e) {}
        }
        return JSON.stringify({v:video,a:audio,c:containers});
    })()"#;

    let result = js_sys::eval(js);
    if let Ok(val) = result {
        if let Some(s) = val.as_string() {
            #[derive(serde::Deserialize)]
            struct Caps {
                v: Vec<String>,
                a: Vec<String>,
                c: Vec<String>,
            }
            if let Ok(caps) = serde_json::from_str::<Caps>(&s) {
                return (caps.v, caps.a, caps.c);
            }
        }
    }
    // Fallback: empty = server uses defaults
    (vec![], vec![], vec![])
}

/// Destroy the hls.js instance.
fn destroy_hls() {
    let _ = js_sys::eval(
        "if (window._orthanc_hls) { window._orthanc_hls.destroy(); window._orthanc_hls = null; }",
    );
}

#[component]
pub fn Player(id: i64) -> Element {
    let auth = use_context::<Signal<AuthState>>();
    let mut stream_url = use_signal(|| None::<String>);
    let mut stream_mode = use_signal(|| String::new());
    let mut error_msg = use_signal(|| None::<String>);
    let mut is_playing = use_signal(|| false);
    let mut current_time = use_signal(|| 0.0_f64);
    let mut duration = use_signal(|| 0.0_f64);
    let mut show_controls = use_signal(|| true);
    let mut volume = use_signal(|| 1.0_f64);
    let mut title = use_signal(|| String::new());
    let mut last_save_time = use_signal(|| 0.0_f64);
    let mut transcode_session_id = use_signal(|| None::<String>);
    let mut stream_token = use_signal(|| String::new());
    // True while a seek restart is in progress (suppress ontimeupdate)
    let mut seeking = use_signal(|| false);
    // True when the video element is waiting for data (buffering)
    let mut is_buffering = use_signal(|| false);
    // Offset added to video.currentTime to get real media position (for HLS mode)
    let mut hls_time_offset = use_signal(|| 0.0_f64);
    // Timestamp of last completed seek (for debouncing spurious onchange)
    let mut last_seek_time = use_signal(|| 0.0_f64);
    let nav = use_navigator();

    // Fetch progress first, then stream token (so we can pass resume position as start_time)
    use_effect(move || {
        spawn(async move {
            // Get saved progress for resume
            let mut resume_time = 0.0_f64;
            let progress_result = with_refresh(auth, |token| async move {
                api::get_progress(&token, id).await
            })
            .await;

            if let Ok(progress) = progress_result {
                if !progress.is_completed && progress.position_seconds > 0 {
                    resume_time = progress.position_seconds as f64;
                }
            }

            // Probe client codec support
            let (video_codecs, audio_codecs, containers) = probe_client_capabilities();

            // Get stream token (pass resume position as start_time for HLS mode)
            let token_result = with_refresh(auth, {
                let vc = video_codecs.clone();
                let ac = audio_codecs.clone();
                let ct = containers.clone();
                move |token| {
                    let vc = vc.clone();
                    let ac = ac.clone();
                    let ct = ct.clone();
                    async move {
                        api::get_stream_token(&token, id, vc, ac, ct, resume_time).await
                    }
                }
            })
            .await;

            match token_result {
                Ok(resp) => {
                    let url = format!("{}{}", api::API_BASE_URL, resp.stream_url);
                    let mode = resp.mode.clone();
                    stream_mode.set(mode.clone());
                    stream_url.set(Some(url.clone()));
                    title.set(resp.title.clone());
                    transcode_session_id.set(resp.transcode_session_id.clone());
                    stream_token.set(resp.token.clone());
                    if let Some(dur) = resp.duration_seconds {
                        if dur > 0 {
                            duration.set(dur as f64);
                        }
                    }

                    // For HLS modes, set the time offset and attach hls.js
                    if mode != "direct" {
                        hls_time_offset.set(resume_time);
                        last_seek_time.set(resume_time);
                        let hls_url = url.clone();
                        let token = resp.token.clone();
                        let js = format!(
                            "setTimeout(function() {{ \
                                var video = document.getElementById('orthanc-player'); \
                                if (!video) return; \
                                if (typeof Hls !== 'undefined' && Hls.isSupported()) {{ \
                                    if (window._orthanc_hls) {{ window._orthanc_hls.destroy(); }} \
                                    var hls = new Hls({{ \
                                        startPosition: 0, \
                                        xhrSetup: function(xhr, url) {{ \
                                            if (url.indexOf('token=') === -1) {{ \
                                                var sep = url.indexOf('?') === -1 ? '?' : '&'; \
                                                url = url + sep + 'token={}'; \
                                            }} \
                                            xhr.open('GET', url, true); \
                                        }} \
                                    }}); \
                                    hls.loadSource('{}'); \
                                    hls.attachMedia(video); \
                                    video.dataset.hlsUrl = '{}'; \
                                    window._orthanc_hls = hls; \
                                }} else if (video.canPlayType('application/vnd.apple.mpegurl')) {{ \
                                    video.src = '{}'; \
                                }} \
                            }}, 100);",
                            token, hls_url, hls_url, hls_url
                        );
                        let _ = js_sys::eval(&js);
                    } else if resume_time > 0.0 {
                        // For direct mode, set currentTime after metadata loads
                        // (handled in onloadedmetadata via hls_time_offset = 0 and resume_time)
                        // We store resume_time so onloadedmetadata can use it
                        current_time.set(resume_time);
                    }
                }
                Err(e) => {
                    error_msg.set(Some(format!("Failed to get stream: {}", e)));
                }
            }
        });
    });

    let toggle_play = move |_| {
        if let Some(video) = get_video() {
            if video.paused() {
                let _ = video.play();
            } else {
                video.pause().ok();
                // Save progress on pause (apply offset for HLS mode)
                let t = video.current_time() + hls_time_offset();
                spawn(async move {
                    let _ = with_refresh(auth, |token| async move {
                        api::update_progress(&token, id, t as i32).await
                    })
                    .await;
                });
            }
        }
    };

    let skip_back = move |_| {
        if seeking() { return; }
        let new_time = (current_time() - 10.0).max(0.0);
        current_time.set(new_time);
        if transcode_session_id().is_none() {
            if let Some(video) = get_video() {
                video.set_current_time(new_time);
            }
        }
    };

    let skip_forward = move |_| {
        if seeking() { return; }
        let new_time = (current_time() + 10.0).min(duration());
        current_time.set(new_time);
        if transcode_session_id().is_none() {
            if let Some(video) = get_video() {
                video.set_current_time(new_time);
            }
        }
    };

    let toggle_mute = move |_| {
        if let Some(video) = get_video() {
            if video.volume() > 0.0 {
                video.set_volume(0.0);
                volume.set(0.0);
            } else {
                video.set_volume(1.0);
                volume.set(1.0);
            }
        }
    };

    // Update visual position while dragging (no server call)
    let on_seek_input = move |evt: Event<FormData>| {
        if seeking() { return; }
        if let Ok(t) = evt.value().parse::<f64>() {
            current_time.set(t);
            // For direct mode, seek immediately
            if transcode_session_id().is_none() {
                if let Some(video) = get_video() {
                    video.set_current_time(t);
                }
            }
        }
    };

    // Commit seek on slider release (onchange fires once on mouseup)
    let on_seek_commit = move |evt: Event<FormData>| {
        // Guard: ignore onchange while a seek is already in flight
        if seeking() { return; }

        if let Ok(t) = evt.value().parse::<f64>() {
            // Debounce: skip if the target is within 10s of the last seek
            // (catches spurious onchange from programmatic slider value updates)
            if transcode_session_id().is_some() && (t - last_seek_time()).abs() < 10.0 {
                return;
            }
            current_time.set(t);

            if let Some(ref session_id) = transcode_session_id() {
                // Remember play state before pausing for the seek
                let was_playing = is_playing();
                if let Some(video) = get_video() {
                    video.pause().ok();
                }
                is_playing.set(false);
                seeking.set(true);

                let session_id = session_id.clone();
                let tk = stream_token();
                spawn(async move {
                    // Server waits until FFmpeg produces first segment before responding
                    let seek_result = with_refresh(auth, {
                        let sid = session_id.clone();
                        move |token| {
                            let sid = sid.clone();
                            async move {
                                api::transcode_seek(&token, &sid, t).await
                            }
                        }
                    })
                    .await;

                    match seek_result {
                        Ok(resp) if resp.ready => {
                            // Update time offset: video timestamps are zero-based,
                            // add this offset to get real media position
                            hls_time_offset.set(resp.seek_time);

                            // Cache-bust: append unique query param so hls.js
                            // fetches fresh manifest and segments after seek
                            let cache_bust = js_sys::Date::now() as u64;

                            // Reload hls.js immediately -- segments are already ready
                            let auto_play = if was_playing { "true" } else { "false" };
                            let js = format!(
                                "(function() {{ \
                                    if (window._orthanc_hls) {{ window._orthanc_hls.destroy(); }} \
                                    var video = document.getElementById('orthanc-player'); \
                                    if (!video) return; \
                                    window._orthanc_seek_done = false; \
                                    var cb = '{}'; \
                                    var autoPlay = {}; \
                                    var hls = new Hls({{ \
                                        startPosition: 0, \
                                        xhrSetup: function(xhr, url) {{ \
                                            var sep = url.indexOf('?') === -1 ? '?' : '&'; \
                                            if (url.indexOf('token=') === -1) {{ \
                                                url = url + sep + 'token={}' + '&_cb=' + cb; \
                                            }} else {{ \
                                                url = url + '&_cb=' + cb; \
                                            }} \
                                            xhr.open('GET', url, true); \
                                        }} \
                                    }}); \
                                    var hlsUrl = video.dataset.hlsUrl || video.src; \
                                    var urlSep = hlsUrl.indexOf('?') === -1 ? '?' : '&'; \
                                    hls.loadSource(hlsUrl + urlSep + '_cb=' + cb); \
                                    hls.attachMedia(video); \
                                    hls.on(Hls.Events.MANIFEST_PARSED, function() {{ \
                                        if (autoPlay) {{ video.play(); }} \
                                        window._orthanc_seek_done = true; \
                                    }}); \
                                    window._orthanc_hls = hls; \
                                }})()",
                                cache_bust, auto_play, tk
                            );
                            let _ = js_sys::eval(&js);

                            // Poll until hls.js signals it's ready (MANIFEST_PARSED fired),
                            // with a 10-second timeout to avoid hanging forever
                            let poll_start = js_sys::Date::now();
                            loop {
                                let done = js_sys::eval("window._orthanc_seek_done === true")
                                    .ok()
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                if done { break; }
                                if js_sys::Date::now() - poll_start > 10_000.0 { break; }
                                let promise = js_sys::Promise::new(&mut |resolve: js_sys::Function, _: js_sys::Function| {
                                    let _ = web_sys::window().unwrap()
                                        .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 100);
                                });
                                let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
                            }

                            last_seek_time.set(t);
                        }
                        _ => {
                            // Seek timed out or failed
                        }
                    }

                    // Clear seeking flag now that hls.js has parsed the manifest
                    seeking.set(false);
                });
            } else {
                if let Some(video) = get_video() {
                    video.set_current_time(t);
                }
            }
        }
    };

    let on_volume = move |evt: Event<FormData>| {
        if let Some(video) = get_video() {
            if let Ok(v) = evt.value().parse::<f64>() {
                video.set_volume(v);
                volume.set(v);
            }
        }
    };

    let on_mouse_move = move |_: MouseEvent| {
        show_controls.set(true);
    };

    let toggle_fullscreen = move |_| {
        if let Some(window) = web_sys::window() {
            if let Some(document) = window.document() {
                // If already fullscreen, exit; otherwise enter
                if document.fullscreen_element().is_some() {
                    document.exit_fullscreen();
                } else if let Some(el) = document.get_element_by_id("player-container") {
                    let _ = el.request_fullscreen();
                }
            }
        }
    };

    let go_back = move |_| {
        // Save progress before leaving
        let t = current_time();
        if t > 0.0 {
            spawn(async move {
                let _ = with_refresh(auth, |token| async move {
                    api::update_progress(&token, id, t as i32).await
                })
                .await;
            });
        }
        destroy_hls();
        nav.go_back();
    };

    let format_time = |seconds: f64| -> String {
        let total = seconds as u64;
        let h = total / 3600;
        let m = (total % 3600) / 60;
        let s = total % 60;
        if h > 0 {
            format!("{}:{:02}:{:02}", h, m, s)
        } else {
            format!("{}:{:02}", m, s)
        }
    };

    let cur = format_time(current_time());
    let dur = format_time(duration());
    let is_direct = stream_mode() == "direct" || stream_mode().is_empty();
    let progress_pct = if duration() > 0.0 {
        (current_time() / duration()) * 100.0
    } else {
        0.0
    };

    // Volume icon state
    let vol = volume();
    let vol_icon = if vol == 0.0 {
        // Muted icon
        r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M11 5L6 9H2v6h4l5 4V5z"/><line x1="23" y1="9" x2="17" y2="15"/><line x1="17" y1="9" x2="23" y2="15"/></svg>"#
    } else if vol < 0.5 {
        // Low volume
        r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M11 5L6 9H2v6h4l5 4V5z"/><path d="M15.54 8.46a5 5 0 010 7.07"/></svg>"#
    } else {
        // High volume
        r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M11 5L6 9H2v6h4l5 4V5z"/><path d="M15.54 8.46a5 5 0 010 7.07"/><path d="M19.07 4.93a10 10 0 010 14.14"/></svg>"#
    };

    rsx! {
        div {
            id: "player-container",
            class: "player-container",
            onmousemove: on_mouse_move,
            onclick: toggle_play,

            if let Some(ref err) = error_msg() {
                div { class: "player-error",
                    p { "{err}" }
                    button { onclick: go_back, "Go Back" }
                }
            } else if let Some(ref url) = stream_url() {
                video {
                    id: "orthanc-player",
                    class: "player-video",
                    src: if is_direct { url.as_str() } else { "about:blank" },
                    preload: "metadata",
                    onplay: move |_| is_playing.set(true),
                    onpause: move |_| is_playing.set(false),
                    onwaiting: move |_| is_buffering.set(true),
                    onplaying: move |_| {
                        is_buffering.set(false);
                        is_playing.set(true);
                    },
                    oncanplay: move |_| is_buffering.set(false),
                    ontimeupdate: move |_| {
                        if seeking() { return; }
                        if let Some(video) = get_video() {
                            let t = video.current_time() + hls_time_offset();
                            current_time.set(t);
                            let last = last_save_time();
                            if (t - last).abs() >= 10.0 {
                                last_save_time.set(t);
                                spawn(async move {
                                    let _ = with_refresh(auth, |token| async move {
                                        api::update_progress(&token, id, t as i32).await
                                    })
                                    .await;
                                });
                            }
                        }
                    },
                    onloadedmetadata: move |_| {
                        if let Some(video) = get_video() {
                            let d = video.duration();
                            if d.is_finite() && d > 0.0 && duration() < 1.0
                                && transcode_session_id().is_none()
                            {
                                duration.set(d);
                            }
                            if transcode_session_id().is_none() {
                                let cur = current_time();
                                if cur > 0.0 {
                                    video.set_current_time(cur);
                                }
                            }
                        }
                    },
                    onerror: move |_| {
                        if stream_mode() == "direct" {
                            error_msg.set(Some("Failed to load video. The file format may not be supported by your browser.".to_string()));
                        }
                    },
                }

                // Transcode mode indicator
                if !is_direct {
                    div { class: "player-transcode-badge", "{stream_mode}" }
                }

                // Center spinner (only when buffering/seeking)
                if seeking() || is_buffering() {
                    div { class: "player-center-overlay",
                        div { class: "player-spinner" }
                    }
                }

                // Controls overlay
                div {
                    class: if show_controls() { "player-controls" } else { "player-controls hidden" },
                    onclick: move |e| e.stop_propagation(),

                    // Top bar - just back arrow
                    div { class: "player-top-bar",
                        button {
                            class: "player-icon-btn player-back-btn",
                            onclick: go_back,
                            dangerous_inner_html: r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M19 12H5"/><path d="M12 19l-7-7 7-7"/></svg>"#,
                        }
                    }

                    // Spacer to push bottom bar down
                    div { class: "player-spacer" }

                    // Bottom section: progress bar + controls
                    div { class: "player-bottom-section",

                        // Progress bar
                        div { class: "player-progress-wrap",
                            div { class: "player-progress-bar",
                                div {
                                    class: "player-progress-fill",
                                    style: "width: {progress_pct}%",
                                }
                                div {
                                    class: "player-progress-thumb",
                                    style: "left: {progress_pct}%",
                                }
                            }
                            input {
                                r#type: "range",
                                class: "player-progress-input",
                                min: "0",
                                max: "{duration}",
                                step: "0.1",
                                value: "{current_time}",
                                oninput: on_seek_input,
                                onchange: on_seek_commit,
                            }
                        }

                        // Control buttons row
                        div { class: "player-bottom-bar",
                            // Left controls
                            div { class: "player-controls-left",
                                // Play/Pause
                                button {
                                    class: "player-icon-btn",
                                    onclick: toggle_play,
                                    dangerous_inner_html: if is_playing() {
                                        r#"<svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4" width="4" height="16" rx="1"/><rect x="14" y="4" width="4" height="16" rx="1"/></svg>"#
                                    } else {
                                        r#"<svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>"#
                                    },
                                }
                                // Skip back 10s
                                button {
                                    class: "player-icon-btn player-skip-btn",
                                    onclick: skip_back,
                                    dangerous_inner_html: r#"<svg viewBox="0 0 24 24" fill="currentColor"><path d="M12.5 3a9 9 0 110 18 9 9 0 010-18m0 2a7 7 0 100 14 7 7 0 000-14" opacity="0"/><path d="M11.5 3C6.25 3 2 7.25 2 12.5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/><path d="M5.5 1.5L2 3.5 2 0" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/><text x="12" y="16.5" font-size="7.5" font-weight="700" text-anchor="middle" font-family="Arial,sans-serif" fill="currentColor">10</text><path d="M12.5 3a9.5 9.5 0 110 19 9.5 9.5 0 010-19" fill="none" stroke="currentColor" stroke-width="2" stroke-dasharray="0 15 999"/></svg>"#,
                                }
                                // Skip forward 10s
                                button {
                                    class: "player-icon-btn player-skip-btn",
                                    onclick: skip_forward,
                                    dangerous_inner_html: r#"<svg viewBox="0 0 24 24" fill="currentColor"><path d="M12.5 3a9 9 0 110 18 9 9 0 010-18m0 2a7 7 0 100 14 7 7 0 000-14" opacity="0"/><path d="M12.5 3C17.75 3 22 7.25 22 12.5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/><path d="M18.5 1.5L22 3.5 22 0" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/><text x="12" y="16.5" font-size="7.5" font-weight="700" text-anchor="middle" font-family="Arial,sans-serif" fill="currentColor">10</text><path d="M12.5 3a9.5 9.5 0 100 19 9.5 9.5 0 000-19" fill="none" stroke="currentColor" stroke-width="2" stroke-dasharray="0 15 999"/></svg>"#,
                                }
                                // Volume
                                div { class: "player-volume-group",
                                    button {
                                        class: "player-icon-btn",
                                        onclick: toggle_mute,
                                        dangerous_inner_html: "{vol_icon}",
                                    }
                                    div { class: "player-volume-slider-wrap",
                                        input {
                                            r#type: "range",
                                            class: "player-volume",
                                            min: "0",
                                            max: "1",
                                            step: "0.05",
                                            value: "{volume}",
                                            oninput: on_volume,
                                        }
                                    }
                                }
                                // Time display
                                span { class: "player-time", "{cur} / {dur}" }
                            }

                            // Center title
                            div { class: "player-controls-center",
                                span { class: "player-title", "{title}" }
                            }

                            // Right controls
                            div { class: "player-controls-right",
                                // Fullscreen
                                button {
                                    class: "player-icon-btn",
                                    onclick: toggle_fullscreen,
                                    dangerous_inner_html: r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M8 3H5a2 2 0 00-2 2v3m18 0V5a2 2 0 00-2-2h-3m0 18h3a2 2 0 002-2v-3M3 16v3a2 2 0 002 2h3"/></svg>"#,
                                }
                            }
                        }
                    }
                }
            } else {
                div { class: "player-loading",
                    div { class: "player-spinner" }
                }
            }
        }
    }
}
