use crate::Route;
use crate::components::bottom_nav::BottomNav;
use dioxus::prelude::*;
use orthanc_core::auth::{AuthState, with_refresh};
use orthanc_core::storage::{KEY_ACCESS_TOKEN, KEY_REFRESH_TOKEN, KEY_SERVER_URL, Storage};

/// Layout wrapper for tab routes (Home, Profile). Owns:
/// - Auth resume on mount: rehydrate tokens from storage, fetch the user.
/// - Server-config gate: if no server URL is set, redirect to /server-config.
/// - The bottom tab bar.
///
/// Routes that should be full-screen (Detail, Player) live outside this layout.
#[component]
pub fn MobileShell() -> Element {
    let mut auth = use_context::<Signal<AuthState>>();
    let storage = use_context::<Storage>();
    let nav = use_navigator();
    let mut checked = use_signal(|| false);

    use_effect(move || {
        // First, redirect to server-config if no URL is configured.
        if storage.get(KEY_SERVER_URL).is_none() {
            nav.replace(Route::ServerConfig {});
            checked.set(true);
            return;
        }

        // If we already have a user in memory, we're done.
        if auth.read().user.is_some() {
            checked.set(true);
            return;
        }

        // Try to rehydrate tokens from storage so a relaunch picks up the
        // session without forcing the user to log in again.
        let access = storage.get(KEY_ACCESS_TOKEN);
        let refresh = storage.get(KEY_REFRESH_TOKEN);
        match (access, refresh) {
            (Some(a), Some(r)) => {
                auth.write().access_token = Some(a);
                auth.write().refresh_token = Some(r);

                spawn(async move {
                    match with_refresh(auth, |token| async move {
                        orthanc_core::api::get_me(&token).await
                    })
                    .await
                    {
                        Ok(user) => {
                            auth.write().user = Some(user);
                        }
                        Err(_) => {
                            nav.replace(Route::Login {});
                        }
                    }
                    checked.set(true);
                });
            }
            _ => {
                nav.replace(Route::Login {});
                checked.set(true);
            }
        }
    });

    if !checked() {
        return rsx! { div { class: "fullscreen-loader", "Loading..." } };
    }

    rsx! {
        div { class: "mobile-shell",
            main { class: "shell-content",
                Outlet::<Route> {}
            }
            BottomNav {}
        }
    }
}
