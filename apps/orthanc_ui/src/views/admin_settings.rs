use dioxus::prelude::*;
use crate::{api::{self, Setting}, state::{AuthState, with_refresh}};

struct SettingMeta {
    key: &'static str,
    label: &'static str,
    section: &'static str,
    secret: bool,
}

const KNOWN_SETTINGS: &[SettingMeta] = &[
    SettingMeta { key: "server_name",                   label: "Server Name",                  section: "General",   secret: false },
    SettingMeta { key: "library_scan_interval_minutes", label: "Default Scan Interval (min)",  section: "Libraries", secret: false },
    SettingMeta { key: "max_concurrent_streams",        label: "Max Concurrent Streams",       section: "Streaming", secret: false },
    SettingMeta { key: "max_concurrent_transcodes",     label: "Max Concurrent Transcodes",    section: "Streaming", secret: false },
    SettingMeta { key: "stream_token_expiry_minutes",   label: "Stream Token Expiry (min)",    section: "Streaming", secret: false },
    SettingMeta { key: "subtitle_cache_max_mb",         label: "Subtitle Cache Max (MB)",      section: "Cache",     secret: false },
    SettingMeta { key: "tmdb_api_key",                  label: "TMDB API Key",                 section: "Metadata",  secret: true  },
    SettingMeta { key: "tvdb_api_key",                  label: "TVDB API Key",                 section: "Metadata",  secret: true  },
];

fn is_secret(key: &str) -> bool {
    KNOWN_SETTINGS.iter().find(|m| m.key == key).map(|m| m.secret).unwrap_or(false)
}

const SECTIONS: &[&str] = &["General", "Libraries", "Streaming", "Cache", "Metadata"];

fn label_for(key: &str) -> &'static str {
    KNOWN_SETTINGS.iter().find(|m| m.key == key).map(|m| m.label).unwrap_or("Unknown")
}

fn section_for(key: &str) -> &'static str {
    KNOWN_SETTINGS.iter().find(|m| m.key == key).map(|m| m.section).unwrap_or("Other")
}

#[component]
pub fn AdminSettings() -> Element {
    let auth = use_context::<Signal<AuthState>>();
    let nav = use_navigator();

    if !auth.read().is_admin() {
        nav.replace(crate::Route::Home {});
    }

    let token = auth.read().access_token.clone().unwrap_or_default();
    let mut settings = use_signal(Vec::<Setting>::new);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| Option::<String>::None);
    let mut save_message = use_signal(|| Option::<(bool, String)>::None);
    let mut edits = use_signal(std::collections::HashMap::<String, String>::new);

    use_effect(move || {
        spawn(async move {
            match with_refresh(auth, |token| async move {
                api::get_server_settings(&token).await
            }).await {
                Ok(s) => {
                    let mut map = std::collections::HashMap::new();
                    for setting in &s {
                        map.insert(setting.key.clone(), setting.value.clone());
                    }
                    edits.set(map);
                    settings.set(s);
                }
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    });

    let tok2 = token.clone();
    let on_save = move |_| {
        let tok = tok2.clone();
        let current_edits = edits.read().clone();
        save_message.set(None);
        spawn(async move {
            let mut failed = Vec::new();
            for (key, value) in &current_edits {
                if let Err(e) = api::update_server_setting(&tok, key, value).await {
                    failed.push(format!("{}: {}", key, e));
                }
            }
            if failed.is_empty() {
                save_message.set(Some((true, "Settings saved".to_string())));
            } else {
                save_message.set(Some((false, format!("Some settings failed to save: {}", failed.join(", ")))));
            }
        });
    };

    // Build owned section data so we don't hold borrows inside rsx!
    let section_data: Vec<(&str, Vec<Setting>)> = SECTIONS.iter().map(|section| {
        let items = settings.read().iter()
            .filter(|s| section_for(&s.key) == *section)
            .cloned()
            .collect::<Vec<_>>();
        (*section, items)
    }).collect();

    rsx! {
        div { class: "page settings-page",
            div { class: "page-header",
                h1 { class: "page-title", "Server Settings" }
                button { class: "btn btn-primary", onclick: on_save, "Save Changes" }
            }

            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }
            if let Some((ok, msg)) = save_message() {
                div { class: if ok { "success-msg" } else { "error-msg" }, "{msg}" }
            }

            if loading() {
                div { class: "loading", "Loading settings..." }
            } else {
                div { class: "settings-sections",
                    for (section, items) in section_data {
                        if !items.is_empty() {
                            div { class: "settings-card",
                                h2 { class: "settings-card-title", "{section}" }
                                for setting in items {
                                    {
                                        let key = setting.key.clone();
                                        let key2 = key.clone();
                                        let value_type = setting.value_type.clone();
                                        let description = setting.description.clone();
                                        let label = label_for(&key);
                                        let current_value = edits.read().get(&key2).cloned().unwrap_or_default();

                                        rsx! {
                                            div { class: "setting-row", key: "{key2}",
                                                div { class: "setting-label-group",
                                                    label { class: "setting-label", "{label}" }
                                                    if let Some(desc) = description {
                                                        p { class: "setting-description", "{desc}" }
                                                    }
                                                }
                                                div { class: "setting-control",
                                                    if value_type == "boolean" {
                                                        label { class: "toggle",
                                                            input {
                                                                r#type: "checkbox",
                                                                checked: current_value == "true",
                                                                onchange: move |e| {
                                                                    let val = if e.checked() { "true" } else { "false" };
                                                                    edits.write().insert(key.clone(), val.to_string());
                                                                },
                                                            }
                                                            span { class: "toggle-slider" }
                                                        }
                                                    } else if value_type == "integer" {
                                                        input {
                                                            class: "form-input setting-input",
                                                            r#type: "number",
                                                            value: "{current_value}",
                                                            oninput: move |e| {
                                                                edits.write().insert(key.clone(), e.value());
                                                            },
                                                        }
                                                    } else if is_secret(&key2) {
                                                        input {
                                                            class: "form-input setting-input",
                                                            r#type: "password",
                                                            placeholder: "Using built-in default",
                                                            value: "{current_value}",
                                                            autocomplete: "off",
                                                            oninput: move |e| {
                                                                edits.write().insert(key.clone(), e.value());
                                                            },
                                                        }
                                                    } else {
                                                        input {
                                                            class: "form-input setting-input",
                                                            r#type: "text",
                                                            value: "{current_value}",
                                                            oninput: move |e| {
                                                                edits.write().insert(key.clone(), e.value());
                                                            },
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
    }
}
