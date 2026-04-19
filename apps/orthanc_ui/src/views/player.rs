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

#[component]
pub fn Player(id: i64) -> Element {
    let auth = use_context::<Signal<AuthState>>();
    let mut stream_url = use_signal(|| None::<String>);
    let mut error_msg = use_signal(|| None::<String>);
    let mut is_playing = use_signal(|| false);
    let mut current_time = use_signal(|| 0.0_f64);
    let mut duration = use_signal(|| 0.0_f64);
    let mut show_controls = use_signal(|| true);
    let mut volume = use_signal(|| 1.0_f64);
    let mut title = use_signal(|| String::new());
    let mut initial_position = use_signal(|| 0_i32);
    let mut last_save_time = use_signal(|| 0.0_f64);
    let nav = use_navigator();

    // Fetch stream token and progress on mount
    use_effect(move || {
        spawn(async move {
            // Get stream token
            let token_result = with_refresh(auth, |token| async move {
                api::get_stream_token(&token, id).await
            })
            .await;

            match token_result {
                Ok(resp) => {
                    let url = format!("{}{}", api::API_BASE_URL, resp.stream_url);
                    stream_url.set(Some(url));
                }
                Err(e) => {
                    error_msg.set(Some(format!("Failed to get stream: {}", e)));
                    return;
                }
            }

            // Get saved progress for resume
            let progress_result = with_refresh(auth, |token| async move {
                api::get_progress(&token, id).await
            })
            .await;

            if let Ok(progress) = progress_result {
                if !progress.is_completed && progress.position_seconds > 0 {
                    initial_position.set(progress.position_seconds);
                }
            }

            // Get media info for title
            let movie_result = with_refresh(auth, |token| async move {
                api::get_movie(&token, id).await
            })
            .await;

            if let Ok(media) = movie_result {
                title.set(media.title);
            }
        });
    });

    let toggle_play = move |_| {
        if let Some(video) = get_video() {
            if video.paused() {
                let _ = video.play();
            } else {
                video.pause().ok();
                // Save progress on pause
                let t = video.current_time();
                spawn(async move {
                    let _ = with_refresh(auth, |token| async move {
                        api::update_progress(&token, id, t as i32).await
                    })
                    .await;
                });
            }
        }
    };

    let on_seek = move |evt: Event<FormData>| {
        if let Some(video) = get_video() {
            if let Ok(t) = evt.value().parse::<f64>() {
                video.set_current_time(t);
                current_time.set(t);
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
                if let Some(el) = document.get_element_by_id("player-container") {
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

    rsx! {
        div {
            id: "player-container",
            class: "player-container",
            onmousemove: on_mouse_move,

            if let Some(ref err) = error_msg() {
                div { class: "player-error",
                    p { "{err}" }
                    button { onclick: go_back, "Go Back" }
                }
            } else if let Some(ref url) = stream_url() {
                video {
                    id: "orthanc-player",
                    class: "player-video",
                    src: "{url}",
                    preload: "metadata",
                    onplay: move |_| is_playing.set(true),
                    onpause: move |_| is_playing.set(false),
                    ontimeupdate: move |_| {
                        if let Some(video) = get_video() {
                            let t = video.current_time();
                            current_time.set(t);
                            // Save progress every ~10 seconds
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
                            duration.set(video.duration());
                            // Resume from saved position
                            let pos = initial_position();
                            if pos > 0 {
                                video.set_current_time(pos as f64);
                            }
                        }
                    },
                    onerror: move |_| {
                        error_msg.set(Some("Failed to load video. The file format may not be supported by your browser.".to_string()));
                    },
                }

                // Controls overlay
                div {
                    class: if show_controls() { "player-controls" } else { "player-controls hidden" },

                    // Top bar
                    div { class: "player-top-bar",
                        button { class: "player-btn player-back-btn", onclick: go_back, "Back" }
                        h2 { class: "player-title", "{title}" }
                    }

                    // Center play button
                    div { class: "player-center",
                        button {
                            class: "player-btn player-play-big",
                            onclick: toggle_play,
                            if is_playing() { "Pause" } else { "Play" }
                        }
                    }

                    // Bottom controls
                    div { class: "player-bottom-bar",
                        button {
                            class: "player-btn",
                            onclick: toggle_play,
                            if is_playing() { "||" } else { ">" }
                        }
                        span { class: "player-time", "{cur}" }
                        input {
                            r#type: "range",
                            class: "player-seek",
                            min: "0",
                            max: "{duration}",
                            step: "0.1",
                            value: "{current_time}",
                            oninput: on_seek,
                        }
                        span { class: "player-time", "{dur}" }
                        input {
                            r#type: "range",
                            class: "player-volume",
                            min: "0",
                            max: "1",
                            step: "0.05",
                            value: "{volume}",
                            oninput: on_volume,
                        }
                        button {
                            class: "player-btn",
                            onclick: toggle_fullscreen,
                            "Fullscreen"
                        }
                    }
                }
            } else {
                div { class: "player-loading", "Loading..." }
            }
        }
    }
}
