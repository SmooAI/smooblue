//! Profile editor — opens from the own-profile view. Lets the user
//! change display name, bio, avatar, and banner. Avatar/banner go
//! through `com.atproto.repo.uploadBlob` first to mint a BlobRef
//! that the new profile record can reference.
//!
//! Round-trips the existing profile record so we don't clobber
//! fields we don't render (joinedViaStarterPack, labels, pinnedPost,
//! etc.) — `getRecord` + modify + `putRecord` with `swapRecord`.

use crate::auth_refresh::fresh_client;
use crate::icons;
use crate::state::ProfileEditOpen;
use dioxus::prelude::*;
use smooblue_oauth::Session;

#[component]
pub fn ProfileEditSheet() -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let mut open = use_context::<Signal<ProfileEditOpen>>();

    if !open.read().0 {
        return rsx! { Fragment {} };
    }

    let mut loading = use_signal(|| true);
    let mut load_error = use_signal(|| None::<String>);
    let mut existing_value = use_signal(|| serde_json::Value::Null);
    let mut swap_cid = use_signal(String::new);

    let mut display_name = use_signal(String::new);
    let mut description = use_signal(String::new);
    let mut new_avatar = use_signal(|| None::<(Vec<u8>, String)>);
    let mut new_banner = use_signal(|| None::<(Vec<u8>, String)>);

    let mut saving = use_signal(|| false);
    let mut save_error = use_signal(|| None::<String>);

    // Boot-load the existing record. Single-shot — we don't need
    // a use_resource because there's no parameter to react to.
    use_future(move || async move {
        let Some(client) = fresh_client(session).await else {
            load_error.set(Some("Not signed in".into()));
            loading.set(false);
            return;
        };
        match client.get_profile_record().await {
            Ok((value, cid)) => {
                display_name.set(
                    value
                        .get("displayName")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                );
                description.set(
                    value
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                );
                existing_value.set(value);
                swap_cid.set(cid);
                loading.set(false);
            }
            Err(e) => {
                load_error.set(Some(format!("Couldn't load profile: {e}")));
                loading.set(false);
            }
        }
    });

    let close = move |_| {
        open.set(ProfileEditOpen(false));
    };

    let pick_avatar = move |_| {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["jpg", "jpeg", "png", "webp", "gif"])
            .pick_file()
        {
            if let Ok(bytes) = std::fs::read(&path) {
                let mime = guess_image_mime(&path);
                new_avatar.set(Some((bytes, mime)));
            }
        }
    };
    let pick_banner = move |_| {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["jpg", "jpeg", "png", "webp", "gif"])
            .pick_file()
        {
            if let Ok(bytes) = std::fs::read(&path) {
                let mime = guess_image_mime(&path);
                new_banner.set(Some((bytes, mime)));
            }
        }
    };

    let save = move |_| {
        if *saving.read() {
            return;
        }
        if *loading.read() {
            return;
        }
        saving.set(true);
        save_error.set(None);
        let mut value = existing_value.read().clone();
        if !value.is_object() {
            value = serde_json::json!({});
        }
        let dn = display_name.read().trim().to_string();
        let desc = description.read().trim().to_string();
        let cid = swap_cid.read().clone();
        let avatar_upload = new_avatar.read().clone();
        let banner_upload = new_banner.read().clone();

        spawn(async move {
            let Some(client) = fresh_client(session).await else {
                save_error.set(Some("Not signed in".into()));
                saving.set(false);
                return;
            };
            // Upload new avatar/banner first; on success substitute
            // the BlobRef into the record. On failure surface a
            // clear error rather than silently saving without it.
            let obj = value.as_object_mut().expect("ensured object above");

            if let Some((bytes, mime)) = avatar_upload {
                match client.upload_blob(bytes, &mime).await {
                    Ok(blob) => {
                        obj.insert(
                            "avatar".into(),
                            serde_json::to_value(blob).unwrap_or(serde_json::Value::Null),
                        );
                    }
                    Err(e) => {
                        save_error.set(Some(format!("Avatar upload failed: {e}")));
                        saving.set(false);
                        return;
                    }
                }
            }
            if let Some((bytes, mime)) = banner_upload {
                match client.upload_blob(bytes, &mime).await {
                    Ok(blob) => {
                        obj.insert(
                            "banner".into(),
                            serde_json::to_value(blob).unwrap_or(serde_json::Value::Null),
                        );
                    }
                    Err(e) => {
                        save_error.set(Some(format!("Banner upload failed: {e}")));
                        saving.set(false);
                        return;
                    }
                }
            }

            // Strings: an empty input means "clear the field" rather
            // than "leave unchanged" — that's the only sensible read
            // when the user blanks out their bio in the editor.
            if dn.is_empty() {
                obj.remove("displayName");
            } else {
                obj.insert("displayName".into(), serde_json::Value::String(dn));
            }
            if desc.is_empty() {
                obj.remove("description");
            } else {
                obj.insert("description".into(), serde_json::Value::String(desc));
            }

            // putRecord wants the $type tag inside the record body.
            obj.entry("$type")
                .or_insert(serde_json::Value::String("app.bsky.actor.profile".into()));

            match client.put_profile_record(value, &cid).await {
                Ok(()) => {
                    saving.set(false);
                    open.set(ProfileEditOpen(false));
                }
                Err(e) => {
                    save_error.set(Some(format!("Save failed: {e}")));
                    saving.set(false);
                }
            }
        });
    };

    rsx! {
        div { class: "modal__backdrop", onclick: close,
            div { class: "modal__sheet profile-edit__sheet",
                onclick: move |e| e.stop_propagation(),
                div { class: "profile-edit__head",
                    span { class: "profile-edit__title", "Edit profile" }
                    button { class: "profile-edit__close",
                        onclick: close,
                        icons::X { size: icons::Size::Sm }
                    }
                }
                if *loading.read() {
                    div { class: "profile-edit__loading", "Loading…" }
                } else if let Some(err) = load_error.read().clone() {
                    div { class: "profile-edit__error", "{err}" }
                } else {
                    div { class: "profile-edit__body",
                        // Display name
                        label { class: "profile-edit__label", "Display name" }
                        input { class: "input",
                            value: "{display_name}",
                            placeholder: "Your name on bsky",
                            oninput: move |e| display_name.set(e.value()),
                        }
                        // Bio
                        label { class: "profile-edit__label", "Bio" }
                        textarea { class: "input profile-edit__bio",
                            value: "{description}",
                            placeholder: "Tell people who you are…",
                            oninput: move |e| description.set(e.value()),
                        }
                        // Avatar picker
                        label { class: "profile-edit__label", "Avatar" }
                        div { class: "profile-edit__row",
                            button { class: "btn btn--ghost",
                                onclick: pick_avatar,
                                "Choose image…"
                            }
                            if new_avatar.read().is_some() {
                                span { class: "profile-edit__hint",
                                    "✓ new avatar selected (saves on update)"
                                }
                            }
                        }
                        // Banner picker
                        label { class: "profile-edit__label", "Banner" }
                        div { class: "profile-edit__row",
                            button { class: "btn btn--ghost",
                                onclick: pick_banner,
                                "Choose image…"
                            }
                            if new_banner.read().is_some() {
                                span { class: "profile-edit__hint",
                                    "✓ new banner selected (saves on update)"
                                }
                            }
                        }

                        if let Some(err) = save_error.read().clone() {
                            div { class: "profile-edit__error", "{err}" }
                        }

                        button { class: "btn btn--primary profile-edit__save",
                            disabled: *saving.read(),
                            onclick: save,
                            if *saving.read() { "Saving…" } else { "Save profile" }
                        }
                    }
                }
            }
        }
    }
}

/// Best-effort mime guess from file extension. We could read the
/// magic bytes but for the four formats we accept (jpg/png/webp/gif)
/// the extension is a fine proxy.
fn guess_image_mime(path: &std::path::Path) -> String {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => "image/jpeg",
    }
    .to_string()
}
