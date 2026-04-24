use crate::Route;
use dioxus::prelude::*;
use orthanc_core::api::{self, LoginRequest};
use orthanc_core::auth::{AuthState, save_auth};
use orthanc_core::storage::{KEY_SERVER_URL, Storage};

/// Mobile-optimised login screen. Slimmer than the web equivalent: no setup
/// flow (mobile users don't bootstrap servers from their phone), and a "Switch
/// server" link to send the user back to /server-config if they typed the
/// wrong URL.
#[component]
pub fn Login() -> Element {
    let mut auth = use_context::<Signal<AuthState>>();
    let storage = use_context::<Storage>();
    let nav = use_navigator();

    if auth.read().logged_in() {
        nav.replace(Route::Home {});
    }

    // Defensive: if the user navigates here directly without configuring a
    // server, send them to /server-config first.
    if storage.get(KEY_SERVER_URL).is_none() {
        nav.replace(Route::ServerConfig {});
    }

    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut loading = use_signal(|| false);

    let on_submit = move |evt: Event<FormData>| {
        evt.prevent_default();
        if loading() {
            return;
        }
        loading.set(true);
        error.set(None);

        let uname = username();
        let pass = password();
        let storage = storage.clone();
        spawn(async move {
            match api::login(LoginRequest {
                username: uname,
                password: pass,
            })
            .await
            {
                Ok(resp) => {
                    save_auth(&storage, &resp.access_token, &resp.refresh_token);
                    auth.write().access_token = Some(resp.access_token);
                    auth.write().refresh_token = Some(resp.refresh_token);
                    auth.write().user = Some(resp.user);
                    nav.replace(Route::Home {});
                }
                Err(e) => {
                    let msg = if e.contains("401") {
                        "Invalid username or password.".to_string()
                    } else {
                        format!("Could not sign in: {}", e)
                    };
                    error.set(Some(msg));
                    loading.set(false);
                }
            }
        });
    };

    rsx! {
        div { class: "auth-page",
            div { class: "auth-card",
                div { class: "auth-logo", "ORTHANC" }
                h2 { class: "auth-title", "Sign In" }

                if let Some(err) = error() {
                    div { class: "error-msg", "{err}" }
                }

                form { onsubmit: on_submit,
                    div { class: "form-group",
                        input {
                            class: "form-input",
                            r#type: "text",
                            inputmode: "email",
                            autocapitalize: "off",
                            autocorrect: "off",
                            placeholder: "Username",
                            value: "{username}",
                            oninput: move |e| username.set(e.value()),
                            required: true,
                        }
                    }
                    div { class: "form-group",
                        input {
                            class: "form-input",
                            r#type: "password",
                            placeholder: "Password",
                            value: "{password}",
                            oninput: move |e| password.set(e.value()),
                            required: true,
                        }
                    }
                    button {
                        class: "btn btn-primary btn-full",
                        r#type: "submit",
                        disabled: loading(),
                        if loading() { "Signing in..." } else { "Sign In" }
                    }
                }

                div { class: "auth-secondary",
                    Link { to: Route::ServerConfig {}, class: "secondary-link",
                        "Switch server"
                    }
                }
            }
        }
    }
}
