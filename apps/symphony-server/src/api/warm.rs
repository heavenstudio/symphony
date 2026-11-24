//! The read hub behind the data routes: published once per server, its first board read awaited.

use std::sync::Arc;
use symphony_board::lock::Recovered as _;

use crate::front::Shared;
use crate::live::Hub;

/// Publish the document hub; its owned worker completes the first board read separately.
pub(crate) async fn start(shared: &Arc<Shared>) -> Result<Arc<Hub>, String> {
    if !shared.stopping()
        && let Some(hub) = shared.api.get()
    {
        return Ok(hub.clone());
    }
    let shared = shared.clone();
    crate::spawn::blocking(move || {
        let _held = shared.lifecycle.lock().recover();
        if shared.stopping() {
            return Err("the Symphony server is stopping".into());
        }
        if let Some(hub) = shared.api.get() {
            return Ok(hub.clone());
        }
        // Only --home servers hold the shared lease. A preview must never send the owner's
        // journal to its board, even when that board is a copied scratch database.
        let hub = Hub::start(
            &shared.root,
            &shared.ui_dist,
            shared.embedded_ui,
            shared.one_server(),
        )?;
        hub.serve_at(&shared.base);
        hub.gate_recorders(shared.recording.clone());
        hub.wake_at_board(shared.board_wakers.clone());
        // The person at the machine decides under the name the gate's profiles give them.
        if let Some(folder) = shared.gate.machine() {
            hub.gates.machine_at(folder.to_path_buf());
        }
        // Saves kept for delivery by pull request are taken up again now and after each refresh.
        let settle = settling(&shared);
        settle();
        hub.after_refresh(Box::new(settle));
        assert!(
            shared.api.set(hub.clone()).is_ok(),
            "hub startup is serialized"
        );
        Ok(hub)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// What takes up again the saves `shared`'s project keeps for delivery by pull request
/// ([`symphony::save::Saves::settle`]): nothing where it keeps none, so a project that does not
/// deliver that way starts no committer for it, and nothing once the server is gone.
pub(crate) fn settling(shared: &Arc<Shared>) -> impl Fn() + Send + Sync + 'static {
    let weak = Arc::downgrade(shared);
    move || {
        if let Some(shared) = weak.upgrade()
            && symphony::commit::pending::kept(&shared.root)
        {
            shared
                .saves
                .get_or_init(|| symphony::save::Saves::start(&shared.root))
                .settle();
        }
    }
}

/// A complete first read for data routes and replacement handover.
pub(crate) async fn warm(shared: &Arc<Shared>) -> Result<Arc<Hub>, String> {
    let hub = start(shared).await?;
    hub.start_initial_board();
    hub.wait_initial_board().await?;
    Ok(hub)
}
