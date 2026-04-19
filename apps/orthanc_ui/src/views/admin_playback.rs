use dioxus::prelude::*;
use crate::{
    api::{self, ActiveStreamRow},
    state::{AuthState, with_refresh},
};

const POLL_INTERVAL_MS: i32 = 3000;

async fn sleep_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve: js_sys::Function, _: js_sys::Function| {
        let _ = web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

fn format_mode(mode: &str) -> &'static str {
    match mode {
        "direct" => "Direct",
        "remux" => "Remux",
        "audio_transcode" => "Audio Transcode",
        "full_transcode" => "Full Transcode",
        _ => "Unknown",
    }
}

#[component]
pub fn AdminPlayback() -> Element {
    let auth = use_context::<Signal<AuthState>>();
    let nav = use_navigator();

    if !auth.read().is_admin() {
        nav.replace(crate::Route::Home {});
    }

    let mut streams = use_signal(Vec::<ActiveStreamRow>::new);
    let mut error = use_signal(|| Option::<String>::None);
    let mut loaded_once = use_signal(|| false);

    let load = move || {
        spawn(async move {
            match with_refresh(auth, |token| async move {
                api::list_active_streams(&token).await
            }).await {
                Ok(rows) => {
                    streams.set(rows);
                    error.set(None);
                }
                Err(e) => error.set(Some(e)),
            }
            loaded_once.set(true);
        });
    };

    // Poll on mount and every POLL_INTERVAL_MS while the component is mounted.
    use_effect(move || {
        spawn(async move {
            loop {
                match with_refresh(auth, |token| async move {
                    api::list_active_streams(&token).await
                }).await {
                    Ok(rows) => {
                        streams.set(rows);
                        error.set(None);
                    }
                    Err(e) => error.set(Some(e)),
                }
                loaded_once.set(true);
                sleep_ms(POLL_INTERVAL_MS).await;
            }
        });
    });

    rsx! {
        div { class: "page",
            div { class: "page-header",
                h1 { class: "page-title", "Active Streams" }
                button {
                    class: "btn btn-secondary",
                    onclick: move |_| load(),
                    "Refresh"
                }
            }

            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }

            if !loaded_once() {
                div { class: "loading", "Loading..." }
            } else if streams().is_empty() {
                div { class: "card", p { "No active transcode sessions." } }
            } else {
                div { class: "card",
                    table { class: "table",
                        thead {
                            tr {
                                th { "User" }
                                th { "Media" }
                                th { "Mode" }
                                th { "Resolution" }
                                th { "Position" }
                                th { "Idle" }
                                th { "Session" }
                                th { "Actions" }
                            }
                        }
                        tbody {
                            for row in streams() {
                                {
                                    let sid = row.session_id.clone();
                                    let sid_key = sid.clone();
                                    let user = row.username.clone().unwrap_or_else(|| format!("user #{}", row.user_id));
                                    let title = row.media_title.clone().unwrap_or_else(|| format!("media #{}", row.media_item_id));
                                    let mode = format_mode(&row.mode);
                                    let height = row.video_height.map(|h| format!("{}p", h)).unwrap_or_else(|| "—".to_string());
                                    let position = format!("{:.0}s", row.start_time_seconds);
                                    let idle = format!("{}s", row.idle_seconds);
                                    let short_sid: String = sid.chars().take(8).collect();
                                    let reload = load.clone();
                                    rsx! {
                                        tr { key: "{sid_key}",
                                            td { "{user}" }
                                            td { "{title}" }
                                            td { span { class: "badge", "{mode}" } }
                                            td { "{height}" }
                                            td { "{position}" }
                                            td { "{idle}" }
                                            td { code { "{short_sid}" } }
                                            td {
                                                button {
                                                    class: "btn btn-sm btn-danger",
                                                    onclick: move |_| {
                                                        let sid = sid.clone();
                                                        let reload = reload.clone();
                                                        spawn(async move {
                                                            let _ = with_refresh(auth, |token| {
                                                                let sid = sid.clone();
                                                                async move {
                                                                    api::stop_active_stream(&token, &sid).await
                                                                }
                                                            }).await;
                                                            reload();
                                                        });
                                                    },
                                                    "Stop"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
