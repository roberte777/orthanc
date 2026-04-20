use dioxus::prelude::*;
use crate::{
    api::{self, ChangePasswordRequest, UpdateProfileRequest, UserPreferences},
    state::{self, AuthState, with_refresh},
};

#[derive(Clone, PartialEq)]
enum SettingsTab {
    Profile,
    Password,
    Playback,
}

pub const LS_DEFAULT_VOLUME: &str = "player_default_volume";
pub const LS_SKIP_SECONDS: &str = "player_skip_seconds";

#[component]
pub fn Settings() -> Element {
    let mut tab = use_signal(|| SettingsTab::Profile);

    rsx! {
        div { class: "page settings-page",
            h1 { class: "page-title", "Settings" }
            div { class: "settings-layout",
                // Sidebar
                nav { class: "settings-sidebar",
                    button {
                        class: if tab() == SettingsTab::Profile {
                            "sidebar-item active"
                        } else {
                            "sidebar-item"
                        },
                        onclick: move |_| tab.set(SettingsTab::Profile),
                        "Profile"
                    }
                    button {
                        class: if tab() == SettingsTab::Password {
                            "sidebar-item active"
                        } else {
                            "sidebar-item"
                        },
                        onclick: move |_| tab.set(SettingsTab::Password),
                        "Password"
                    }
                    button {
                        class: if tab() == SettingsTab::Playback {
                            "sidebar-item active"
                        } else {
                            "sidebar-item"
                        },
                        onclick: move |_| tab.set(SettingsTab::Playback),
                        "Playback"
                    }
                }
                // Content
                div { class: "settings-content",
                    match tab() {
                        SettingsTab::Profile => rsx! { ProfileSection {} },
                        SettingsTab::Password => rsx! { PasswordSection {} },
                        SettingsTab::Playback => rsx! { PlaybackSection {} },
                    }
                }
            }
        }
    }
}

#[component]
fn ProfileSection() -> Element {
    let mut auth = use_context::<Signal<AuthState>>();
    let user = auth.read().user.clone();

    let mut display_name = use_signal(|| {
        user.as_ref()
            .and_then(|u| u.display_name.clone())
            .unwrap_or_default()
    });
    let mut email = use_signal(|| {
        user.as_ref().map(|u| u.email.clone()).unwrap_or_default()
    });
    let mut message = use_signal(|| Option::<(bool, String)>::None);
    let mut loading = use_signal(|| false);

    let on_submit = move |evt: Event<FormData>| {
        evt.prevent_default();
        loading.set(true);
        message.set(None);
        let token = auth.read().access_token.clone().unwrap_or_default();
        let dn = display_name();
        let em = email();
        spawn(async move {
            let req = UpdateProfileRequest {
                display_name: if dn.is_empty() { None } else { Some(dn) },
                email: Some(em),
            };
            match api::update_profile(&token, req).await {
                Ok(user) => {
                    auth.write().user = Some(user);
                    message.set(Some((true, "Profile updated".to_string())));
                }
                Err(e) => {
                    message.set(Some((false, format!("Error: {}", e))));
                }
            }
            loading.set(false);
        });
    };

    rsx! {
        div { class: "settings-section",
            h2 { "Profile" }
            if let Some((ok, msg)) = message() {
                div {
                    class: if ok { "success-msg" } else { "error-msg" },
                    "{msg}"
                }
            }
            form { onsubmit: on_submit,
                div { class: "form-group",
                    label { class: "form-label", "Display Name" }
                    input {
                        class: "form-input",
                        r#type: "text",
                        value: "{display_name}",
                        oninput: move |e| display_name.set(e.value()),
                    }
                }
                div { class: "form-group",
                    label { class: "form-label", "Email" }
                    input {
                        class: "form-input",
                        r#type: "email",
                        value: "{email}",
                        oninput: move |e| email.set(e.value()),
                        required: true,
                    }
                }
                button {
                    class: "btn btn-primary",
                    r#type: "submit",
                    disabled: loading(),
                    if loading() { "Saving..." } else { "Save Changes" }
                }
            }
        }
    }
}

