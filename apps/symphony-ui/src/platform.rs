//! Every platform hook behind one trait, so the same app runs in the browser and, later, in the
//! desktop window (tools/python-zero/desktop implements it there).

use std::rc::Rc;

/// What the app asks of the platform it runs on.
pub trait Platform {
    /// Tell the owner something while they are not looking at the app.
    fn notify(&self, title: &str, body: &str);
    /// Open a link outside the app: a browser tab, or the system browser from the window.
    fn open_external(&self, url: &str);
    /// A value this app saved on this machine, or `None`.
    fn load(&self, key: &str) -> Option<String>;
    /// Save a small value on this machine; best effort.
    fn save(&self, key: &str, value: &str);
}

/// The platform the app was built for.
pub fn current() -> Rc<dyn Platform> {
    #[cfg(target_arch = "wasm32")]
    return Rc::new(Browser);
    #[cfg(not(target_arch = "wasm32"))]
    Rc::new(Nowhere)
}

/// No platform: every hook does nothing (the host's tests and server-side rendering).
pub struct Nowhere;

impl Platform for Nowhere {
    fn notify(&self, _title: &str, _body: &str) {}
    fn open_external(&self, _url: &str) {}
    fn load(&self, _key: &str) -> Option<String> {
        None
    }
    fn save(&self, _key: &str, _value: &str) {}
}

/// A browser tab.
#[cfg(target_arch = "wasm32")]
pub struct Browser;

#[cfg(target_arch = "wasm32")]
impl Platform for Browser {
    /// A system notification, only once the owner has allowed them and only while the tab is
    /// hidden: a page in front of them already shows it. One tag for every notice, so two open
    /// tabs, or two events in a row, leave one notification, the latest.
    fn notify(&self, title: &str, body: &str) {
        use web_sys::{Notification, NotificationOptions, NotificationPermission};
        if Notification::permission() != NotificationPermission::Granted {
            return;
        }
        let hidden = web_sys::window()
            .and_then(|window| window.document())
            .is_some_and(|document| document.hidden());
        if !hidden {
            return;
        }
        let options = NotificationOptions::new();
        options.set_body(body);
        options.set_tag("review-waiting");
        let _ = Notification::new_with_options(title, &options);
    }

    fn open_external(&self, url: &str) {
        if let Some(window) = web_sys::window() {
            let _ = window.open_with_url_and_target(url, "_blank");
        }
    }

    fn load(&self, key: &str) -> Option<String> {
        web_sys::window()?
            .local_storage()
            .ok()??
            .get_item(key)
            .ok()?
    }

    fn save(&self, key: &str, value: &str) {
        if let Some(Ok(Some(storage))) = web_sys::window().map(|window| window.local_storage()) {
            let _ = storage.set_item(key, value);
        }
    }
}
