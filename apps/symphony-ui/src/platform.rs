//! Every platform hook behind one trait, so the same app runs in the browser and, later, in the
//! desktop window (tools/python-zero/desktop implements it there).

use std::rc::Rc;

/// What the app asks of the platform it runs on.
pub trait Platform {
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
    fn open_external(&self, url: &str) {
        if let Some(window) = web_sys::window() {
            let _ = window.open_with_url_and_target(url, "_blank");
        }
    }

    /// The page's project's own value: the one server's projects share an origin
    /// ([`crate::project::stored`]).
    fn load(&self, key: &str) -> Option<String> {
        web_sys::window()?
            .local_storage()
            .ok()??
            .get_item(&crate::project::stored(&crate::project::current(), key))
            .ok()?
    }

    fn save(&self, key: &str, value: &str) {
        if let Some(Ok(Some(storage))) = web_sys::window().map(|window| window.local_storage()) {
            let key = crate::project::stored(&crate::project::current(), key);
            let _ = storage.set_item(&key, value);
        }
    }
}
