use crate::Route;
use dioxus::prelude::*;
use orthanc_core::api;
use orthanc_core::auth::{AuthState, clear_auth};
use orthanc_core::storage::{KEY_SERVER_URL, Storage};

#[component]
pub fn Profile() -> Element {
    let mut auth = use_context::<Signal<AuthState>>();
    let storage = use_context::<Storage>();
    let nav = use_navigator();

    let username = auth
        .read()
        .user
        .as_ref()
        .map(|u| u.display_name.clone().unwrap_or_else(|| u.username.clone()))
        .unwrap_or_default();

    let email = auth
        .read()
        .user
        .as_ref()
        .map(|u| u.email.clone())
        .unwrap_or_default();

    let logout_storage = storage.clone();
    let on_logout = move |_| {
        let storage = logout_storage.clone();
        let access = auth.read().access_token.clone().unwrap_or_default();
        let refresh = auth.read().refresh_token.clone().unwrap_or_default();
        spawn(async move {
            let _ = api::logout(&access, &refresh).await;
            clear_auth(&storage);
            auth.write().access_token = None;
            auth.write().refresh_token = None;
            auth.write().user = None;
        });
        nav.replace(Route::Login {});
    };

    let switch_storage = storage;
    let on_switch_server = move |_| {
        // Clear server selection AND tokens — switching servers invalidates
        // the current session unconditionally.
        switch_storage.remove(KEY_SERVER_URL);
        clear_auth(&switch_storage);
        auth.write().access_token = None;
        auth.write().refresh_token = None;
        auth.write().user = None;
        nav.replace(Route::ServerConfig {});
    };

    rsx! {
        div { class: "page profile-page",
            h1 { class: "page-title", "Profile" }
            div { class: "profile-card",
                div { class: "profile-name", "{username}" }
                if !email.is_empty() {
                    div { class: "profile-email", "{email}" }
                }
            }
            div { class: "profile-actions",
                button {
                    class: "btn btn-secondary btn-full",
                    onclick: on_switch_server,
                    "Switch server"
                }
                button {
                    class: "btn btn-danger btn-full",
                    onclick: on_logout,
                    "Sign out"
                }
            }
        }
    }
}
