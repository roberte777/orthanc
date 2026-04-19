use dioxus::prelude::*;
use state::AuthState;

mod api;
mod components;
mod state;
mod views;

use views::{
    admin_libraries::AdminLibraries,
    admin_playback::AdminPlayback,
    admin_settings::AdminSettings,
    admin_users::AdminUsers,
    app_shell::AppShell,
    browse_movies::{BrowseMovies, MovieDetail},
    browse_shows::{BrowseShows, ShowDetail},
    home::Home,
    login::Login,
    player::Player,
    settings::Settings,
    setup::Setup,
};

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    #[route("/login")]
    Login {},
    #[route("/setup")]
    Setup {},
    #[route("/player/:id")]
    Player { id: i64 },
    #[layout(AppShell)]
        #[route("/")]
        Home {},
        #[route("/movies")]
        BrowseMovies {},
        #[route("/movies/:id")]
        MovieDetail { id: i64 },
        #[route("/shows")]
        BrowseShows {},
        #[route("/shows/:id")]
        ShowDetail { id: i64 },
        #[route("/settings")]
        Settings {},
        #[route("/admin/libraries")]
        AdminLibraries {},
        #[route("/admin/users")]
        AdminUsers {},
        #[route("/admin/playback")]
        AdminPlayback {},
        #[route("/admin/settings")]
        AdminSettings {},
}

const FAVICON: Asset = asset!("/assets/favicon.ico");
const MAIN_CSS: Asset = asset!("/assets/styling/main.css");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    use_context_provider(|| Signal::new(AuthState::default()));

    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        Router::<Route> {}
    }
}
