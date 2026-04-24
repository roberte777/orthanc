use crate::Route;
use dioxus::prelude::*;
use dioxus::router::hooks::use_route;

/// Netflix-style bottom tab bar. Sits above the home indicator on iOS using
/// safe-area-inset-bottom (set in mobile.css). Two tabs in v1 — Home and
/// Profile — leaving room to add Search and Library tabs in v2 without
/// changing the layout.
#[component]
pub fn BottomNav() -> Element {
    let current = use_route::<Route>();

    rsx! {
        nav { class: "bottom-nav",
            TabItem {
                label: "Home",
                icon: "🏠",
                route: Route::Home {},
                active: matches!(current, Route::Home {}),
            }
            TabItem {
                label: "Profile",
                icon: "👤",
                route: Route::Profile {},
                active: matches!(current, Route::Profile {}),
            }
        }
    }
}

#[component]
fn TabItem(label: &'static str, icon: &'static str, route: Route, active: bool) -> Element {
    let cls = if active {
        "tab-item tab-item-active"
    } else {
        "tab-item"
    };
    rsx! {
        Link { to: route, class: cls,
            span { class: "tab-icon", "{icon}" }
            span { class: "tab-label", "{label}" }
        }
    }
}
