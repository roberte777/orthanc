use crate::Route;
use dioxus::prelude::*;
use orthanc_core::api;
use orthanc_core::storage::{KEY_SERVER_URL, Storage};

/// One-time server URL entry. Persists the URL via Storage so the next launch
/// skips this screen. Pings /api/setup-status as the connectivity check
/// because that endpoint is unauthenticated and always present.
#[component]
pub fn ServerConfig() -> Element {
    let storage = use_context::<Storage>();
    let nav = use_navigator();

    let initial = storage
        .get(KEY_SERVER_URL)
        .unwrap_or_else(|| "http://".to_string());
    let mut url = use_signal(|| initial);
    let mut error = use_signal(|| None::<String>);
    let mut testing = use_signal(|| false);

    let on_submit = move |evt: Event<FormData>| {
        evt.prevent_default();
        if testing() {
            return;
        }
        testing.set(true);
        error.set(None);

        let candidate = url().trim().trim_end_matches('/').to_string();
        if !candidate.starts_with("http://") && !candidate.starts_with("https://") {
            error.set(Some("URL must start with http:// or https://".into()));
            testing.set(false);
            return;
        }

        let storage = storage.clone();
        spawn(async move {
            // Probe the candidate URL by hitting an unauthenticated endpoint.
            // We temporarily set the base URL so api::get_setup_status uses it.
            api::set_base_url(candidate.clone());
            match api::get_setup_status().await {
                Ok(_) => {
                    storage.set(KEY_SERVER_URL, &candidate);
                    nav.replace(Route::Login {});
                }
                Err(e) => {
                    error.set(Some(format!("Could not reach server: {}", e)));
                    testing.set(false);
                }
            }
        });
    };

    rsx! {
        div { class: "auth-page",
            div { class: "auth-card",
                div { class: "auth-logo", "ORTHANC" }
                h2 { class: "auth-title", "Server" }
                p { class: "auth-subtitle",
                    "Enter the address of your Orthanc server. Example: "
                    code { "http://192.168.1.100:8081" }
                }

                if let Some(err) = error() {
                    div { class: "error-msg", "{err}" }
                }

                form { onsubmit: on_submit,
                    div { class: "form-group",
                        input {
                            class: "form-input",
                            r#type: "url",
                            inputmode: "url",
                            autocapitalize: "off",
                            autocorrect: "off",
                            placeholder: "http://192.168.1.100:8081",
                            value: "{url}",
                            oninput: move |e| url.set(e.value()),
                            required: true,
                        }
                    }
                    button {
                        class: "btn btn-primary btn-full",
                        r#type: "submit",
                        disabled: testing(),
                        if testing() { "Testing connection..." } else { "Continue" }
                    }
                }
            }
        }
    }
}
