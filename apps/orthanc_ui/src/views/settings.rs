use dioxus::prelude::*;
use crate::{
    api::{self, ChangePasswordRequest, UpdateProfileRequest},
    state::AuthState,
};

#[derive(Clone, PartialEq)]
enum SettingsTab {
    Profile,
    Password,
}

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
                }
                // Content
                div { class: "settings-content",
                    match tab() {
                        SettingsTab::Profile => rsx! { ProfileSection {} },
                        SettingsTab::Password => rsx! { PasswordSection {} },
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
