//! The state a one server stops beside its starter's, and the closing of its saves. A project the
//! registry no longer names as it was (renamed, moved or removed) is retired: stopped, and its
//! saves closed, at once. A request that still holds its state keeps it alive, and a stopping
//! server drains it with every project it serves, so no save outlives the server's last close.

use std::sync::{Arc, Weak};

use crate::front::Shared;

/// A project's state a stopping server stopped, with the id its stage lines name.
pub(crate) struct Stopped {
    pub id: String,
    pub shared: Arc<Shared>,
}

/// A retired project's state, which the server holds no longer: alive while a request holds it
/// or its retirement still closes its saves.
pub(super) struct Retired {
    id: String,
    shared: Weak<Shared>,
}

/// Retire `gone` beside `retired`, forgetting the retired state nothing holds any more: stop each
/// and close its saves on a thread of its own, so a request that reread the registry never waits
/// on a save in flight.
pub(super) fn retire(gone: Vec<Stopped>, retired: &mut Vec<Retired>) {
    retired.retain(|old| old.shared.strong_count() > 0);
    retired.extend(gone.iter().map(|stopped| Retired {
        id: stopped.id.clone(),
        shared: Arc::downgrade(&stopped.shared),
    }));
    if !gone.is_empty() {
        std::thread::spawn(move || {
            for stopped in &gone {
                stopped.shared.stop();
            }
            close_saves(&gone, &|_| {});
        });
    }
}

/// The retired state still alive, for the server's final drain.
pub(super) fn alive(retired: Vec<Retired>) -> impl Iterator<Item = Stopped> {
    retired.into_iter().filter_map(|old| {
        old.shared
            .upgrade()
            .map(|shared| Stopped { id: old.id, shared })
    })
}

/// Close the saves of each of `stopped`, as the server closes its starter's: a save in flight
/// ends and the queued verdict publications go out, each wait named through `stage` with its
/// project's id. Saves no request opened are opened closed, so a request still holding the state
/// is refused rather than opening them after the close; a close while another runs waits for it.
pub(crate) fn close_saves(stopped: &[Stopped], stage: &dyn Fn(&str)) {
    for Stopped { id, shared } in stopped {
        shared
            .saves
            .get_or_init(|| symphony::save::Saves::start(&shared.root))
            .close_saying(|wait| stage(&format!("{id}: {wait}")));
    }
}
