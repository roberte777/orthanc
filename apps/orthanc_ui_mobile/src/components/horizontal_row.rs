use crate::components::media_card::MediaCard;
use dioxus::prelude::*;
use orthanc_core::api::MediaItemResponse;

/// Heading + horizontally-scrolling list of MediaCards. Native browser
/// touch-scroll with momentum (no JS gesture work needed).
#[component]
pub fn HorizontalRow(title: String, items: Vec<MediaItemResponse>) -> Element {
    if items.is_empty() {
        return rsx! {};
    }
    rsx! {
        section { class: "horizontal-row",
            h2 { class: "row-title", "{title}" }
            div { class: "row-scroller",
                for item in items {
                    MediaCard { key: "{item.id}", item: item.clone() }
                }
            }
        }
    }
}
