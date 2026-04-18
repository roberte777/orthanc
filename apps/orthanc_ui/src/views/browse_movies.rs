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
pub fn BrowseMovies() -> Element {
    let auth = use_context::<Signal<AuthState>>();
    let mut movies = use_signal(Vec::<MediaItemResponse>::new);
    let mut loading = use_signal(|| true);

    use_effect(move || {
        let token = auth.read().access_token.clone().unwrap_or_default();
        spawn(async move {
            if let Ok(m) = api::get_all_movies(&token).await {
                movies.set(m);
            }
            loading.set(false);
        });
    });

    rsx! {
        div { class: "page browse-page",
            h1 { class: "page-title", "Movies" }

            if loading() {
                div { class: "loading", "Loading..." }
            } else if movies().is_empty() {
                div { class: "empty-state",
                    h2 { "No movies found" }
                    p { "Scan your movie libraries to discover media." }
                }
            } else {
                div { class: "media-grid",
                    for movie in movies() {
                        MovieGridCard { item: movie }
                    }
                }
            }
        }
    }
}

#[component]
fn MovieGridCard(item: MediaItemResponse) -> Element {
    let year = format_year(&item.release_date);
    let size = format_size(item.file_size_bytes);
    let fmt = item.container_format.clone().unwrap_or_default().to_uppercase();
    let id = item.id;

    rsx! {
        Link {
            to: crate::Route::MovieDetail { id },
            class: "media-grid-card",
            div { class: "media-grid-card-poster",
                div { class: "media-grid-card-overlay",
                    span { class: "media-grid-card-type", "MOVIE" }
                }
            }
            div { class: "media-grid-card-info",
                h3 { class: "media-grid-card-title", "{item.title}" }
                div { class: "media-grid-card-meta",
                    if !year.is_empty() {
                        span { "{year}" }
                    }
                    if !fmt.is_empty() {
                        span { "{fmt}" }
                    }
                    if !size.is_empty() {
                        span { "{size}" }
                    }
                }
            }
        }
    }
}

#[component]
pub fn MovieDetail(id: i64) -> Element {
    let auth = use_context::<Signal<AuthState>>();
    let mut movie = use_signal(|| Option::<MediaItemResponse>::None);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| Option::<String>::None);

    use_effect(move || {
        let token = auth.read().access_token.clone().unwrap_or_default();
        spawn(async move {
            match api::get_movie(&token, id).await {
                Ok(m) => movie.set(Some(m)),
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    });

    if loading() {
        return rsx! { div { class: "page", div { class: "loading", "Loading..." } } };
    }

    if let Some(err) = error() {
        return rsx! { div { class: "page", div { class: "error-msg", "{err}" } } };
    }

    let m = match movie() {
        Some(m) => m,
        None => return rsx! { div { class: "page", "Movie not found" } },
    };

    let year = format_year(&m.release_date);
    let size = format_size(m.file_size_bytes);
    let fmt = m.container_format.clone().unwrap_or_default().to_uppercase();
    let file_path = m.file_path.clone().unwrap_or_default();

    rsx! {
        div { class: "detail-page",
            div { class: "detail-backdrop" }
            div { class: "detail-content",
                div { class: "detail-poster-placeholder",
                    span { "MOVIE" }
                }
                div { class: "detail-info",
                    h1 { class: "detail-title", "{m.title}" }
                    div { class: "detail-meta-row",
                        if !year.is_empty() {
                            span { class: "detail-meta-tag", "{year}" }
                        }
                        if !fmt.is_empty() {
                            span { class: "detail-meta-tag", "{fmt}" }
                        }
                        if !size.is_empty() {
                            span { class: "detail-meta-tag", "{size}" }
                        }
                    }
                    div { class: "detail-actions",
                        button { class: "btn-play", disabled: true, "Play" }
                    }
                    if !file_path.is_empty() {
                        div { class: "detail-file-path",
                            span { class: "detail-label", "File" }
                            span { class: "detail-value mono", "{file_path}" }
                        }
                    }
                }
            }
        }
    }
}
