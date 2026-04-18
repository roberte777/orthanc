use dioxus::prelude::*;
use crate::{
    api::{self, CreateLibraryRequest, LibraryResponse, UpdateLibraryRequest},
    state::AuthState,
};

#[component]
pub fn AdminLibraries() -> Element {
    let auth = use_context::<Signal<AuthState>>();
    let nav = use_navigator();

    if !auth.read().is_admin() {
        nav.replace(crate::Route::Home {});
    }

    let token = auth.read().access_token.clone().unwrap_or_default();
    let mut libraries = use_signal(Vec::<LibraryResponse>::new);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| Option::<String>::None);
    let mut show_create = use_signal(|| false);
    let mut editing_id = use_signal(|| Option::<i64>::None);
    let mut delete_confirm = use_signal(|| Option::<i64>::None);

    let load_libraries = {
        let token = token.clone();
        move || {
            let tok = token.clone();
            loading.set(true);
            spawn(async move {
                match api::list_libraries(&tok).await {
                    Ok(libs) => libraries.set(libs),
                    Err(e) => error.set(Some(e)),
                }
                loading.set(false);
            });
        }
    };

    let mut load_for_effect = load_libraries.clone();
    let load_for_create = load_libraries.clone();
    let load_for_edit = load_libraries.clone();
    let load_for_delete = load_libraries.clone();
    let load_for_path = load_libraries.clone();

    use_effect(move || load_for_effect());

    rsx! {
        div { class: "page",
            div { class: "page-header",
                h1 { class: "page-title", "Libraries" }
                button {
                    class: "btn btn-primary",
                    onclick: move |_| show_create.set(true),
                    "+ Add Library"
                }
            }

            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }

            if loading() {
                div { class: "loading", "Loading..." }
            } else if libraries().is_empty() {
                div { class: "empty-state",
                    h2 { "No Libraries" }
                    p { "Create a library to start organizing your media." }
                }
            } else {
                div { class: "library-grid",
                    for lib in libraries() {
                        {
                            let lib_id = lib.id;
                            let lib_name = lib.name.clone();
                            let lib_type = lib.library_type.clone();
                            let lib_desc = lib.description.clone();
                            let lib_enabled = lib.is_enabled;
                            let paths = lib.paths.clone();
                            let tok = auth.read().access_token.clone().unwrap_or_default();
                            let mut reload_path = load_for_path.clone();

                            rsx! {
                                div { class: "library-card", key: "{lib_id}",
                                    div { class: "library-card-header",
                                        div { class: "library-card-info",
                                            h3 { class: "library-card-name", "{lib_name}" }
                                            span {
                                                class: "badge",
                                                class: if lib_type == "movies" { "badge-movies" } else { "badge-tv" },
                                                if lib_type == "movies" { "Movies" } else { "TV Shows" }
                                            }
                                            if !lib_enabled {
                                                span { class: "badge badge-inactive", "Disabled" }
                                            }
                                        }
                                        div { class: "action-buttons",
                                            ScanButton { token: tok.clone(), library_id: lib_id }
                                            button {
                                                class: "btn btn-sm btn-secondary",
                                                onclick: move |_| editing_id.set(Some(lib_id)),
                                                "Edit"
                                            }
                                            button {
                                                class: "btn btn-sm btn-danger",
                                                onclick: move |_| delete_confirm.set(Some(lib_id)),
                                                "Delete"
                                            }
                                        }
                                    }
                                    if let Some(desc) = &lib_desc {
                                        p { class: "library-card-desc", "{desc}" }
                                    }
                                    div { class: "library-paths",
                                        h4 { class: "library-paths-title", "Paths" }
                                        if paths.is_empty() {
                                            p { class: "library-paths-empty", "No paths configured" }
                                        } else {
                                            for path in &paths {
                                                {
                                                    let path_id = path.id;
                                                    let path_str = path.path.clone();
                                                    let tok2 = tok.clone();
                                                    let reload2 = load_for_path.clone();
                                                    rsx! {
                                                        div { class: "library-path-row", key: "{path_id}",
                                                            span { class: "library-path-text", "{path_str}" }
                                                            button {
                                                                class: "btn btn-sm btn-danger",
                                                                onclick: move |_| {
                                                                    let tok = tok2.clone();
                                                                    let mut reload = reload2.clone();
                                                                    spawn(async move {
                                                                        let _ = api::remove_library_path(&tok, lib_id, path_id).await;
                                                                        reload();
                                                                    });
                                                                },
                                                                "Remove"
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        // Add path input
                                        AddPathInput { token: tok.clone(), library_id: lib_id, on_added: move |_| reload_path() }
                                    }
                                    // Metadata providers
                                    ProviderConfig { token: tok.clone(), library_id: lib_id }
                                }
                            }
                        }
                    }
                }
            }

            // Create library modal
            if show_create() {
                CreateLibraryModal {
                    token: auth.read().access_token.clone().unwrap_or_default(),
                    on_close: move |_| show_create.set(false),
                    on_created: {
                        let mut reload = load_for_create.clone();
                        move |_| {
                            show_create.set(false);
                            reload();
                        }
                    }
                }
            }

            // Edit library modal
            if let Some(edit_id) = editing_id() {
                if let Some(lib) = libraries().iter().find(|l| l.id == edit_id).cloned() {
                    EditLibraryModal {
                        token: auth.read().access_token.clone().unwrap_or_default(),
                        library: lib,
                        on_close: move |_| editing_id.set(None),
                        on_saved: {
                            let mut reload = load_for_edit.clone();
                            move |_| {
                                editing_id.set(None);
                                reload();
                            }
                        }
                    }
                }
            }

            // Delete confirmation modal
            if let Some(del_id) = delete_confirm() {
                div { class: "modal-overlay",
                    div { class: "modal",
                        h3 { "Delete Library" }
                        p { "Are you sure? This will remove the library and all its paths. Media files on disk are not affected." }
                        div { class: "modal-actions",
                            button {
                                class: "btn btn-secondary",
                                onclick: move |_| delete_confirm.set(None),
                                "Cancel"
                            }
                            button {
                                class: "btn btn-danger",
                                onclick: move |_| {
                                    let tok = auth.read().access_token.clone().unwrap_or_default();
                                    delete_confirm.set(None);
                                    let mut reload = load_for_delete.clone();
                                    spawn(async move {
                                        let _ = api::delete_library(&tok, del_id).await;
                                        reload();
                                    });
                                },
                                "Delete"
                            }
                        }
                    }
                }
            }
        }
    }
}

// ── Inline add-path input ──

#[derive(Props, Clone, PartialEq)]
struct AddPathInputProps {
    token: String,
    library_id: i64,
    on_added: EventHandler,
}

#[component]
fn AddPathInput(props: AddPathInputProps) -> Element {
    let mut path_input = use_signal(String::new);
    let mut path_error = use_signal(|| Option::<String>::None);

    let token = props.token.clone();
    let lib_id = props.library_id;

    rsx! {
        div { class: "add-path-row",
            input {
                class: "form-input add-path-input",
                r#type: "text",
                placeholder: "/path/to/media",
                value: "{path_input}",
                oninput: move |e| {
                    path_input.set(e.value());
                    path_error.set(None);
                },
            }
            button {
                class: "btn btn-sm btn-primary",
                disabled: path_input().trim().is_empty(),
                onclick: move |_| {
                    let tok = token.clone();
                    let path = path_input().trim().to_string();
                    let on_added = props.on_added.clone();
                    if path.is_empty() { return; }
                    spawn(async move {
                        match api::add_library_path(&tok, lib_id, &path).await {
                            Ok(_) => {
                                path_input.set(String::new());
                                path_error.set(None);
                                on_added.call(());
                            }
                            Err(e) => path_error.set(Some(e)),
                        }
                    });
                },
                "Add"
            }
        }
        if let Some(err) = path_error() {
            div { class: "error-msg", style: "margin-top: 0.5rem; font-size: 0.8rem;", "{err}" }
        }
    }
}

// ── Create Library Modal ──

#[derive(Props, Clone, PartialEq)]
struct CreateLibraryModalProps {
    token: String,
    on_close: EventHandler,
    on_created: EventHandler,
}

#[component]
fn CreateLibraryModal(props: CreateLibraryModalProps) -> Element {
    let mut name = use_signal(String::new);
    let mut library_type = use_signal(|| "movies".to_string());
    let mut description = use_signal(String::new);
    let mut error = use_signal(|| Option::<String>::None);
    let mut loading = use_signal(|| false);

    let token = props.token.clone();

    let on_submit = move |evt: Event<FormData>| {
        evt.prevent_default();
        loading.set(true);
        error.set(None);
        let tok = token.clone();
        let on_created = props.on_created.clone();
        spawn(async move {
            let desc = description();
            let req = CreateLibraryRequest {
                name: name(),
                library_type: library_type(),
                description: if desc.is_empty() { None } else { Some(desc) },
                paths: vec![],
                scan_interval_minutes: None,
            };
            match api::create_library(&tok, req).await {
                Ok(_) => on_created.call(()),
                Err(e) => {
                    error.set(Some(e));
                    loading.set(false);
                }
            }
        });
    };

    rsx! {
        div { class: "modal-overlay",
            div { class: "modal",
                div { class: "modal-header",
                    h3 { "Add Library" }
                    button {
                        class: "modal-close",
                        onclick: move |_| props.on_close.call(()),
                        "\u{00d7}"
                    }
                }
                if let Some(err) = error() {
                    div { class: "error-msg", "{err}" }
                }
                form { onsubmit: on_submit,
                    div { class: "form-group",
                        label { class: "form-label", "Name" }
                        input {
                            class: "form-input",
                            r#type: "text",
                            placeholder: "e.g. Movies, Anime, Kids TV",
                            value: "{name}",
                            oninput: move |e| name.set(e.value()),
                            required: true,
                        }
                    }
                    div { class: "form-group",
                        label { class: "form-label", "Type" }
                        select {
                            class: "form-input",
                            value: "{library_type}",
                            onchange: move |e| library_type.set(e.value()),
                            option { value: "movies", "Movies" }
                            option { value: "tv_shows", "TV Shows" }
                        }
                    }
                    div { class: "form-group",
                        label { class: "form-label", "Description (optional)" }
                        input {
                            class: "form-input",
                            r#type: "text",
                            placeholder: "A brief description",
                            value: "{description}",
                            oninput: move |e| description.set(e.value()),
                        }
                    }
                    div { class: "modal-actions",
                        button {
                            class: "btn btn-secondary",
                            r#type: "button",
                            onclick: move |_| props.on_close.call(()),
                            "Cancel"
                        }
                        button {
                            class: "btn btn-primary",
                            r#type: "submit",
                            disabled: loading(),
                            if loading() { "Creating..." } else { "Create Library" }
                        }
                    }
                }
            }
        }
    }
}

// ── Edit Library Modal ──

#[derive(Props, Clone, PartialEq)]
struct EditLibraryModalProps {
    token: String,
    library: LibraryResponse,
    on_close: EventHandler,
    on_saved: EventHandler,
}

#[component]
fn EditLibraryModal(props: EditLibraryModalProps) -> Element {
    let mut name = use_signal(|| props.library.name.clone());
    let mut description = use_signal(|| props.library.description.clone().unwrap_or_default());
    let mut is_enabled = use_signal(|| props.library.is_enabled);
    let mut error = use_signal(|| Option::<String>::None);
    let mut loading = use_signal(|| false);

    let token = props.token.clone();
    let lib_id = props.library.id;

    let on_submit = move |evt: Event<FormData>| {
        evt.prevent_default();
        loading.set(true);
        error.set(None);
        let tok = token.clone();
        let on_saved = props.on_saved.clone();
        spawn(async move {
            let desc = description();
            let req = UpdateLibraryRequest {
                name: Some(name()),
                description: if desc.is_empty() { None } else { Some(desc) },
                is_enabled: Some(is_enabled()),
                scan_interval_minutes: None,
            };
            match api::update_library(&tok, lib_id, req).await {
                Ok(_) => on_saved.call(()),
                Err(e) => {
                    error.set(Some(e));
                    loading.set(false);
                }
            }
        });
    };

    rsx! {
        div { class: "modal-overlay",
            div { class: "modal",
                div { class: "modal-header",
                    h3 { "Edit Library" }
                    button {
                        class: "modal-close",
                        onclick: move |_| props.on_close.call(()),
                        "\u{00d7}"
                    }
                }
                if let Some(err) = error() {
                    div { class: "error-msg", "{err}" }
                }
                form { onsubmit: on_submit,
                    div { class: "form-group",
                        label { class: "form-label", "Name" }
                        input {
                            class: "form-input",
                            r#type: "text",
                            value: "{name}",
                            oninput: move |e| name.set(e.value()),
                            required: true,
                        }
                    }
                    div { class: "form-group",
                        label { class: "form-label", "Description" }
                        input {
                            class: "form-input",
                            r#type: "text",
                            value: "{description}",
                            oninput: move |e| description.set(e.value()),
                        }
                    }
                    div { class: "form-group checkbox-group",
                        input {
                            r#type: "checkbox",
                            id: "lib-enabled",
                            checked: is_enabled(),
                            onchange: move |e| is_enabled.set(e.checked()),
                        }
                        label { r#for: "lib-enabled", "Enabled" }
                    }
                    div { class: "modal-actions",
                        button {
                            class: "btn btn-secondary",
                            r#type: "button",
                            onclick: move |_| props.on_close.call(()),
                            "Cancel"
                        }
                        button {
                            class: "btn btn-primary",
                            r#type: "submit",
                            disabled: loading(),
                            if loading() { "Saving..." } else { "Save Changes" }
                        }
                    }
                }
            }
        }
    }
}

// ── Scan Button ──

#[derive(Props, Clone, PartialEq)]
struct ScanButtonProps {
    token: String,
    library_id: i64,
}

#[component]
fn ScanButton(props: ScanButtonProps) -> Element {
    let mut scanning = use_signal(|| false);
    let mut scan_msg = use_signal(|| Option::<(bool, String)>::None);

    let token = props.token.clone();
    let lib_id = props.library_id;

    rsx! {
        button {
            class: "btn btn-sm btn-primary",
            disabled: scanning(),
            onclick: move |_| {
                let tok = token.clone();
                scanning.set(true);
                scan_msg.set(None);
                spawn(async move {
                    match api::scan_library(&tok, lib_id).await {
                        Ok(result) => {
                            let msg = format!("{} added, {} unchanged", result.added, result.unchanged);
                            scan_msg.set(Some((true, msg)));
                        }
                        Err(e) => {
                            scan_msg.set(Some((false, e)));
                        }
                    }
                    scanning.set(false);
                });
            },
            if scanning() { "Scanning..." } else { "Scan" }
        }
        if let Some((ok, msg)) = scan_msg() {
            span {
                class: if ok { "scan-result-ok" } else { "scan-result-err" },
                "{msg}"
            }
        }
    }
}

// ── Provider Config ──

#[derive(Props, Clone, PartialEq)]
struct ProviderConfigProps {
    token: String,
    library_id: i64,
}

#[component]
fn ProviderConfig(props: ProviderConfigProps) -> Element {
    let mut providers = use_signal(Vec::<api::MetadataProviderResponse>::new);
    let mut loaded = use_signal(|| false);

    let token = props.token.clone();
    let lib_id = props.library_id;

    let load_providers = {
        let tok = token.clone();
        move || {
            let tok = tok.clone();
            spawn(async move {
                if let Ok(p) = api::list_providers(&tok, lib_id).await {
                    providers.set(p);
                }
                loaded.set(true);
            });
        }
    };

    let mut initial_load = load_providers.clone();
    use_effect(move || { initial_load(); });

    let provider_list = providers();

    let total = provider_list.len();

    rsx! {
        div { class: "library-providers",
            h4 { class: "library-paths-title", "Metadata Providers" }
            div { class: "provider-list",
                for (idx, prov) in provider_list.iter().enumerate() {
                    {
                        let name = prov.provider.clone();
                        let display_name = match name.as_str() {
                            "tmdb" => "TMDB".to_string(),
                            "anidb" => "AniDB".to_string(),
                            _ => name.clone(),
                        };
                        let enabled = prov.is_enabled;
                        let priority = prov.priority;
                        let is_first = idx == 0;
                        let is_last = idx == total - 1;
                        let tok = token.clone();
                        let reload = load_providers.clone();

                        let prev_name = if idx > 0 { Some(provider_list[idx - 1].provider.clone()) } else { None };
                        let next_name = if idx + 1 < total { Some(provider_list[idx + 1].provider.clone()) } else { None };

                        rsx! {
                            div { class: "provider-row", key: "{name}",
                                div { class: "provider-info",
                                    span {
                                        class: if enabled { "provider-name" } else { "provider-name provider-disabled" },
                                        "{display_name}"
                                    }
                                }
                                div { class: "provider-controls",
                                    button {
                                        class: "provider-arrow",
                                        disabled: is_first,
                                        onclick: {
                                            let name = name.clone();
                                            let tok = tok.clone();
                                            let mut reload = reload.clone();
                                            let prev_name = prev_name.clone();
                                            move |_| {
                                                let tok = tok.clone();
                                                let name = name.clone();
                                                let mut reload = reload.clone();
                                                if let Some(ref other) = prev_name {
                                                    let other = other.clone();
                                                    spawn(async move {
                                                        let _ = api::swap_providers(&tok, lib_id, &name, &other).await;
                                                        reload();
                                                    });
                                                }
                                            }
                                        },
                                        "\u{25B2}"
                                    }
                                    button {
                                        class: "provider-arrow",
                                        disabled: is_last,
                                        onclick: {
                                            let name = name.clone();
                                            let tok = tok.clone();
                                            let mut reload = reload.clone();
                                            let next_name = next_name.clone();
                                            move |_| {
                                                let tok = tok.clone();
                                                let name = name.clone();
                                                let mut reload = reload.clone();
                                                if let Some(ref other) = next_name {
                                                    let other = other.clone();
                                                    spawn(async move {
                                                        let _ = api::swap_providers(&tok, lib_id, &name, &other).await;
                                                        reload();
                                                    });
                                                }
                                            }
                                        },
                                        "\u{25BC}"
                                    }
                                    label { class: "toggle",
                                        input {
                                            r#type: "checkbox",
                                            checked: enabled,
                                            onchange: {
                                                let name = name.clone();
                                                let tok = tok.clone();
                                                let mut reload = reload.clone();
                                                move |e: Event<FormData>| {
                                                    let new_enabled = e.checked();
                                                    let tok = tok.clone();
                                                    let name = name.clone();
                                                    let mut reload = reload.clone();
                                                    spawn(async move {
                                                        let _ = api::update_provider(&tok, lib_id, &name, new_enabled).await;
                                                        reload();
                                                    });
                                                }
                                            },
                                        }
                                        span { class: "toggle-slider" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
