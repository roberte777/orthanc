use dioxus::prelude::*;
use crate::api::{self, MediaItemResponse};
use crate::state::{AuthState, with_refresh};

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

fn format_runtime(seconds: Option<i32>) -> String {
    match seconds {
        Some(s) if s > 0 => {
            let h = s / 3600;
            let m = (s % 3600) / 60;
            if h > 0 { format!("{}h {}m", h, m) } else { format!("{}m", m) }
        }
        _ => String::new(),
    }
}

fn api_base() -> String {
    crate::api::API_BASE_URL.to_string()
}

#[component]
pub fn BrowseMovies() -> Element {
    let auth = use_context::<Signal<AuthState>>();
    let mut movies = use_signal(Vec::<MediaItemResponse>::new);
    let mut loading = use_signal(|| true);

    use_effect(move || {
        spawn(async move {
            if let Ok(m) = with_refresh(auth, |token| async move {
                api::get_all_movies(&token).await
            }).await {
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
    let id = item.id;
    let has_poster = item.poster_url.is_some();
    let poster_src = item.poster_url.clone().map(|p| format!("{}{}", api_base(), p));

    rsx! {
        Link {
            to: crate::Route::MovieDetail { id },
            class: "media-grid-card",
            div { class: "media-grid-card-poster",
                if let Some(ref src) = poster_src {
                    img { src: "{src}", class: "poster-img", alt: "{item.title}" }
                }
                if !has_poster {
                    div { class: "media-grid-card-overlay",
                        span { class: "media-grid-card-type", "MOVIE" }
                    }
                }
            }
            div { class: "media-grid-card-info",
                h3 { class: "media-grid-card-title", "{item.title}" }
                div { class: "media-grid-card-meta",
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

#[component]
pub fn MovieDetail(id: i64) -> Element {
    let auth = use_context::<Signal<AuthState>>();
    let is_admin = auth.read().is_admin();
    let mut movie = use_signal(|| Option::<MediaItemResponse>::None);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| Option::<String>::None);
    let mut refresh_msg = use_signal(|| Option::<String>::None);

    let mut load_count = use_signal(|| 0u32);

    let load_movie = move || {
        spawn(async move {
            match with_refresh(auth, |token| async move {
                api::get_movie(&token, id).await
            }).await {
                Ok(m) => {
                    movie.set(Some(m));
                    load_count += 1;
                }
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    };

    let mut initial_load = load_movie.clone();
    use_effect(move || { initial_load(); });

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
    let runtime = format_runtime(m.duration_seconds);
    let cb = load_count();
    let backdrop_src = m.backdrop_url.clone().map(|p| format!("{}{}?v={}", api_base(), p, cb));
    let poster_src = m.poster_url.clone().map(|p| format!("{}{}?v={}", api_base(), p, cb));

    rsx! {
        div { class: "detail-page",
            div { class: "detail-backdrop",
                if let Some(ref src) = backdrop_src {
                    img { src: "{src}", class: "detail-backdrop-img", alt: "" }
                }
            }
            div { class: "detail-content",
                if let Some(ref src) = poster_src {
                    img { src: "{src}", class: "detail-poster-img", alt: "{m.title}" }
                } else {
                    div { class: "detail-poster-placeholder",
                        span { "MOVIE" }
                    }
                }
                div { class: "detail-info",
                    h1 { class: "detail-title", "{m.title}" }
                    if let Some(ref tagline) = m.tagline {
                        p { class: "detail-tagline", "{tagline}" }
                    }
                    div { class: "detail-meta-row",
                        if !year.is_empty() {
                            span { class: "detail-meta-tag", "{year}" }
                        }
                        if let Some(ref cr) = m.content_rating {
                            span { class: "detail-meta-tag detail-rating-badge", "{cr}" }
                        }
                        if !runtime.is_empty() {
                            span { class: "detail-meta-tag", "{runtime}" }
                        }
                        if let Some(ref rating) = m.rating {
                            span { class: "detail-meta-tag detail-score", "{rating:.1}" }
                        }
                        if !fmt.is_empty() {
                            span { class: "detail-meta-tag", "{fmt}" }
                        }
                        if !size.is_empty() {
                            span { class: "detail-meta-tag", "{size}" }
                        }
                    }
                    if let Some(ref genres) = m.genres {
                        div { class: "detail-genres",
                            for g in genres {
                                span { class: "genre-tag", "{g}" }
                            }
                        }
                    }
                    if let Some(ref desc) = m.description {
                        p { class: "detail-description", "{desc}" }
                    }
                    div { class: "detail-actions",
                        button { class: "btn-play", disabled: true, "Play" }
                    }
                    if is_admin {
                        div { class: "detail-admin-actions",
                            {
                                let mut reload = load_movie.clone();
                                rsx! {
                                    button {
                                        class: "btn-admin-refresh",
                                        onclick: move |_| {
                                            let token = auth.read().access_token.clone().unwrap_or_default();
                                            let mut reload2 = reload.clone();
                                            refresh_msg.set(Some("Refreshing...".to_string()));
                                            spawn(async move {
                                                match api::refresh_metadata(&token, id, "standard").await {
                                                    Ok(_) => {
                                                        refresh_msg.set(Some("Metadata refreshed".to_string()));
                                                        reload2();
                                                    }
                                                    Err(e) => refresh_msg.set(Some(format!("Error: {}", e))),
                                                }
                                            });
                                        },
                                        "Refresh Metadata"
                                    }
                                    button {
                                        class: "btn-admin-refresh",
                                        onclick: move |_| {
                                            let token = auth.read().access_token.clone().unwrap_or_default();
                                            let mut reload2 = reload.clone();
                                            refresh_msg.set(Some("Full refresh...".to_string()));
                                            spawn(async move {
                                                match api::refresh_metadata(&token, id, "full").await {
                                                    Ok(_) => {
                                                        refresh_msg.set(Some("Full refresh done".to_string()));
                                                        reload2();
                                                    }
                                                    Err(e) => refresh_msg.set(Some(format!("Error: {}", e))),
                                                }
                                            });
                                        },
                                        "Replace All Metadata"
                                    }
                                    if let Some(msg) = refresh_msg() {
                                        span { class: "refresh-status", "{msg}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