#[component]
fn PasswordSection() -> Element {
    let auth = use_context::<Signal<AuthState>>();
    let mut current = use_signal(String::new);
    let mut new_pass = use_signal(String::new);
    let mut confirm = use_signal(String::new);
    let mut message = use_signal(|| Option::<(bool, String)>::None);
    let mut loading = use_signal(|| false);

    let on_submit = move |evt: Event<FormData>| {
        evt.prevent_default();
        if new_pass() != confirm() {
            message.set(Some((false, "Passwords do not match".to_string())));
            return;
        }
        loading.set(true);
        message.set(None);
        let token = auth.read().access_token.clone().unwrap_or_default();
        spawn(async move {
            let req = ChangePasswordRequest {
                current_password: current(),
                new_password: new_pass(),
            };
            match api::change_password(&token, req).await {
                Ok(_) => {
                    current.set(String::new());
                    new_pass.set(String::new());
                    confirm.set(String::new());
                    message.set(Some((
                        true,
                        "Password changed successfully".to_string(),
                    )));
                }
                Err(e) => {
                    message.set(Some((false, format!("Error: {}", e))));
                }
            }
            loading.set(false);
        });
    };

    rsx! {
        div { class: "settings-section",
            h2 { "Change Password" }
            if let Some((ok, msg)) = message() {
                div {
                    class: if ok { "success-msg" } else { "error-msg" },
                    "{msg}"
                }
            }
            form { onsubmit: on_submit,
                div { class: "form-group",
                    label { class: "form-label", "Current Password" }
                    input {
                        class: "form-input",
                        r#type: "password",
                        value: "{current}",
                        oninput: move |e| current.set(e.value()),
                        required: true,
                    }
                }
                div { class: "form-group",
                    label { class: "form-label", "New Password" }
                    input {
                        class: "form-input",
                        r#type: "password",
                        value: "{new_pass}",
                        oninput: move |e| new_pass.set(e.value()),
                        required: true,
                    }
                }
                div { class: "form-group",
                    label { class: "form-label", "Confirm New Password" }
                    input {
                        class: "form-input",
                        r#type: "password",
                        value: "{confirm}",
                        oninput: move |e| confirm.set(e.value()),
                        required: true,
                    }
                }
                button {
                    class: "btn btn-primary",
                    r#type: "submit",
                    disabled: loading(),
                    if loading() { "Changing..." } else { "Change Password" }
                }
            }
        }
    }
}

