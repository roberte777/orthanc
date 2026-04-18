use dioxus::prelude::*;
use crate::api::{self, MediaItemResponse};
use crate::state::AuthState;

fn format_size(bytes: Option<i64>) -> String {
    match bytes {
        Some(b) if b >= 1_073_741_824 => format!("{:.1} GB", b as f64 / 1_073_741_824.0),
        Some(b) if b >= 1_048_576 => format!("{:.0} MB", b as f64 / 1_048_576.0),
        Some(b) => format!("{} KB", b / 1024),
        None => String::new(),
    }
}

fn format_year(release_date: &Option<String>) -> String {
    release_date
        .as_ref()
        .and_then(|d| d.get(..4))
        .unwrap_or("")
        .to_string()
}

#[component]
pub fn Home() -> Element {
    let auth = use_context::<Signal<AuthState>>();
    let username = auth
        .read()
        .user
        .as_ref()
        .map(|u| {
            u.display_name
                .clone()
                .unwrap_or_else(|| u.username.clone())
        })
        .unwrap_or_default();

    let mut movies = use_signal(Vec::<MediaItemResponse>::new);
    let mut shows = use_signal(Vec::<MediaItemResponse>::new);
    let mut loaded = use_signal(|| false);

    use_effect(move || {
        let token = auth.read().access_token.clone().unwrap_or_default();
        spawn(async move {
            if let Ok(recent) = api::get_recent_media(&token).await {
                movies.set(recent.movies);
                shows.set(recent.shows);
            }
            loaded.set(true);
        });
    });

    let has_media = !movies().is_empty() || !shows().is_empty();

    rsx! {
        div { class: "home-full",
            // Hero banner
            div { class: "hero-banner",
                div { class: "hero-content",
                    div { class: "hero-tag",
                        span { class: "hero-tag-icon", "O" }
                        "orthanc"
                    }
                    h1 { class: "hero-title", "Welcome, {username}" }
                    p { class: "hero-description",
                        if has_media {
                            "Your personal streaming server"
                        } else {
                            "Add media libraries and scan them to start watching."
                        }
                    }
                    div { class: "hero-actions",
                        if auth.read().is_admin() {
                            Link {
                                to: crate::Route::AdminLibraries {},
                                class: "btn-play",
                                if has_media { "Manage Libraries" } else { "Setup Libraries" }
                            }
                        }
                    }
                }
            }

            // Content rows
            div { class: "content-rows",
                if !loaded() {
                    div { class: "loading", "Loading media..." }
                } else if !has_media {
                    div { class: "empty-state",
                        h2 { "No media found" }
                        p { "Add paths to your libraries and run a scan to discover your media files." }
                    }
                } else {
                    // Movies row
                    if !movies().is_empty() {
                        div { class: "content-row",
                            div { class: "row-header",
                                h2 { class: "row-title", "Movies" }
                                Link { to: crate::Route::BrowseMovies {}, class: "row-see-all", "See All" }
                            }
                            div { class: "row-cards",
                                for movie in movies() {
                                    MediaCard { item: movie }
                                }
                            }
                        }
                    }
                    // TV Shows row
                    if !shows().is_empty() {
                        div { class: "content-row",
                            div { class: "row-header",
                                h2 { class: "row-title", "TV Shows" }
                                Link { to: crate::Route::BrowseShows {}, class: "row-see-all", "See All" }
                            }
                            div { class: "row-cards",
                                for show in shows() {
                                    MediaCard { item: show }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn MediaCard(item: MediaItemResponse) -> Element {
    let year = format_year(&item.release_date);
    let is_show = item.media_type == "tv_show";
    let id = item.id;
    let has_poster = item.poster_url.is_some();
    let poster_src = item.poster_url.clone().map(|p| format!("{}{}", crate::api::API_BASE_URL, p));

    let route = if is_show {
        crate::Route::ShowDetail { id }
    } else {
        crate::Route::MovieDetail { id }
    };

    rsx! {
        Link { to: route, class: "media-card", key: "{item.id}",
            if let Some(ref src) = poster_src {
                img { src: "{src}", class: "poster-img", alt: "{item.title}" }
            }
            if !has_poster {
                div { class: "media-card-content",
                    div { class: "media-card-type",
                        if is_show { "TV" } else { "MOVIE" }
                    }
                    h3 { class: "media-card-title", "{item.title}" }
                    div { class: "media-card-meta",
                        if !year.is_empty() {
                            span { "{year}" }
                        }
                        if let Some(ref rating) = item.rating {
                            span { "{rating:.1}" }
                        }
                    }
                }
            }
        }
    }
}
