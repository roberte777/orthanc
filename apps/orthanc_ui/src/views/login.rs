use crate::{
    Route,
    api::{self, LoginRequest},
    state::{AuthState, save_auth},
};
use dioxus::prelude::*;

#[component]
pub fn Login() -> Element {
    let mut auth = use_context::<Signal<AuthState>>();
    let nav = use_navigator();

    // If already logged in, redirect
    if auth.read().logged_in() {
        nav.replace(Route::Home {});
    }

    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut error = use_signal(|| Option::<String>::None);
    let mut loading = use_signal(|| false);
    let mut server_unreachable = use_signal(|| false);

    // Check setup status on mount
    use_effect(move || {
        spawn(async move {
            match api::get_setup_status().await {
                Ok(status) => {
                    if status.needs_setup {
                        nav.replace(Route::Setup {});
                    }
                }
                Err(_) => {
                    server_unreachable.set(true);
                }
            }
        });
    });

    let on_submit = move |evt: Event<FormData>| {
        evt.prevent_default();
        if loading() {
            return;
        }
        loading.set(true);
        error.set(None);

        let uname = username();
        let pass = password();
        spawn(async move {
            match api::login(LoginRequest {
                username: uname,
                password: pass,
            })
            .await
            {
                Ok(resp) => {
                    save_auth(&resp.access_token, &resp.refresh_token);
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

                if server_unreachable() {
                    div { class: "error-msg",
                        "Cannot reach the server. Make sure Orthanc is running."
                    }
                }

                if let Some(err) = error() {
                    div { class: "error-msg", "{err}" }
                }

                form { onsubmit: on_submit,
                    fieldset {
                        disabled: server_unreachable(),
                        style: "border: none; padding: 0; margin: 0;",
                        div { class: "form-group",
                            input {
                                class: "form-input",
                                r#type: "text",
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
                }

            }
        }
    }
}
