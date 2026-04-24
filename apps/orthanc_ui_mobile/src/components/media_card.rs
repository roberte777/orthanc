use crate::Route;
use dioxus::prelude::*;
use orthanc_core::api::{self, MediaItemResponse};

/// Portrait poster card. 2:3 aspect ratio (Netflix standard for mobile rows).
/// Tapping navigates to the detail page. No hover state — touch-only.
#[component]
pub fn MediaCard(item: MediaItemResponse) -> Element {
    let id = item.id;
    let poster_src = item
        .poster_url
        .as_ref()
        .map(|p| format!("{}{}", api::base_url(), p));

    rsx! {
        Link { to: Route::Detail { id }, class: "media-card",
            if let Some(src) = poster_src {
                img {
                    class: "media-poster",
                    src: "{src}",
                    alt: "{item.title}",
                    loading: "lazy",
                }
            } else {
                div { class: "media-poster media-poster-fallback",
                    span { class: "media-fallback-title", "{item.title}" }
                }
            }
        }
    }
}
