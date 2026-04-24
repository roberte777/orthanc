use dioxus::prelude::*;
use orthanc_core::auth::AuthState;
use orthanc_core::storage::{KEY_SERVER_URL, Storage};
use std::sync::Arc;

mod components;
mod storage;
mod views;

use storage::KeyringStorage;

use views::{
    detail::Detail, home::Home, login::Login, mobile_shell::MobileShell, player::Player,
    profile::Profile, server_config::ServerConfig,
};

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
pub enum Route {
    #[route("/server-config")]
    ServerConfig {},
    #[route("/login")]
    Login {},
    #[route("/detail/:id")]
    Detail { id: i64 },
    #[route("/player/:id")]
    Player { id: i64 },
    #[layout(MobileShell)]
        #[route("/")]
        Home {},
        #[route("/profile")]
        Profile {},
}

const MAIN_CSS: Asset = asset!("/assets/styling/mobile.css");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    // Provide platform-specific token storage at the root so every
    // descendant can read/write tokens via the shared `Storage` trait.
    let storage: Storage = Arc::new(KeyringStorage::new());

    // If the user has previously configured a server URL, load it before any
    // descendant component issues an API request. Without this, the very first
    // render may try to hit the default localhost URL.
    if let Some(url) = storage.get(KEY_SERVER_URL) {
        orthanc_core::api::set_base_url(url);
    }

    use_context_provider::<Storage>(|| storage);
    use_context_provider(|| Signal::new(AuthState::default()));

    rsx! {
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        Router::<Route> {}
    }
}
