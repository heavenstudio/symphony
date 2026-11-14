//! Server-backed Later/Undo controls of the verdict editor (the queue's are the web client's,
//! apps/symphony-web/src/queue.tsx).

use crate::{cache::STALE, docs, frame::FrameRefresh, request};
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
    // While the page may show what is out of date, the card may have moved since: wait for it.
    let checking = STALE();
    let label = if pending() {
        "Saving…"
    } else if checking {
        crate::words::CHECKING
    } else if postponed {
        "Undo Later"
    } else {
        "Later"
    };
    rsx! {
        span { class: "postpone-controls",
            if postponed { small { class: "postponed", "postponed" } }
            crate::components::button::Button { r#type: "button", disabled: pending() || checking, title: "Return after the rest of this group, or in 3 days",
                onclick: move |_| {
                    if pending() || *STALE.peek() { return; }
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
                "{label}"
            }
            if let Some(why) = error() { span { role: "alert", "{why}" } }
        }
    }
}
