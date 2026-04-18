use dioxus::prelude::*;
use crate::{
    Route,
    state::{AuthState, clear_auth},
};

#[component]
pub fn AppShell() -> Element {
    let mut auth = use_context::<Signal<AuthState>>();
    let nav = use_navigator();
    let mut checked = use_signal(|| false);

    // On mount: if we have a token but no user, try to fetch the user
    use_effect(move || {
        let has_token = auth.read().access_token.is_some();
        let has_user = auth.read().user.is_some();

        if has_token && !has_user {
            let token = auth.read().access_token.clone().unwrap_or_default();
            spawn(async move {
                match crate::api::get_me(&token).await {
                    Ok(user) => {
                        auth.write().user = Some(user);
                    }
                    Err(_) => {
                        // Token is invalid, clear and redirect
                        clear_auth();
                        auth.write().access_token = None;
                        auth.write().refresh_token = None;
                        nav.replace(Route::Login {});
                    }
                }
                checked.set(true);
            });
        } else if !has_token {
            nav.replace(Route::Login {});
            checked.set(true);
        } else {
            checked.set(true);
        }
    });

    // Don't render until auth check is complete
    if !checked() {
        return rsx! { div { class: "loading", "Loading..." } };
    }

    let user_display = auth
        .read()
        .user
        .as_ref()
        .map(|u| u.display_name.clone().unwrap_or_else(|| u.username.clone()))
        .unwrap_or_default();

    let is_admin = auth.read().is_admin();

    let initials = user_display
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());

    let mut show_dropdown = use_signal(|| false);

    rsx! {
        div { class: "app-shell",
            // Top navbar
            nav { class: "navbar",
                // Left: Logo + Nav links
                div { class: "navbar-left",
                    div { class: "navbar-logo",
                        Link { to: Route::Home {}, "ORTHANC" }
                    }
                    div { class: "navbar-links",
                        Link { to: Route::Home {}, class: "nav-link", "Home" }
                        Link { to: Route::BrowseMovies {}, class: "nav-link", "Movies" }
                        Link { to: Route::BrowseShows {}, class: "nav-link", "TV Shows" }
                        if is_admin {
                            Link { to: Route::AdminLibraries {}, class: "nav-link", "Libraries" }
                            Link { to: Route::AdminUsers {}, class: "nav-link", "Users" }
                            Link { to: Route::AdminSettings {}, class: "nav-link", "Settings" }
                        }
                    }
                }
                // User avatar and dropdown
                div { class: "navbar-user",
                    div {
                        class: "avatar",
                        onclick: move |_| show_dropdown.toggle(),
                        "{initials}"
                    }
                    if show_dropdown() {
                        div { class: "dropdown",
                            Link {
                                to: Route::Settings {},
                                class: "dropdown-item",
                                onclick: move |_| show_dropdown.set(false),
                                "Profile Settings"
                            }
                            div {
                                class: "dropdown-item dropdown-danger",
                                onclick: move |_| {
                                    show_dropdown.set(false);
                                    let refresh_token = auth
                                        .read()
                                        .refresh_token
                                        .clone()
                                        .unwrap_or_default();
                                    let access_token = auth
                                        .read()
                                        .access_token
                                        .clone()
                                        .unwrap_or_default();
                                    spawn(async move {
                                        let _ = crate::api::logout(&access_token, &refresh_token)
                                            .await;
                                        clear_auth();
                                        auth.write().access_token = None;
                                        auth.write().refresh_token = None;
                                        auth.write().user = None;
                                    });
                                    nav.replace(Route::Login {});
                                },
                                "Sign Out"
                            }
                        }
                    }
                }
            }
            // Page content
            main { class: "content",
                Outlet::<Route> {}
            }
        }
    }
}