#[component]
fn PlaybackSection() -> Element {
    let auth = use_context::<Signal<AuthState>>();

    // Server-synced language defaults
    let mut audio_lang = use_signal(String::new);
    let mut subtitle_lang = use_signal(String::new);
    let mut subtitles_on = use_signal(|| false);
    let mut normalize_on = use_signal(|| false);
    let mut loaded = use_signal(|| false);

    // Client-only (localStorage) player UX
    let mut volume = use_signal(|| {
        state::storage_get(LS_DEFAULT_VOLUME)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(1.0)
    });
    let mut skip_seconds = use_signal(|| {
        state::storage_get(LS_SKIP_SECONDS)
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(10)
    });

    let mut message = use_signal(|| Option::<(bool, String)>::None);
    let mut saving = use_signal(|| false);

    use_effect(move || {
        spawn(async move {
            let prefs = with_refresh(auth, |token| async move {
                api::get_user_preferences(&token).await
            })
            .await;
            if let Ok(p) = prefs {
                audio_lang.set(p.preferred_audio_language.unwrap_or_default());
                subtitle_lang.set(p.preferred_subtitle_language.unwrap_or_default());
                subtitles_on.set(p.subtitles_enabled_default);
                normalize_on.set(p.audio_normalize_default);
            }
            loaded.set(true);
        });
    });

    let on_submit = move |evt: Event<FormData>| {
        evt.prevent_default();
        saving.set(true);
        message.set(None);

        // Save localStorage first (fire-and-forget — no network)
        state::storage_set(LS_DEFAULT_VOLUME, &format!("{}", volume()));
        state::storage_set(LS_SKIP_SECONDS, &format!("{}", skip_seconds()));

        let prefs = UserPreferences {
            preferred_audio_language: {
                let s = audio_lang();
                if s.trim().is_empty() { None } else { Some(s) }
            },
            preferred_subtitle_language: {
                let s = subtitle_lang();
                if s.trim().is_empty() { None } else { Some(s) }
            },
            subtitles_enabled_default: subtitles_on(),
            audio_normalize_default: normalize_on(),
        };

        spawn(async move {
            let result = with_refresh(auth, move |token| {
                let prefs = prefs.clone();
                async move { api::update_user_preferences(&token, &prefs).await }
            })
            .await;
            match result {
                Ok(_) => message.set(Some((true, "Playback preferences saved".to_string()))),
                Err(e) => message.set(Some((false, format!("Error: {}", e)))),
            }
            saving.set(false);
        });
    };

    rsx! {
        div { class: "settings-section",
            h2 { "Playback" }
            p { class: "setting-description",
                "Global defaults applied when a show has no saved track preference. Language codes are ISO 639-2 (three letters), e.g. eng, jpn, spa."
            }
            if let Some((ok, msg)) = message() {
                div {
                    class: if ok { "success-msg" } else { "error-msg" },
                    "{msg}"
                }
            }
            if !loaded() {
                div { class: "loading", "Loading preferences..." }
            } else {
                form { onsubmit: on_submit,
                    div { class: "form-group",
                        label { class: "form-label", "Preferred Audio Language" }
                        input {
                            class: "form-input",
                            r#type: "text",
                            placeholder: "e.g. eng",
                            maxlength: 3,
                            value: "{audio_lang}",
                            oninput: move |e| audio_lang.set(e.value()),
                        }
                    }
                    div { class: "form-group",
                        label { class: "form-label", "Preferred Subtitle Language" }
                        input {
                            class: "form-input",
                            r#type: "text",
                            placeholder: "e.g. eng",
                            maxlength: 3,
                            value: "{subtitle_lang}",
                            oninput: move |e| subtitle_lang.set(e.value()),
                        }
                    }
                    div { class: "form-group",
                        label { class: "toggle-row",
                            input {
                                r#type: "checkbox",
                                checked: subtitles_on(),
                                onchange: move |e| subtitles_on.set(e.checked()),
                            }
                            span { " Show subtitles by default" }
                        }
                    }
                    div { class: "form-group",
                        label { class: "toggle-row",
                            input {
                                r#type: "checkbox",
                                checked: normalize_on(),
                                onchange: move |e| normalize_on.set(e.checked()),
                            }
                            span { " Normalize audio loudness by default" }
                        }
                    }
                    div { class: "form-group",
                        label { class: "form-label", "Default Volume" }
                        input {
                            class: "form-input",
                            r#type: "range",
                            min: "0",
                            max: "1",
                            step: "0.05",
                            value: "{volume}",
                            oninput: move |e| {
                                if let Ok(v) = e.value().parse::<f64>() {
                                    volume.set(v);
                                }
                            },
                        }
                        span { class: "setting-description", "{(volume() * 100.0).round() as i32}%" }
                    }
                    div { class: "form-group",
                        label { class: "form-label", "Skip Forward/Back (seconds)" }
                        input {
                            class: "form-input",
                            r#type: "number",
                            min: "1",
                            max: "120",
                            value: "{skip_seconds}",
                            oninput: move |e| {
                                if let Ok(v) = e.value().parse::<i64>() {
                                    skip_seconds.set(v);
                                }
                            },
                        }
                    }
                    button {
                        class: "btn btn-primary",
                        r#type: "submit",
                        disabled: saving(),
                        if saving() { "Saving..." } else { "Save Changes" }
                    }
                }
            }
        }
    }
}
