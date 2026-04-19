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
            let m = s / 60;
            format!("{}m", m)
        }
        _ => String::new(),
    }
}

fn count_episodes(show: &MediaItemResponse) -> usize {
    show.children
        .as_ref()
        .map(|seasons| {
            seasons
                .iter()
                .map(|s| s.children.as_ref().map(|eps| eps.len()).unwrap_or(0))
                .sum()
        })
        .unwrap_or(0)
}

fn api_base() -> String {
    crate::api::API_BASE_URL.to_string()
}

#[component]
pub fn BrowseShows() -> Element {
    let auth = use_context::<Signal<AuthState>>();
    let mut shows = use_signal(Vec::<MediaItemResponse>::new);
    let mut loading = use_signal(|| true);

    use_effect(move || {
        spawn(async move {
            if let Ok(s) = with_refresh(auth, |token| async move {
                api::get_all_shows(&token).await
            }).await {
                shows.set(s);
            }
            loading.set(false);
        });
    });

    rsx! {
        div { class: "page browse-page",
            h1 { class: "page-title", "TV Shows" }

            if loading() {
                div { class: "loading", "Loading..." }
            } else if shows().is_empty() {
                div { class: "empty-state",
                    h2 { "No TV shows found" }
                    p { "Scan your TV show libraries to discover media." }
                }
            } else {
                div { class: "media-grid",
                    for show in shows() {
                        ShowGridCard { item: show }
                    }
                }
            }
        }
    }
}

