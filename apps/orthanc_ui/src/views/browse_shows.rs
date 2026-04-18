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

#[component]
pub fn BrowseShows() -> Element {
    let auth = use_context::<Signal<AuthState>>();
    let mut shows = use_signal(Vec::<MediaItemResponse>::new);
    let mut loading = use_signal(|| true);

    use_effect(move || {
        let token = auth.read().access_token.clone().unwrap_or_default();
        spawn(async move {
            if let Ok(s) = api::get_all_shows(&token).await {
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

    rsx! {
        Link {
            to: crate::Route::ShowDetail { id },
            class: "media-grid-card",
            div { class: "media-grid-card-poster show-poster",
                div { class: "media-grid-card-overlay",
                    span { class: "media-grid-card-type", "TV" }
                }
            }
            div { class: "media-grid-card-info",
                h3 { class: "media-grid-card-title", "{item.title}" }
                div { class: "media-grid-card-meta",
                    if !year.is_empty() {
                        span { "{year}" }
                    }
                }
            }
        }
    }
}

#[component]
pub fn ShowDetail(id: i64) -> Element {
    let auth = use_context::<Signal<AuthState>>();
    let mut show = use_signal(|| Option::<MediaItemResponse>::None);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| Option::<String>::None);
    let mut active_season = use_signal(|| 0usize);

    use_effect(move || {
        let token = auth.read().access_token.clone().unwrap_or_default();
        spawn(async move {
            match api::get_show(&token, id).await {
                Ok(s) => show.set(Some(s)),
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

    let s = match show() {
        Some(s) => s,
        None => return rsx! { div { class: "page", "Show not found" } },
    };

    let year = format_year(&s.release_date);
    let seasons = s.children.clone().unwrap_or_default();
    let season_count = seasons.len();
    let episode_count = count_episodes(&s);

    let current_season = seasons.get(active_season()).cloned();
    let episodes = current_season
        .and_then(|s| s.children)
        .unwrap_or_default();

    rsx! {
        div { class: "detail-page",
            div { class: "detail-backdrop" }
            div { class: "detail-content",
                div { class: "detail-poster-placeholder show-poster-detail",
                    span { "TV" }
                }
                div { class: "detail-info",
                    h1 { class: "detail-title", "{s.title}" }
                    div { class: "detail-meta-row",
                        if !year.is_empty() {
                            span { class: "detail-meta-tag", "{year}" }
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
                }
            }

            // Season tabs + episode list
            if !seasons.is_empty() {
                div { class: "show-seasons",
                    // Season tabs
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

                    // Episodes list
                    div { class: "episode-list",
                        for ep in &episodes {
                            EpisodeRow { episode: ep.clone() }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn EpisodeRow(episode: MediaItemResponse) -> Element {
    let ep_num = episode.episode_number.unwrap_or(0);
    let size = format_size(episode.file_size_bytes);
    let fmt = episode.container_format.clone().unwrap_or_default().to_uppercase();
    let file_path = episode.file_path.clone().unwrap_or_default();
    let file_name = std::path::Path::new(&file_path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("")
        .to_string();

    rsx! {
        div { class: "episode-row",
            div { class: "episode-number", "{ep_num}" }
            div { class: "episode-info",
                h4 { class: "episode-title", "{episode.title}" }
                p { class: "episode-file", "{file_name}" }
            }
            div { class: "episode-meta",
                if !fmt.is_empty() {
                    span { class: "detail-meta-tag", "{fmt}" }
                }
                if !size.is_empty() {
                    span { class: "detail-meta-tag", "{size}" }
                }
            }
            div { class: "episode-actions",
                button { class: "btn btn-sm btn-secondary", disabled: true, "Play" }
            }
        }
    }
}
