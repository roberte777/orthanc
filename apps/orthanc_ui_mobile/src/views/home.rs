use crate::components::hero_section::HeroSection;
use crate::components::horizontal_row::HorizontalRow;
use dioxus::prelude::*;
use orthanc_core::api::{self, MediaItemResponse, RecentMedia};
use orthanc_core::auth::{AuthState, with_refresh};

/// Netflix-style home: featured hero on top, then horizontal rows of recent
/// media (movies, then shows). The first movie (by recency) is the hero
/// pick — when there are no movies, falls back to the first show.
#[component]
pub fn Home() -> Element {
    let auth = use_context::<Signal<AuthState>>();

    let mut recent = use_signal(|| None::<RecentMedia>);
    let mut error = use_signal(|| None::<String>);

    use_effect(move || {
        spawn(async move {
            match with_refresh(
                auth,
                |token| async move { api::get_recent_media(&token).await },
            )
            .await
            {
                Ok(r) => recent.set(Some(r)),
                Err(e) => error.set(Some(e)),
            }
        });
    });

    let featured: Option<MediaItemResponse> = recent
        .read()
        .as_ref()
        .and_then(|r| r.movies.first().or_else(|| r.shows.first()).cloned());

    let movies = recent
        .read()
        .as_ref()
        .map(|r| r.movies.clone())
        .unwrap_or_default();
    let shows = recent
        .read()
        .as_ref()
        .map(|r| r.shows.clone())
        .unwrap_or_default();

    rsx! {
        div { class: "home-page",
            HeroSection { featured }

            if let Some(err) = error() {
                div { class: "error-msg home-error", "{err}" }
            }

            HorizontalRow { title: "Recently Added Movies".to_string(), items: movies }
            HorizontalRow { title: "TV Shows".to_string(), items: shows }
        }
    }
}
