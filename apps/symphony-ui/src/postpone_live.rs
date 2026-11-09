//! Server-backed Later/Undo controls shared by the queue and the verdict editor.

use crate::{docs, frame::FrameRefresh, request};
use dioxus::prelude::*;

#[component]
pub fn Controls(
    path: String,
    slug: String,
    scope: String,
    postponed: bool,
    #[props(default)] seen: Option<String>,
) -> Element {
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let frame = use_context::<FrameRefresh>();
    let p = path.clone();
    let s = slug.clone();
    let sc = scope.clone();
    use_effect(use_reactive!(|seen| {
        if seen.is_some() {
            let p = p.clone();
            let s = s.clone();
            let sc = sc.clone();
            spawn(async move {
                match request::post(
                    &docs::scoped(&sc, "/api/postpone"),
                    &[
                        ("path", p.into()),
                        ("slug", s.into()),
                        ("action", "seen".into()),
                    ],
                )
                .await
                {
                    Ok(value) if request::field(&value, "changed").as_bool() == Some(true) => {
                        frame.refresh()
                    }
                    Ok(_) => {}
                    Err(why) => error.set(Some(why)),
                }
            });
        }
    }));
    rsx! {
        span { class: "postpone-controls",
            if postponed { small { class: "postponed", "postponed" } }
            crate::components::button::Button { r#type: "button", disabled: pending(), title: "Return after the rest of this group, or in 3 days",
                onclick: move |_| {
                    if pending() { return; }
                    pending.set(true); error.set(None);
                    let path = path.clone(); let slug = slug.clone(); let scope = scope.clone();
                    spawn(async move {
                        match request::post(&docs::scoped(&scope, "/api/postpone"), &[("path", path.into()), ("slug", slug.into()), ("action", if postponed { "undo" } else { "later" }.into())]).await {
                            Ok(value) => {
                                frame.refresh();
                                if !postponed && let Some(next) = request::field(&value, "next").as_string()
                                    && let Some(window) = web_sys::window()
                                { let _ = window.location().set_href(&crate::project::current().here(&next)); }
                            }
                            Err(why) => error.set(Some(why)),
                        }
                        pending.set(false);
                    });
                },
                if pending() { "Saving…" } else if postponed { "Undo Later" } else { "Later" }
            }
            if let Some(why) = error() { span { role: "alert", "{why}" } }
        }
    }
}