#[component]
fn ShowGridCard(item: MediaItemResponse) -> Element {
    let year = format_year(&item.release_date);
    let id = item.id;
    let has_poster = item.poster_url.is_some();
    let poster_src = item.poster_url.clone().map(|p| format!("{}{}", api_base(), p));

    rsx! {
        Link {
            to: crate::Route::ShowDetail { id },
            class: "media-grid-card",
            div { class: if has_poster { "media-grid-card-poster" } else { "media-grid-card-poster show-poster" },
                if let Some(ref src) = poster_src {
                    img { src: "{src}", class: "poster-img", alt: "{item.title}" }
                }
                if !has_poster {
                    div { class: "media-grid-card-overlay",
                        span { class: "media-grid-card-type", "TV" }
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
pub fn ShowDetail(id: i64) -> Element {
    let auth = use_context::<Signal<AuthState>>();
    let is_admin = auth.read().is_admin();
    let mut show = use_signal(|| Option::<MediaItemResponse>::None);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| Option::<String>::None);
    let mut active_season = use_signal(|| 0usize);
    let mut refresh_msg = use_signal(|| Option::<String>::None);

    let mut cache_bust = use_signal(|| 0u32);

    let mut load_count = use_signal(|| 0u32);

    let load_show = move || {
        spawn(async move {
            match with_refresh(auth, |token| async move {
                api::get_show(&token, id).await
            }).await {
                Ok(s) => {
                    show.set(Some(s));
                    load_count += 1;
                }
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    };

    let mut initial_load = load_show.clone();
    use_effect(move || { initial_load(); });

    if loading() {
        return rsx! { div { class: "page", div { class: "loading", "Loading..." } } };
    }
    if let Some(err) = error() {
        return rsx! { div { class: "page", div { class: "error-msg", "{err}" } } };
    }
    let s = match show() {
        Some(s) => s,
        None => return rsx! { div { class: "page", "Show not found" } },
    };

    let year = format_year(&s.release_date);
    let seasons = s.children.clone().unwrap_or_default();
    let season_count = seasons.len();
    let episode_count = count_episodes(&s);
    let cb = load_count();
    let backdrop_src = s.backdrop_url.clone().map(|p| format!("{}{}?v={}", api_base(), p, cb));
    let poster_src = s.poster_url.clone().map(|p| format!("{}{}?v={}", api_base(), p, cb));

    let current_season = seasons.get(active_season()).cloned();
    let episodes = current_season
        .and_then(|s| s.children)
        .unwrap_or_default();

    rsx! {
        div { class: "detail-page",
            div { class: "detail-backdrop",
                if let Some(ref src) = backdrop_src {
                    img { src: "{src}", class: "detail-backdrop-img", alt: "" }
                }
            }
            div { class: "detail-content",
                if let Some(ref src) = poster_src {
                    img { src: "{src}", class: "detail-poster-img", alt: "{s.title}" }
                } else {
                    div { class: "detail-poster-placeholder show-poster-detail",
                        span { "TV" }
                    }
                }
                div { class: "detail-info",
                    h1 { class: "detail-title", "{s.title}" }
                    if let Some(ref tagline) = s.tagline {
                        p { class: "detail-tagline", "{tagline}" }
                    }
                    div { class: "detail-meta-row",
                        if !year.is_empty() {
                            span { class: "detail-meta-tag", "{year}" }
                        }
                        if let Some(ref cr) = s.content_rating {
                            span { class: "detail-meta-tag detail-rating-badge", "{cr}" }
                        }
                        if let Some(ref rating) = s.rating {
                            span { class: "detail-meta-tag detail-score", "{rating:.1}" }
                        }
                        {
                            let label = if season_count != 1 { "Seasons" } else { "Season" };
                            rsx! { span { class: "detail-meta-tag", "{season_count} {label}" } }
                        }
                        {
                            let label = if episode_count != 1 { "Episodes" } else { "Episode" };
                            rsx! { span { class: "detail-meta-tag", "{episode_count} {label}" } }
                        }
                    }
                    if let Some(ref genres) = s.genres {
                        div { class: "detail-genres",
                            for g in genres {
                                span { class: "genre-tag", "{g}" }
                            }
                        }
                    }
                    if let Some(ref desc) = s.description {
                        p { class: "detail-description", "{desc}" }
                    }
                    div { class: "detail-actions",
                        {
                            let nav = use_navigator();
                            let first_ep_id = seasons.first()
                                .and_then(|s| s.children.as_ref())
                                .and_then(|eps| eps.first())
                                .map(|ep| ep.id);
                            rsx! {
                                if let Some(ep_id) = first_ep_id {
                                    button {
                                        class: "btn-play",
                                        onclick: move |_| {
                                            nav.push(crate::Route::Player { id: ep_id });
                                        },
                                        "Play"
                                    }
                                } else {
                                    button { class: "btn-play", disabled: true, "Play" }
                                }
                            }
                        }
                    }
                    if is_admin {
                        div { class: "detail-admin-actions",
                            {
                                let mut reload = load_show.clone();
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

            // Season tabs + episode list
            if !seasons.is_empty() {
                div { class: "show-seasons",
                    div { class: "season-tabs",
                        for (idx, season) in seasons.iter().enumerate() {
                            {
                                let season_title = season.title.clone();
                                rsx! {
                                    button {
                                        class: if idx == active_season() { "season-tab active" } else { "season-tab" },
                                        onclick: move |_| active_season.set(idx),
                                        "{season_title}"
                                    }
                                }
                            }
                        }
                    }
                    div { class: "episode-list",
                        for ep in &episodes {
                            EpisodeRow { episode: ep.clone(), cache_bust: cb }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn EpisodeRow(episode: MediaItemResponse, cache_bust: u32) -> Element {
    let ep_id = episode.id;
    let ep_num = episode.episode_number.unwrap_or(0);
    let runtime = format_runtime(episode.duration_seconds);
    let thumb_src = episode.backdrop_url.clone().map(|p| format!("{}{}?v={}", api_base(), p, cache_bust));
    let has_thumb = thumb_src.is_some();
    let nav = use_navigator();

    rsx! {
        div {
            class: "ep-card",
            onclick: move |_| {
                nav.push(crate::Route::Player { id: ep_id });
            },
            // Left: number
            div { class: "ep-card-num", "{ep_num}" }
            // Thumbnail
            div { class: "ep-card-thumb",
                if let Some(ref src) = thumb_src {
                    img { src: "{src}", class: "ep-card-thumb-img", alt: "{episode.title}" }
                } else {
                    div { class: "ep-card-thumb-placeholder",
                        span { class: "ep-card-thumb-ep", "E{ep_num}" }
                    }
                }
                div { class: "ep-card-play-overlay",
                    div { class: "ep-card-play-icon" }
                }
            }
            // Info
            div { class: "ep-card-body",
                div { class: "ep-card-header",
                    h4 { class: "ep-card-title", "{episode.title}" }
                    if !runtime.is_empty() {
                        span { class: "ep-card-runtime", "{runtime}" }
                    }
                }
                if let Some(ref desc) = episode.description {
                    p { class: "ep-card-desc", "{desc}" }
                }
            }
        }
    }
}
