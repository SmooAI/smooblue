//! Suggested-follow row — one card in the Suggestions column.
//!
//! Renders avatar + display name + handle + bio + Follow toggle.
//! Clicking the avatar/name opens the full ProfileSheet so the user
//! can see banner + counts + recent posts before deciding to follow.
//! The Follow button itself is optimistic — flips immediately on
//! click, network reconciles in the background.

use crate::auth_refresh::fresh_client;
use crate::icons;
use crate::state::ProfileFocus;
use dioxus::prelude::*;
use smooblue_atproto::ActorProfile;
use smooblue_oauth::Session;

#[component]
pub fn SuggestionRow(actor: ActorProfile) -> Element {
    let mut profile_focus = use_context::<Signal<ProfileFocus>>();
    let session = use_context::<Signal<Option<Session>>>();

    let did = actor.did.clone();
    let handle = actor.handle.clone();
    let display = actor
        .display_name
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&actor.handle)
        .to_string();
    let avatar = actor.avatar.clone();
    let bio = actor
        .description
        .clone()
        .unwrap_or_default()
        .trim()
        .to_string();

    let initial_follow_uri = actor
        .viewer
        .as_ref()
        .and_then(|v| v.following.as_ref().cloned());
    let mut follow_uri = use_signal(|| initial_follow_uri.clone());
    let mut follow_pending = use_signal(|| false);
    let is_following = follow_uri.read().is_some();

    // Closures that capture `profile_focus` are FnMut and can't be Copy,
    // so we mint two separate ones for the two click targets.
    let did_for_avatar = did.clone();
    let open_profile_avatar = move |_| {
        profile_focus.set(ProfileFocus(Some(did_for_avatar.clone())));
    };
    let did_for_name = did.clone();
    let open_profile_name = move |_| {
        profile_focus.set(ProfileFocus(Some(did_for_name.clone())));
    };

    let did_for_follow = did.clone();
    let toggle_follow = move |evt: MouseEvent| {
        evt.stop_propagation();
        if *follow_pending.read() {
            return;
        }
        if session.read().is_none() {
            return;
        }
        follow_pending.set(true);
        let did_clone = did_for_follow.clone();
        let currently_following = follow_uri.read().clone();
        spawn(async move {
            let Some(client) = fresh_client(session).await else {
                follow_pending.set(false);
                return;
            };
            if let Some(uri) = currently_following {
                match client.delete_record(&uri).await {
                    Ok(_) => follow_uri.set(None),
                    Err(e) => tracing::warn!(error = %e, "smooblue: unfollow failed"),
                }
            } else {
                match client.create_follow(&did_clone).await {
                    Ok(rec) => follow_uri.set(Some(rec.uri)),
                    Err(e) => tracing::warn!(error = %e, "smooblue: follow failed"),
                }
            }
            follow_pending.set(false);
        });
    };

    let follow_class = if is_following {
        "btn btn--secondary suggestion__follow suggestion__follow--following"
    } else {
        "btn btn--primary suggestion__follow"
    };
    let follow_label = if *follow_pending.read() {
        "…"
    } else if is_following {
        "Following"
    } else {
        "Follow"
    };

    rsx! {
        article { class: "suggestion",
            div { class: "suggestion__avatar suggestion__avatar--clickable",
                onclick: open_profile_avatar,
                title: "Open profile",
                if let Some(url) = avatar {
                    img { src: "{url}", alt: "{handle}" }
                } else {
                    div { class: "suggestion__avatar-placeholder",
                        icons::User { size: icons::Size::Md }
                    }
                }
            }
            div { class: "suggestion__body",
                div { class: "suggestion__head",
                    button { class: "suggestion__name-link",
                        onclick: open_profile_name,
                        span { class: "suggestion__name", "{display}" }
                        span { class: "suggestion__handle", "@{handle}" }
                    }
                }
                if !bio.is_empty() {
                    p { class: "suggestion__bio", "{bio}" }
                }
            }
            if session.read().is_some() {
                button {
                    class: "{follow_class}",
                    disabled: *follow_pending.read(),
                    onclick: toggle_follow,
                    "{follow_label}"
                }
            }
        }
    }
}
