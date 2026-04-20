use dioxus::prelude::*;
use web_sys::wasm_bindgen::{self, JsCast};

use crate::api;
use crate::state::{self, with_refresh, AuthState};
use crate::views::settings::{LS_DEFAULT_VOLUME, LS_SKIP_SECONDS};

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

/// Inject the subtitle attach/detach bridge. Must be called once per player
/// mount so the helper exists on `window`.
fn install_subtitle_bridge() {
    let _ = js_sys::eval(r#"
        window._orthancAttachSubtitle = function(url, lang, label, isDefault) {
            var attempts = 0;
            var apply = function() {
                var video = document.getElementById('orthanc-player');
                if (!video) {
                    if (attempts++ < 20) { setTimeout(apply, 100); return; }
                    return;
                }
                var existing = video.querySelectorAll('track[data-orthanc-sub="1"]');
                for (var i = 0; i < existing.length; i++) { existing[i].remove(); }
                for (var i = 0; i < video.textTracks.length; i++) {
                    video.textTracks[i].mode = 'disabled';
                }
                if (!url) return;
                var tr = document.createElement('track');
                tr.kind = 'subtitles';
                tr.setAttribute('crossorigin', 'anonymous');
                tr.src = url;
                if (lang) tr.srclang = lang;
                if (label) tr.label = label;
                if (isDefault) tr.default = true;
                tr.setAttribute('data-orthanc-sub', '1');
                tr.addEventListener('error', function(e) {
                    console.error('[orthanc] subtitle track load error for url=' + url + ' err=' + (e && e.message ? e.message : 'unknown'));
                });
                video.appendChild(tr);
                console.debug('[orthanc] subtitle track attached: ' + label + ' (' + lang + ') url=' + url);
                var forceShow = function() {
                    try {
                        if (tr.track) { tr.track.mode = 'showing'; }
                    } catch (_) {}
                    for (var i = 0; i < video.textTracks.length; i++) {
                        var t = video.textTracks[i];
                        // Only show the one we just attached; disable any others.
                        if (t === tr.track || (label && t.label === label) || (lang && t.language === lang)) {
                            t.mode = 'showing';
                        } else {
                            t.mode = 'disabled';
                        }
                    }
                };
                tr.addEventListener('load', function() {
                    console.debug('[orthanc] subtitle track loaded url=' + url);
                    forceShow();
                });
                setTimeout(forceShow, 60);
                setTimeout(forceShow, 250);
                setTimeout(forceShow, 1000);
            };
            apply();
        };
    "#);
}

/// Build the URL+label for a subtitle track, then ask the JS bridge to attach it.
/// `url` of None detaches (removes) any currently-attached subtitle track.
fn js_attach_subtitle(url: Option<&str>, lang: &str, label: &str, is_default: bool) {
    let js = match url {
        Some(u) => {
            let u_esc = u.replace('\\', "\\\\").replace('\'', "\\'");
            let lang_esc = lang.replace('\\', "\\\\").replace('\'', "\\'");
            let label_esc = label.replace('\\', "\\\\").replace('\'', "\\'");
            format!(
                "window._orthancAttachSubtitle('{}', '{}', '{}', {})",
                u_esc, lang_esc, label_esc, is_default
            )
        }
        None => "window._orthancAttachSubtitle(null)".to_string(),
    };
    let _ = js_sys::eval(&js);
}

/// Produce a user-facing label for a subtitle track.
fn subtitle_label(t: &api::SubtitleTrack) -> String {
    let base = t
        .title
        .clone()
        .or_else(|| t.language.clone())
        .unwrap_or_else(|| format!("Subtitle #{}", t.id));
    let mut chips: Vec<&str> = Vec::new();
    if t.is_forced {
        chips.push("Forced");
    }
    if t.is_default {
        chips.push("Default");
    }
    if t.is_external {
        chips.push("External");
    }
    if chips.is_empty() {
        base
    } else {
        format!("{} · {}", base, chips.join(" · "))
    }
}

/// Produce a user-facing label for an audio track.
fn audio_label(t: &api::AudioTrack) -> String {
    let base = t
        .language
        .clone()
        .or_else(|| t.title.clone())
        .unwrap_or_else(|| format!("Audio #{}", t.id));
    let mut chips: Vec<String> = Vec::new();
    if let Some(codec) = &t.codec {
        chips.push(codec.to_uppercase());
    }
    if let Some(ch) = t.channels {
        let pretty = match ch {
            1 => "Mono".to_string(),
            2 => "Stereo".to_string(),
            6 => "5.1".to_string(),
            8 => "7.1".to_string(),
            n => format!("{}ch", n),
        };
        chips.push(pretty);
    }
    if let Some(title) = &t.title {
        if t.language.as_ref().map(|l| l != title).unwrap_or(true) {
            chips.push(title.clone());
        }
    }
    if t.is_default {
        chips.push("Default".to_string());
    }
    if chips.is_empty() {
        base
    } else {
        format!("{} · {}", base, chips.join(" · "))
    }
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
    let mut last_activity = use_signal(|| js_sys::Date::now());
    let initial_volume = state::storage_get(LS_DEFAULT_VOLUME)
        .and_then(|v| v.parse::<f64>().ok())
        .map(|v| v.clamp(0.0, 1.0))
        .unwrap_or(1.0);
    let mut volume = use_signal(|| initial_volume);
    let skip_seconds = state::storage_get(LS_SKIP_SECONDS)
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|&s| s > 0.0)
        .unwrap_or(10.0);
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
    // Subtitle state
    let mut available_subtitles = use_signal(Vec::<api::SubtitleTrack>::new);
    let mut selected_subtitle_id = use_signal(|| None::<i64>);
    let mut burned_subtitle_id = use_signal(|| None::<i64>);
    let mut subtitle_menu_open = use_signal(|| false);
    // PTS offset for HLS subtitle alignment; 0.0 for direct play.
    let mut subtitle_offset = use_signal(|| 0.0_f64);
    // Audio state
    let mut available_audio_tracks = use_signal(Vec::<api::AudioTrack>::new);
    let mut selected_audio_stream_id = use_signal(|| None::<i64>);
    let mut audio_normalize_on = use_signal(|| false);
    let mut audio_menu_open = use_signal(|| false);
    let nav = use_navigator();

    // Install the subtitle JS bridge once per mount.
    use_effect(move || {
        install_subtitle_bridge();
    });

    // Helper: apply the currently-selected subtitle (or detach if None).
    // Reads the latest signals each call so it's correct across seeks.
    let mut apply_selected_subtitle = move || {
        let maybe_id = selected_subtitle_id();
        let subs = available_subtitles();
        let token = stream_token();
        let offset = subtitle_offset();
        match maybe_id.and_then(|id| subs.iter().find(|t| t.id == id).cloned()) {
            Some(track) => {
                let url = api::subtitle_url(track.id, &token, offset);
                let label = subtitle_label(&track);
                let lang = track.language.clone().unwrap_or_default();
                js_attach_subtitle(Some(&url), &lang, &label, track.is_default);
            }
            None => {
                js_attach_subtitle(None, "", "", false);
            }
        }
    };

    // Fire-and-forget: persist the user's current audio/subtitle choice for
    // this show (or movie). Scope resolution happens on the server.
    let save_preferences = move || {
        let audio_lang = selected_audio_stream_id()
            .and_then(|sid| available_audio_tracks().iter().find(|t| t.id == sid).cloned())
            .and_then(|t| t.language);
        let sub_id = selected_subtitle_id().or(burned_subtitle_id());
        let subs_enabled = sub_id.is_some();
        let sub_lang = sub_id
            .and_then(|sid| available_subtitles().iter().find(|t| t.id == sid).cloned())
            .and_then(|t| t.language);
        let normalize = audio_normalize_on();
        spawn(async move {
            let _ = with_refresh(auth, {
                let audio_lang = audio_lang.clone();
                let sub_lang = sub_lang.clone();
                move |token| {
                    let audio_lang = audio_lang.clone();
                    let sub_lang = sub_lang.clone();
                    async move {
                        api::save_track_preferences(
                            &token,
                            id,
                            audio_lang,
                            sub_lang,
                            subs_enabled,
                            normalize,
                        )
                        .await
                    }
                }
            })
            .await;
        });
    };

    // Cleanup transcode session on unmount (e.g. browser navigation, route change)
    use_drop(move || {
        let session = transcode_session_id();
        if let Some(sid) = session {
            destroy_hls();
            // Remove keyboard listener
            let _ = js_sys::eval(
                "if (window._orthanc_keydown) { window.removeEventListener('keydown', window._orthanc_keydown); window._orthanc_keydown = null; }",
            );
            spawn(async move {
                let _ = with_refresh(auth, move |token| {
                    let sid = sid.clone();
                    async move { api::stop_transcode(&token, &sid).await }
                })
                .await;
            });
        }
    });

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
                        api::get_stream_token(
                            &token,
                            id,
                            vc,
                            ac,
                            ct,
                            resume_time,
                            None,
                            None,
                            false,
                        )
                        .await
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

                    // Seed subtitle state
                    available_subtitles.set(resp.subtitles.clone());
                    burned_subtitle_id.set(resp.burned_subtitle_id);
                    let offset = resp
                        .transcode_actual_start_seconds
                        .unwrap_or(if mode == "direct" { 0.0 } else { resume_time });
                    subtitle_offset.set(offset);

                    // Pick the VTT subtitle the server resolved (from saved preference)
                    // or, if none and no burn is active, fall back to the default text track.
                    let auto_select_id = resp.selected_subtitle_id.or_else(|| {
                        if resp.burned_subtitle_id.is_some() {
                            None
                        } else {
                            resp.subtitles
                                .iter()
                                .find(|s| s.is_default && s.delivery == "vtt")
                                .map(|s| s.id)
                        }
                    });
                    selected_subtitle_id.set(auto_select_id);
                    apply_selected_subtitle();

                    // Seed audio state
                    available_audio_tracks.set(resp.audio_tracks.clone());
                    selected_audio_stream_id.set(resp.selected_audio_stream_id);
                    audio_normalize_on.set(resp.audio_normalize);

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
                    let msg = if e.contains("429") {
                        "Too many active transcoding streams. Please stop playback on another device or wait a moment and try again.".to_string()
                    } else {
                        format!("Failed to get stream: {}", e)
                    };
                    error_msg.set(Some(msg));
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

    let toggle_mute = move |_| {
        if let Some(video) = get_video() {
            if video.volume() > 0.0 {
                video.set_volume(0.0);
                volume.set(0.0);
            } else {
                video.set_volume(initial_volume);
                volume.set(initial_volume);
            }
        }
    };

    // Shared seek-to-time logic for both slider and skip buttons
    let mut seek_to = move |t: f64| {
        if seeking() { return; }
        current_time.set(t);

        if let Some(ref session_id) = transcode_session_id() {
            let was_playing = is_playing();
            if let Some(video) = get_video() {
                video.pause().ok();
            }
            is_playing.set(false);
            seeking.set(true);

            let session_id = session_id.clone();
            let tk = stream_token();
            spawn(async move {
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
                        hls_time_offset.set(resp.seek_time);
                        // Align subtitle cues to the new HLS playlist origin.
                        // actual_start_seconds == seek_time (server side), so
                        // this is the correct offset for cue shifting.
                        subtitle_offset.set(resp.actual_start_seconds.max(resp.seek_time));
                        let cache_bust = js_sys::Date::now() as u64;
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
                        // Re-attach the current subtitle with the new offset.
                        apply_selected_subtitle();
                    }
                    _ => {}
                }

                seeking.set(false);
            });
        } else {
            if let Some(video) = get_video() {
                video.set_current_time(t);
            }
        }
    };

    let skip_back = move |_| {
        if let Some(video) = get_video() {
            if transcode_session_id().is_some() {
                let new_time = (current_time() - skip_seconds).max(0.0);
                seek_to(new_time);
            } else {
                let new_time = (video.current_time() - skip_seconds).max(0.0);
                video.set_current_time(new_time);
                current_time.set(new_time);
            }
        }
    };

    let skip_forward = move |_| {
        if let Some(video) = get_video() {
            if transcode_session_id().is_some() {
                let new_time = (current_time() + skip_seconds).min(duration());
                seek_to(new_time);
            } else {
                let dur = video.duration();
                let new_time = (video.current_time() + skip_seconds).min(if dur.is_finite() { dur } else { f64::MAX });
                video.set_current_time(new_time);
                current_time.set(new_time);
            }
        }
    };

    // Update visual position while dragging (no server call)
    let on_seek_input = move |evt: Event<FormData>| {
        if seeking() { return; }
        if let Ok(t) = evt.value().parse::<f64>() {
            current_time.set(t);
            if transcode_session_id().is_none() {
                if let Some(video) = get_video() {
                    video.set_current_time(t);
                }
            }
        }
    };

    // Commit seek on slider release
    let on_seek_commit = move |evt: Event<FormData>| {
        if let Ok(t) = evt.value().parse::<f64>() {
            // Debounce spurious onchange for transcoded streams
            if transcode_session_id().is_some() && (t - last_seek_time()).abs() < 10.0 {
                return;
            }
            seek_to(t);
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
        last_activity.set(js_sys::Date::now());
        if !show_controls() {
            show_controls.set(true);
        }
    };

    // Auto-hide the overlay (and cursor) after a few seconds of inactivity,
    // unless the video is paused or a menu is open. The polling loop is owned
    // by this scope, so Dioxus cancels it when the player unmounts.
    use_effect(move || {
        spawn(async move {
            loop {
                let promise = js_sys::Promise::new(&mut |resolve: js_sys::Function, _: js_sys::Function| {
                    let _ = web_sys::window().unwrap()
                        .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 500);
                });
                let _ = wasm_bindgen_futures::JsFuture::from(promise).await;

                if subtitle_menu_open() || audio_menu_open() || !is_playing() {
                    if !show_controls() {
                        show_controls.set(true);
                    }
                    continue;
                }

                if show_controls() && js_sys::Date::now() - last_activity() > 3000.0 {
                    show_controls.set(false);
                }
            }
        });
    });

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

    // Select a subtitle track (by id). None = off.
    let mut select_subtitle = move |id: Option<i64>| {
        selected_subtitle_id.set(id);
        apply_selected_subtitle();
        subtitle_menu_open.set(false);
        save_preferences();
    };

    // Cycle to the next deliverable subtitle (skips burn-required).
    let mut cycle_subtitle = move || {
        let next = api::cycle_subtitle_selection(selected_subtitle_id(), &available_subtitles());
        select_subtitle(next);
    };

    // Request a burn-in restart: fetch a fresh stream token with burn_subtitle_id set.
    // The existing HLS session is torn down; a new one is started server-side.
    let mut request_burn_subtitle = move |burn_id: i64| {
        subtitle_menu_open.set(false);
        let t = current_time();
        let prev_session = transcode_session_id();
        destroy_hls();
        spawn(async move {
            // Save progress so the new session picks up from the same position.
            if t > 0.0 {
                let _ = with_refresh(auth, |token| async move {
                    api::update_progress(&token, id, t as i32).await
                })
                .await;
            }
            // Stop the current transcode session (ignore failures).
            if let Some(sid) = prev_session {
                let _ = with_refresh(auth, move |token| {
                    let sid = sid.clone();
                    async move { api::stop_transcode(&token, &sid).await }
                })
                .await;
            }

            let (vc, ac, ct) = probe_client_capabilities();
            let audio_id = selected_audio_stream_id();
            let normalize = audio_normalize_on();
            let resp_result = with_refresh(auth, {
                let vc = vc.clone();
                let ac = ac.clone();
                let ct = ct.clone();
                move |token| {
                    let vc = vc.clone();
                    let ac = ac.clone();
                    let ct = ct.clone();
                    async move {
                        api::get_stream_token(
                            &token,
                            id,
                            vc,
                            ac,
                            ct,
                            t,
                            Some(burn_id),
                            audio_id,
                            normalize,
                        )
                        .await
                    }
                }
            })
            .await;

            match resp_result {
                Ok(resp) => {
                    let url = format!("{}{}", api::API_BASE_URL, resp.stream_url);
                    stream_mode.set(resp.mode.clone());
                    stream_url.set(Some(url.clone()));
                    transcode_session_id.set(resp.transcode_session_id.clone());
                    stream_token.set(resp.token.clone());
                    burned_subtitle_id.set(resp.burned_subtitle_id);
                    available_subtitles.set(resp.subtitles.clone());
                    available_audio_tracks.set(resp.audio_tracks.clone());
                    selected_audio_stream_id.set(resp.selected_audio_stream_id);
                    audio_normalize_on.set(resp.audio_normalize);
                    // A burn supersedes any selected VTT track.
                    selected_subtitle_id.set(None);
                    let offset = resp.transcode_actual_start_seconds.unwrap_or(t);
                    subtitle_offset.set(offset);
                    hls_time_offset.set(t);
                    last_seek_time.set(t);
                    // Re-attach (null to clear any prior track).
                    apply_selected_subtitle();
                    // Spin up hls.js for the new session
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
                            }} \
                        }}, 100);",
                        token, hls_url, hls_url
                    );
                    let _ = js_sys::eval(&js);
                    save_preferences();
                }
                Err(e) => {
                    error_msg.set(Some(format!("Burn-in restart failed: {}", e)));
                }
            }
        });
    };

    // Clear an active burn-in: fetch a fresh stream token without burn, resuming at current pos.
    let mut clear_burn = move || {
        subtitle_menu_open.set(false);
        let t = current_time();
        let prev_session = transcode_session_id();
        destroy_hls();
        spawn(async move {
            if t > 0.0 {
                let _ = with_refresh(auth, |token| async move {
                    api::update_progress(&token, id, t as i32).await
                })
                .await;
            }
            if let Some(sid) = prev_session {
                let _ = with_refresh(auth, move |token| {
                    let sid = sid.clone();
                    async move { api::stop_transcode(&token, &sid).await }
                })
                .await;
            }
            let (vc, ac, ct) = probe_client_capabilities();
            let audio_id = selected_audio_stream_id();
            let normalize = audio_normalize_on();
            let resp_result = with_refresh(auth, {
                let vc = vc.clone();
                let ac = ac.clone();
                let ct = ct.clone();
                move |token| {
                    let vc = vc.clone();
                    let ac = ac.clone();
                    let ct = ct.clone();
                    async move {
                        api::get_stream_token(
                            &token, id, vc, ac, ct, t, None, audio_id, normalize,
                        )
                        .await
                    }
                }
            })
            .await;
            if let Ok(resp) = resp_result {
                let url = format!("{}{}", api::API_BASE_URL, resp.stream_url);
                stream_mode.set(resp.mode.clone());
                stream_url.set(Some(url.clone()));
                transcode_session_id.set(resp.transcode_session_id.clone());
                stream_token.set(resp.token.clone());
                burned_subtitle_id.set(None);
                available_subtitles.set(resp.subtitles.clone());
                available_audio_tracks.set(resp.audio_tracks.clone());
                selected_audio_stream_id.set(resp.selected_audio_stream_id);
                audio_normalize_on.set(resp.audio_normalize);
                let offset = resp.transcode_actual_start_seconds.unwrap_or(t);
                subtitle_offset.set(offset);
                hls_time_offset.set(t);
                last_seek_time.set(t);
                if resp.mode != "direct" {
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
                            }} \
                        }}, 100);",
                        token, hls_url, hls_url
                    );
                    let _ = js_sys::eval(&js);
                }
                save_preferences();
            }
        });
    };

    // Request a new stream with different audio track and/or normalization.
    // Restart pattern mirrors `clear_burn`: destroy HLS, save progress, stop
    // the old transcode, fetch a fresh token, spin up hls.js on the new URL.
    // Burn state is preserved across the swap.
    let mut request_audio_change = move |new_audio_id: Option<i64>, new_normalize: bool| {
        audio_menu_open.set(false);
        let t = current_time();
        let prev_session = transcode_session_id();
        let burn_id = burned_subtitle_id();
        destroy_hls();
        spawn(async move {
            if t > 0.0 {
                let _ = with_refresh(auth, |token| async move {
                    api::update_progress(&token, id, t as i32).await
                })
                .await;
            }
            if let Some(sid) = prev_session {
                let _ = with_refresh(auth, move |token| {
                    let sid = sid.clone();
                    async move { api::stop_transcode(&token, &sid).await }
                })
                .await;
            }
            let (vc, ac, ct) = probe_client_capabilities();
            let resp_result = with_refresh(auth, {
                let vc = vc.clone();
                let ac = ac.clone();
                let ct = ct.clone();
                move |token| {
                    let vc = vc.clone();
                    let ac = ac.clone();
                    let ct = ct.clone();
                    async move {
                        api::get_stream_token(
                            &token,
                            id,
                            vc,
                            ac,
                            ct,
                            t,
                            burn_id,
                            new_audio_id,
                            new_normalize,
                        )
                        .await
                    }
                }
            })
            .await;
            match resp_result {
                Ok(resp) => {
                    let url = format!("{}{}", api::API_BASE_URL, resp.stream_url);
                    stream_mode.set(resp.mode.clone());
                    stream_url.set(Some(url.clone()));
                    transcode_session_id.set(resp.transcode_session_id.clone());
                    stream_token.set(resp.token.clone());
                    available_subtitles.set(resp.subtitles.clone());
                    burned_subtitle_id.set(resp.burned_subtitle_id);
                    available_audio_tracks.set(resp.audio_tracks.clone());
                    selected_audio_stream_id.set(resp.selected_audio_stream_id);
                    audio_normalize_on.set(resp.audio_normalize);
                    let offset = resp.transcode_actual_start_seconds.unwrap_or(t);
                    subtitle_offset.set(offset);
                    hls_time_offset.set(t);
                    last_seek_time.set(t);
                    apply_selected_subtitle();
                    if resp.mode != "direct" {
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
                                }} \
                            }}, 100);",
                            token, hls_url, hls_url
                        );
                        let _ = js_sys::eval(&js);
                    }
                    save_preferences();
                }
                Err(e) => {
                    error_msg.set(Some(format!("Audio change failed: {}", e)));
                }
            }
        });
    };

    // Cycle audio tracks with the same keyboard shortcut pattern as subtitles.
    let mut cycle_audio = move || {
        let list = available_audio_tracks();
        if list.len() < 2 {
            return;
        }
        let next = api::cycle_audio_selection(selected_audio_stream_id(), &list);
        request_audio_change(next, audio_normalize_on());
    };

    let go_back = move |_| {
        destroy_hls();
        // Save progress and stop transcode session, then navigate away
        let t = current_time();
        let session = transcode_session_id();
        spawn(async move {
            if t > 0.0 {
                let _ = with_refresh(auth, |token| async move {
                    api::update_progress(&token, id, t as i32).await
                })
                .await;
            }
            if let Some(ref sid) = session {
                let sid = sid.clone();
                let _ = with_refresh(auth, move |token| {
                    let sid = sid.clone();
                    async move { api::stop_transcode(&token, &sid).await }
                })
                .await;
            }
            nav.go_back();
        });
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

    // Keyboard controls via JS: space = play/pause, left/right = skip 10s.
    // Direct mode: JS seeks the video element directly.
    // HLS mode: JS simulates a click on the skip buttons to reuse the Dioxus seek flow.
    use_effect(move || {
        let _ = js_sys::eval(r#"
            (function() {
                if (window._orthanc_keydown) {
                    window.removeEventListener('keydown', window._orthanc_keydown);
                }
                window._orthanc_keydown = function(e) {
                    var tag = e.target && e.target.tagName;
                    if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
                    var video = document.getElementById('orthanc-player');
                    if (!video) return;
                    if (e.key === ' ') {
                        e.preventDefault();
                        if (video.paused) { video.play(); } else { video.pause(); }
                    } else if (e.key === 'ArrowLeft') {
                        e.preventDefault();
                        var btn = document.querySelector('[data-action="skip-back"]');
                        if (btn) btn.click();
                    } else if (e.key === 'ArrowRight') {
                        e.preventDefault();
                        var btn = document.querySelector('[data-action="skip-forward"]');
                        if (btn) btn.click();
                    } else if (e.key === 'c' || e.key === 'C') {
                        e.preventDefault();
                        var btn = document.querySelector('[data-action="subtitle-cycle"]');
                        if (btn) btn.click();
                    } else if (e.key === 'a' || e.key === 'A') {
                        e.preventDefault();
                        var btn = document.querySelector('[data-action="audio-cycle"]');
                        if (btn) btn.click();
                    }
                };
                window.addEventListener('keydown', window._orthanc_keydown);
            })()
        "#);
    });

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
            class: if show_controls() { "player-container" } else { "player-container idle" },
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
                    crossorigin: "anonymous",
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
                            video.set_volume(initial_volume);
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
                                // Skip back 10s (Material Design replay_10)
                                button {
                                    "data-action": "skip-back",
                                    class: "player-icon-btn player-skip-btn",
                                    onclick: skip_back,
                                    dangerous_inner_html: r#"<svg viewBox="0 0 24 24" fill="currentColor" style="pointer-events:none"><path d="M11.99,5V1l-5,5l5,5V7c3.31,0,6,2.69,6,6s-2.69,6-6,6s-6-2.69-6-6h-2c0,4.42,3.58,8,8,8s8-3.58,8-8S16.41,5,11.99,5z"/><path d="M10.89,16h-0.85v-3.26l-1.01,0.31v-0.69l1.77-0.63h0.09V16z"/><path d="M15.17,14.24c0,0.32-0.03,0.6-0.1,0.82s-0.17,0.42-0.29,0.57s-0.28,0.26-0.45,0.33s-0.37,0.1-0.59,0.1s-0.41-0.03-0.59-0.1s-0.33-0.18-0.46-0.33s-0.23-0.34-0.3-0.57s-0.11-0.5-0.11-0.82V13.5c0-0.32,0.03-0.6,0.1-0.82s0.17-0.42,0.29-0.57s0.28-0.26,0.45-0.33s0.37-0.1,0.59-0.1s0.41,0.03,0.59,0.1c0.18,0.07,0.33,0.18,0.46,0.33s0.23,0.34,0.3,0.57s0.11,0.5,0.11,0.82V14.24z M14.32,13.38c0-0.19-0.01-0.35-0.04-0.48s-0.07-0.23-0.12-0.31s-0.11-0.14-0.19-0.17s-0.16-0.05-0.25-0.05s-0.18,0.02-0.25,0.05s-0.14,0.09-0.19,0.17s-0.09,0.18-0.12,0.31s-0.04,0.29-0.04,0.48v0.97c0,0.19,0.01,0.35,0.04,0.48s0.07,0.24,0.12,0.32s0.11,0.14,0.19,0.17s0.16,0.05,0.25,0.05s0.18-0.02,0.25-0.05s0.14-0.09,0.19-0.17s0.09-0.19,0.11-0.32s0.04-0.29,0.04-0.48V13.38z"/></svg>"#,
                                }
                                // Skip forward 10s (Material Design forward_10)
                                button {
                                    "data-action": "skip-forward",
                                    class: "player-icon-btn player-skip-btn",
                                    onclick: skip_forward,
                                    dangerous_inner_html: r#"<svg viewBox="0 0 24 24" fill="currentColor" style="pointer-events:none"><path d="M18,13c0,3.31-2.69,6-6,6s-6-2.69-6-6s2.69-6,6-6v4l5-5l-5-5v4c-4.42,0-8,3.58-8,8c0,4.42,3.58,8,8,8s8-3.58,8-8H18z"/><polygon points="10.86,15.94 10.86,11.67 10.77,11.67 9,12.3 9,12.99 10.01,12.68 10.01,15.94"/><path d="M12.25,13.44v0.74c0,1.9,1.31,1.82,1.44,1.82c0.14,0,1.44,0.09,1.44-1.82v-0.74c0-1.9-1.31-1.82-1.44-1.82C13.55,11.62,12.25,11.53,12.25,13.44z M14.29,13.32v0.97c0,0.77-0.21,1.03-0.59,1.03c-0.38,0-0.6-0.26-0.6-1.03v-0.97c0-0.75,0.22-1.01,0.59-1.01C14.07,12.3,14.29,12.57,14.29,13.32z"/></svg>"#,
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
                                // Audio menu (shown whenever an audio stream exists;
                                // covers track picking + loudness normalization).
                                if !available_audio_tracks().is_empty() {
                                    div { class: "player-audio-wrap",
                                        button {
                                            class: if audio_normalize_on() { "player-icon-btn player-audio-btn active" } else { "player-icon-btn player-audio-btn" },
                                            onclick: move |_| {
                                                let v = !audio_menu_open();
                                                audio_menu_open.set(v);
                                            },
                                            dangerous_inner_html: r#"<svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 3a9 9 0 0 0-9 9v5a3 3 0 0 0 3 3h1a1 1 0 0 0 1-1v-6a1 1 0 0 0-1-1H5.07A7 7 0 0 1 19 12v.07H17a1 1 0 0 0-1 1V19a1 1 0 0 0 1 1h1a3 3 0 0 0 3-3v-5a9 9 0 0 0-9-9z"/></svg>"#,
                                        }
                                        if audio_menu_open() {
                                            div { class: "player-audio-menu",
                                                onclick: move |e| e.stop_propagation(),
                                                div { class: "player-audio-menu-header", "Audio" }
                                                for track in available_audio_tracks().iter() {
                                                    {
                                                        let label = audio_label(track);
                                                        let tid = track.id;
                                                        let is_selected = selected_audio_stream_id() == Some(tid);
                                                        let normalize = audio_normalize_on();
                                                        rsx! {
                                                            button {
                                                                key: "{tid}",
                                                                class: if is_selected { "player-audio-item selected" } else { "player-audio-item" },
                                                                onclick: move |_| {
                                                                    if !is_selected {
                                                                        request_audio_change(Some(tid), normalize);
                                                                    } else {
                                                                        audio_menu_open.set(false);
                                                                    }
                                                                },
                                                                "{label}"
                                                            }
                                                        }
                                                    }
                                                }
                                                div { class: "player-audio-divider", "Output" }
                                                {
                                                    let normalize_on = audio_normalize_on();
                                                    let current_audio = selected_audio_stream_id();
                                                    rsx! {
                                                        button {
                                                            class: if normalize_on { "player-audio-item player-audio-toggle selected" } else { "player-audio-item player-audio-toggle" },
                                                            onclick: move |_| {
                                                                request_audio_change(current_audio, !normalize_on);
                                                            },
                                                            if normalize_on { "Normalize loudness: On" } else { "Normalize loudness: Off" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        // Hidden cycle button — target of keyboard 'a'/'A'
                                        button {
                                            "data-action": "audio-cycle",
                                            style: "display:none",
                                            onclick: move |_| cycle_audio(),
                                        }
                                    }
                                }
                                // Subtitle menu (only if subtitles exist)
                                if !available_subtitles().is_empty() {
                                    div { class: "player-subtitle-wrap",
                                        button {
                                            class: if selected_subtitle_id().is_some() || burned_subtitle_id().is_some() { "player-icon-btn player-sub-btn active" } else { "player-icon-btn player-sub-btn" },
                                            onclick: move |_| {
                                                let v = !subtitle_menu_open();
                                                subtitle_menu_open.set(v);
                                            },
                                            dangerous_inner_html: r#"<svg viewBox="0 0 24 24" fill="currentColor"><path d="M20 4H4c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V6c0-1.1-.9-2-2-2zM4 12h4v2H4v-2zm10 6H4v-2h10v2zm6 0h-4v-2h4v2zm0-4H10v-2h10v2z"/></svg>"#,
                                        }
                                        if subtitle_menu_open() {
                                            div { class: "player-subtitle-menu",
                                                onclick: move |e| e.stop_propagation(),
                                                div { class: "player-subtitle-menu-header", "Subtitles" }
                                                // "Off" option
                                                button {
                                                    class: if selected_subtitle_id().is_none() && burned_subtitle_id().is_none() { "player-subtitle-item selected" } else { "player-subtitle-item" },
                                                    onclick: move |_| {
                                                        if burned_subtitle_id().is_some() {
                                                            clear_burn();
                                                        } else {
                                                            select_subtitle(None);
                                                        }
                                                    },
                                                    "Off"
                                                }
                                                // Deliverable VTT tracks
                                                for sub in available_subtitles().iter().filter(|s| s.delivery == "vtt") {
                                                    {
                                                        let label = subtitle_label(sub);
                                                        let sid = sub.id;
                                                        let is_selected = selected_subtitle_id() == Some(sid) && burned_subtitle_id().is_none();
                                                        rsx! {
                                                            button {
                                                                key: "{sid}",
                                                                class: if is_selected { "player-subtitle-item selected" } else { "player-subtitle-item" },
                                                                onclick: move |_| select_subtitle(Some(sid)),
                                                                "{label}"
                                                            }
                                                        }
                                                    }
                                                }
                                                // Burn-required tracks (separate section)
                                                if available_subtitles().iter().any(|s| s.delivery == "burn_required") {
                                                    div { class: "player-subtitle-divider", "Burn-in only" }
                                                    for sub in available_subtitles().iter().filter(|s| s.delivery == "burn_required") {
                                                        {
                                                            let label = subtitle_label(sub);
                                                            let sid = sub.id;
                                                            let is_burned = burned_subtitle_id() == Some(sid);
                                                            rsx! {
                                                                button {
                                                                    key: "{sid}",
                                                                    class: if is_burned { "player-subtitle-item selected" } else { "player-subtitle-item" },
                                                                    onclick: move |_| request_burn_subtitle(sid),
                                                                    "{label} [Burn]"
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        // Hidden cycle button — target of keyboard 'c'/'C'
                                        button {
                                            "data-action": "subtitle-cycle",
                                            style: "display:none",
                                            onclick: move |_| cycle_subtitle(),
                                        }
                                    }
                                }
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
