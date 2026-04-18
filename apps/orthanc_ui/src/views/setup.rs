use dioxus::prelude::*;
use crate::{
    api::{self, SetupRequest},
    state::{AuthState, save_auth},
    Route,
};

#[component]
pub fn Setup() -> Element {
    let mut auth = use_context::<Signal<AuthState>>();
    let nav = use_navigator();

    let mut username = use_signal(String::new);
    let mut email = use_signal(String::new);
    let mut display_name = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut confirm_password = use_signal(String::new);
    let mut error = use_signal(|| Option::<String>::None);
    let mut loading = use_signal(|| false);

    let on_submit = move |evt: Event<FormData>| {
        evt.prevent_default();
        if loading() {
            return;
        }

        let pass = password();
        let confirm = confirm_password();

        if pass != confirm {
            error.set(Some("Passwords do not match".to_string()));
            return;
        }
        if pass.len() < 8 {
            error.set(Some(
                "Password must be at least 8 characters".to_string(),
            ));
            return;
        }

        loading.set(true);
        error.set(None);

        let dn = display_name();
        spawn(async move {
            let req = SetupRequest {
                username: username(),
                email: email(),
                password: pass,
                display_name: if dn.is_empty() { None } else { Some(dn) },
            };

            match api::setup(req).await {
                Ok(resp) => {
                    save_auth(&resp.access_token, &resp.refresh_token);
                    auth.write().access_token = Some(resp.access_token);
                    auth.write().refresh_token = Some(resp.refresh_token);
                    auth.write().user = Some(resp.user);
                    nav.replace(Route::Home {});
                }
                Err(e) => {
                    if e.contains("409") || e.contains("Conflict") {
                        nav.replace(Route::Login {});
                    } else {
                        error.set(Some(format!("Setup failed: {}", e)));
                        loading.set(false);
                    }
                }
            }
        });
    };

    rsx! {
        div { class: "auth-page",
            div { class: "auth-card auth-card-wide",
                div { class: "auth-logo", "ORTHANC" }
                h2 { class: "auth-title", "Welcome to Orthanc" }
                p { class: "auth-subtitle", "Create your admin account to get started" }

                if let Some(err) = error() {
                    div { class: "error-msg", "{err}" }
                }

                form { onsubmit: on_submit,
                    div { class: "form-row",
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
                                r#type: "text",
                                placeholder: "Display Name (optional)",
                                value: "{display_name}",
                                oninput: move |e| display_name.set(e.value()),
                            }
                        }
                    }
                    div { class: "form-group",
                        input {
                            class: "form-input",
                            r#type: "email",
                            placeholder: "Email",
                            value: "{email}",
                            oninput: move |e| email.set(e.value()),
                            required: true,
                        }
                    }
                    div { class: "form-row",
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
                        div { class: "form-group",
                            input {
                                class: "form-input",
                                r#type: "password",
                                placeholder: "Confirm Password",
                                value: "{confirm_password}",
                                oninput: move |e| confirm_password.set(e.value()),
                                required: true,
                            }
                        }
                    }
                    button {
                        class: "btn btn-primary btn-full",
                        r#type: "submit",
                        disabled: loading(),
                        if loading() { "Creating account..." } else { "Create Admin Account" }
                    }
                }
            }
        }
    }
}
