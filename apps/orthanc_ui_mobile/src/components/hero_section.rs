use crate::Route;
use dioxus::prelude::*;
use orthanc_core::api::{self, MediaItemResponse};
use orthanc_core::formatters::format_year;

/// Top-of-home featured-media hero. Full-bleed backdrop, gradient overlay,
/// title and metadata at the bottom, big Play button. Picks the first
/// non-empty backdrop from the candidate list — falls back gracefully when
/// none are present.
#[component]
pub fn HeroSection(featured: Option<MediaItemResponse>) -> Element {
    let Some(item) = featured else {
        return rsx! { div { class: "hero-empty" } };
    };

    let id = item.id;
    let backdrop = item
        .backdrop_url
        .as_ref()
        .or(item.poster_url.as_ref())
        .map(|p| format!("{}{}", api::base_url(), p));
    let year = format_year(&item.release_date);
    let is_show = item.media_type == "tv_show";

    rsx! {
        section { class: "hero",
            if let Some(src) = backdrop {
                img {
                    class: "hero-backdrop",
                    src: "{src}",
                    alt: "{item.title}",
                }
            }
            div { class: "hero-overlay" }
            div { class: "hero-content",
                div { class: "hero-meta",
                    if is_show { span { "Series" } } else { span { "Movie" } }
                    if !year.is_empty() {
                        span { class: "hero-meta-dot", "·" }
                        span { "{year}" }
                    }
                }
                h1 { class: "hero-title", "{item.title}" }
                if let Some(desc) = item.description.clone() {
                    p { class: "hero-desc", "{desc}" }
                }
                div { class: "hero-actions",
                    Link { to: Route::Detail { id }, class: "btn btn-primary",
                        "▶ Play"
                    }
                    Link { to: Route::Detail { id }, class: "btn btn-secondary",
                        "ⓘ Info"
                    }
                }
            }
        }
    }
}
