//! The shared server's part in the blocks the owner's word ends ([`symphony::decided`]): on its
//! board clock, each task blocked on a card the owner has decided since, or held on the owner's
//! approve of a tip the owner has approved, goes back to the status its block recorded; what the
//! owner said on such a card that decides nothing is told to its orchestrator once. It reads the
//! owner's lines from main's page as its last read of the documents left it, main's copies and
//! each branch's, so a save needs no hook of its own: the watcher that reads the saved document
//! again brings the owner's line to the next tick. A private server leaves the blocks alone.

use super::Hub;

impl Hub {
    /// One look at `board`, the board on `store` as this tick read it, against the owner's lines
    /// main's page holds: whether it moved a task, so the tick reads the board again.
    pub(crate) fn keep_decided(
        &self,
        store: &symphony_board::Store,
        board: &symphony_board::Board,
    ) -> bool {
        let mut kept = self.decided.lock().unwrap_or_else(|e| e.into_inner());
        let Some(kept) = kept.as_mut() else {
            return false;
        };
        let Some(said) = self
            .pages
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get("")
            .map(|page| std::sync::Arc::clone(&page.said))
        else {
            return false;
        };
        let looked = kept.look(store, &self.root, board, &said, (self.clock)());
        for line in &looked.lines {
            eprintln!("BLOCKS {line}");
        }
        looked.moved > 0
    }
}
