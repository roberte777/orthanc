use dioxus::prelude::*;
use crate::{
    api::{self, CreateUserRequest, UpdateUserRequest, UserResponse},
    state::AuthState,
};

#[component]
pub fn AdminUsers() -> Element {
    let auth = use_context::<Signal<AuthState>>();
    let nav = use_navigator();

    // Redirect non-admins
    if !auth.read().is_admin() {
        nav.replace(crate::Route::Home {});
    }

    let token = auth.read().access_token.clone().unwrap_or_default();
    let mut users = use_signal(Vec::<UserResponse>::new);
    let mut total = use_signal(|| 0i64);
    let page = use_signal(|| 1u32);
    let mut loading = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);
    let mut show_create = use_signal(|| false);
    let mut delete_confirm = use_signal(|| Option::<i64>::None);

    // Define load_users as a closure; clone it for each use site
    let load_users = {
        let token = token.clone();
        move || {
            let tok = token.clone();
            let pg = page();
            loading.set(true);
            spawn(async move {
                match api::list_users(&tok, pg).await {
                    Ok(resp) => {
                        users.set(resp.users);
                        total.set(resp.total);
                    }
                    Err(e) => error.set(Some(e)),
                }
                loading.set(false);
            });
        }
    };

    let mut load_for_effect = load_users.clone();
    let load_for_create = load_users.clone();
    let load_for_delete = load_users.clone();

    use_effect(move || load_for_effect());

    let current_user_id = auth.read().user.as_ref().map(|u| u.id);

    rsx! {
        div { class: "page",
            div { class: "page-header",
                h1 { class: "page-title", "User Management" }
                button {
                    class: "btn btn-primary",
                    onclick: move |_| show_create.set(true),
                    "+ Add User"
                }
            }

            if let Some(err) = error() {
                div { class: "error-msg", "{err}" }
            }

            if loading() {
                div { class: "loading", "Loading..." }
            } else {
                div { class: "card",
                    table { class: "table",
                        thead {
                            tr {
                                th { "Username" }
                                th { "Email" }
                                th { "Display Name" }
                                th { "Role" }
                                th { "Status" }
                                th { "Actions" }
                            }
                        }
                        tbody {
                            for user in users() {
                                {
                                    let uid = user.id;
                                    let is_self = Some(uid) == current_user_id;
                                    let tok2 = auth.read().access_token.clone().unwrap_or_default();
                                    let is_active = user.is_active;
                                    let is_admin_user = user.is_admin;
                                    let load_for_row = load_users.clone();
                                    rsx! {
                                        tr { key: "{uid}",
                                            td { "{user.username}" }
                                            td { "{user.email}" }
                                            td { "{user.display_name.clone().unwrap_or_default()}" }
                                            td {
                                                span {
                                                    class: if is_admin_user {
                                                        "badge badge-admin"
                                                    } else {
                                                        "badge badge-user"
                                                    },
                                                    if is_admin_user { "Admin" } else { "User" }
                                                }
                                            }
                                            td {
                                                span {
                                                    class: if is_active {
                                                        "badge badge-active"
                                                    } else {
                                                        "badge badge-inactive"
                                                    },
                                                    if is_active { "Active" } else { "Inactive" }
                                                }
                                            }
                                            td {
                                                div { class: "action-buttons",
                                                    if !is_self {
                                                        button {
                                                            class: "btn btn-sm btn-secondary",
                                                            onclick: move |_| {
                                                                let tok = tok2.clone();
                                                                let new_active = !is_active;
                                                                let mut reload = load_for_row.clone();
                                                                spawn(async move {
                                                                    let _ = api::update_user(
                                                                            &tok,
                                                                            uid,
                                                                            UpdateUserRequest {
                                                                                is_active: Some(
                                                                                    new_active,
                                                                                ),
                                                                                ..Default::default()
                                                                            },
                                                                        )
                                                                        .await;
                                                                    reload();
                                                                });
                                                            },
                                                            if is_active { "Disable" } else { "Enable" }
                                                        }
                                                        button {
                                                            class: "btn btn-sm btn-danger",
                                                            onclick: move |_| {
                                                                delete_confirm.set(Some(uid))
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
                        }
                    }
                }
            }

            // Pagination info
            div { class: "pagination-info", "Total users: {total()}" }

            // Create user modal
            if show_create() {
                CreateUserModal {
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

            // Delete confirmation modal
            if let Some(uid) = delete_confirm() {
                div { class: "modal-overlay",
                    div { class: "modal",
                        h3 { "Delete User" }
                        p {
                            "Are you sure you want to delete this user? This cannot be undone."
                        }
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
                                        let _ = api::delete_user(&tok, uid).await;
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

#[derive(Props, Clone, PartialEq)]
struct CreateUserModalProps {
    token: String,
    on_close: EventHandler,
    on_created: EventHandler,
}

#[component]
fn CreateUserModal(props: CreateUserModalProps) -> Element {
    let mut username = use_signal(String::new);
    let mut email = use_signal(String::new);
    let mut display_name = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut is_admin = use_signal(|| false);
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
            let dn = display_name();
            let req = CreateUserRequest {
                username: username(),
                email: email(),
                password: password(),
                display_name: if dn.is_empty() { None } else { Some(dn) },
                is_admin: is_admin(),
            };
            match api::create_user(&tok, req).await {
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
                    h3 { "Add User" }
                    button {
                        class: "modal-close",
                        onclick: move |_| props.on_close.call(()),
                        "×"
                    }
                }
                if let Some(err) = error() {
                    div { class: "error-msg", "{err}" }
                }
                form { onsubmit: on_submit,
                    div { class: "form-group",
                        input {
                            class: "form-input",
                            r#type: "text",
                            placeholder: "Username",
                            value: "{username}",
                            oninput: move |e| username.set(e.value()),
                            required: true,
                        }
                    }
                    div { class: "form-group",
                        input {
                            class: "form-input",
                            r#type: "email",
                            placeholder: "Email",
                            value: "{email}",
                            oninput: move |e| email.set(e.value()),
                            required: true,
                        }
                    }
                    div { class: "form-group",
                        input {
                            class: "form-input",
                            r#type: "text",
                            placeholder: "Display Name (optional)",
                            value: "{display_name}",
                            oninput: move |e| display_name.set(e.value()),
                        }
                    }
                    div { class: "form-group",
                        input {
                            class: "form-input",
                            r#type: "password",
                            placeholder: "Password",
                            value: "{password}",
                            oninput: move |e| password.set(e.value()),
                            required: true,
                        }
                    }
                    div { class: "form-group checkbox-group",
                        input {
                            r#type: "checkbox",
                            id: "is-admin",
                            checked: is_admin(),
                            onchange: move |e| is_admin.set(e.checked()),
                        }
                        label { r#for: "is-admin", "Administrator" }
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
                            if loading() { "Creating..." } else { "Create User" }
                        }
                    }
                }
            }
        }
    }
}
