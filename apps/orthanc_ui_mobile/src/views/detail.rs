use crate::Route;
use dioxus::prelude::*;
use orthanc_core::api::{self, MediaItemResponse};
use orthanc_core::auth::{AuthState, with_refresh};
use orthanc_core::formatters::{format_runtime, format_year};

/// Movie/show detail. Detects type from the API response and renders the
/// appropriate sub-layout. Movies get a Play button; shows get a Play button
/// for the first episode (resume target) plus an episode picker per season.
#[component]
pub fn Detail(id: i64) -> Element {
    let auth = use_context::<Signal<AuthState>>();
    let nav = use_navigator();

    let mut item = use_signal(|| None::<MediaItemResponse>);
    let mut error = use_signal(|| None::<String>);

    use_effect(move || {
        spawn(async move {
            // Try movie first, fall back to show. The server has separate
            // endpoints for each — there is no unified GET /api/media/:id.
            let movie = with_refresh(
                auth,
                |token| async move { api::get_movie(&token, id).await },
            )
            .await;

            match movie {
                Ok(m) => item.set(Some(m)),
                Err(_) => {
                    let show =
                        with_refresh(auth, |token| async move { api::get_show(&token, id).await })
                            .await;
                    match show {
                        Ok(s) => item.set(Some(s)),
                        Err(e) => error.set(Some(e)),
                    }
                }
            }
        });
    });

    let it = item.read();

    rsx! {
        div { class: "detail-page",
            button {
                class: "back-btn-floating",
                onclick: move |_| { nav.go_back(); },
                "← Back"
            }

            if let Some(err) = error() {
                div { class: "error-msg detail-error", "{err}" }
            } else if let Some(media) = it.as_ref() {
                DetailBody { media: media.clone() }
            } else {
                div { class: "fullscreen-loader", "Loading..." }
            }
        }
    }
}

#[component]
fn DetailBody(media: MediaItemResponse) -> Element {
    let backdrop = media
        .backdrop_url
        .as_ref()
        .or(media.poster_url.as_ref())
        .map(|p| format!("{}{}", api::base_url(), p));

    let year = format_year(&media.release_date);
    let runtime = format_runtime(media.duration_seconds);
    let is_show = media.media_type == "tv_show";

    // For shows, the playable target is the first episode of the first
    // season — server doesn't currently expose a "resume next episode" call,
    // so v1 always plays from the start of the first available episode.
    let playable_id = if is_show {
        first_episode_id(&media).unwrap_or(media.id)
    } else {
        media.id
    };

    rsx! {
        div { class: "detail-hero",
            if let Some(src) = backdrop {
                img {
                    class: "detail-backdrop",
                    src: "{src}",
                    alt: "{media.title}",
                }
            }
            div { class: "detail-hero-overlay" }
        }

        div { class: "detail-content",
            h1 { class: "detail-title", "{media.title}" }

            div { class: "detail-meta",
                if !year.is_empty() {
                    span { "{year}" }
                }
                if !runtime.is_empty() {
                    span { class: "meta-dot", "·" }
                    span { "{runtime}" }
                }
                if let Some(rating) = media.rating {
                    span { class: "meta-dot", "·" }
                    span { "★ {rating:.1}" }
                }
                if let Some(rt) = media.content_rating.as_ref() {
                    span { class: "meta-dot", "·" }
                    span { class: "content-rating", "{rt}" }
                }
            }

            Link {
                to: Route::Player { id: playable_id },
                class: "btn btn-primary btn-full detail-play",
                "▶ Play"
            }

            if let Some(desc) = media.description.as_ref() {
                p { class: "detail-desc", "{desc}" }
            }

            if let Some(genres) = media.genres.as_ref() {
                if !genres.is_empty() {
                    div { class: "detail-chips",
                        for g in genres {
                            span { key: "{g}", class: "chip", "{g}" }
                        }
                    }
                }
            }

            if is_show {
                SeasonsList { show: media.clone() }
            }
        }
    }
}

/// Walks the show → season → episode tree and returns the first episode id.
fn first_episode_id(show: &MediaItemResponse) -> Option<i64> {
    let seasons = show.children.as_ref()?;
    let first_season = seasons.first()?;
    let episodes = first_season.children.as_ref()?;
    let first_ep = episodes.first()?;
    Some(first_ep.id)
}

#[component]
fn SeasonsList(show: MediaItemResponse) -> Element {
    let seasons = show.children.unwrap_or_default();
    if seasons.is_empty() {
        return rsx! {};
    }

    // Default to the first season expanded — typical Netflix UX.
    let mut selected_season = use_signal(|| seasons.first().map(|s| s.id).unwrap_or(0));

    rsx! {
        section { class: "seasons-section",
            div { class: "seasons-tabs",
                for s in &seasons {
                    button {
                        key: "{s.id}",
                        class: if selected_season() == s.id {
                            "season-tab season-tab-active"
                        } else {
                            "season-tab"
                        },
                        onclick: {
                            let id = s.id;
                            move |_| selected_season.set(id)
                        },
                        if let Some(n) = s.season_number {
                            "Season {n}"
                        } else {
                            "{s.title}"
                        }
                    }
                }
            }
            for s in &seasons {
                if selected_season() == s.id {
                    EpisodeList { season: s.clone() }
                }
            }
        }
    }
}

#[component]
fn EpisodeList(season: MediaItemResponse) -> Element {
    let episodes = season.children.unwrap_or_default();
    rsx! {
        ul { class: "episode-list",
            for ep in episodes {
                li { key: "{ep.id}", class: "episode-row",
                    Link {
                        to: Route::Player { id: ep.id },
                        class: "episode-link",
                        div { class: "episode-num",
                            if let Some(n) = ep.episode_number {
                                "{n}"
                            } else { "·" }
                        }
                        div { class: "episode-info",
                            div { class: "episode-title", "{ep.title}" }
                            if let Some(desc) = ep.description.as_ref() {
                                div { class: "episode-desc", "{desc}" }
                            }
                        }
                        div { class: "episode-runtime",
                            "{format_runtime(ep.duration_seconds)}"
                        }
                    }
                }
            }
        }
    }
}
