//! The shared server's part in the owner's asks, on its board clock after the chat journal is
//! handed over: the open asks whose answers the documents or the ask store hold closed, and those
//! whose card, comment, worktree or thread is gone marked so and left open
//! ([`symphony::owner_asks`]), when the documents or the ask store changed and a minute after it
//! last looked. A private server leaves them alone. No start moves anything else: the verdict tasks
//! filed before asks become asks only by `symphony verdict-asks`, run once in a project's main
//! checkout ([`symphony::verdicts::to_asks`]). Every server, a private one too, reads the open asks
//! each tick for the agent queue's rows ([`Hub::note_asks`]): who holds each and since when.

use crate::lock::Held as _;
use std::sync::atomic::Ordering;

use super::{Hub, measured};
use symphony::owner_asks::{self, Due, Reading};

/// What the shared server keeps of the owner's asks from one tick to the next.
#[derive(Default)]
pub(crate) struct Kept {
    due: Due,
}

impl Hub {
    /// Tell the agent queue's cards the owner's open asks as the board on `store` holds them now:
    /// who holds each and since when. Every server does, a private one and one behind the board's
    /// schema too, as it only reads; a read that fails leaves the rows as they were.
    pub(super) fn note_asks(&self, store: &symphony_board::Store) {
        let open = match measured("queue asks", || store.asks(None)) {
            Ok(open) => open,
            Err(error) => {
                eprintln!("ASKS not read for the queue: {error}");
                return;
            }
        };
        {
            let mut kept = self.queue_asks.lock().held();
            if **kept == open {
                return;
            }
            *kept = std::sync::Arc::new(open.clone());
        }
        let mut pages = self.pages.write().held();
        for page in pages.values_mut() {
            let mut agent = page.queue.agent.clone();
            owner_asks::note(&mut agent, &open, &page.held);
            let mut changes = Vec::new();
            super::diff_items(
                &mut changes,
                "agent-queue",
                &page.queue.agent,
                &agent,
                |c| c.href.clone(),
            );
            page.queue.agent = agent;
            self.publish(page, changes);
        }
    }

    pub(super) fn keep_asks(&self, store: &symphony_board::Store) {
        let mut kept = self.owner_asks.lock().held();
        let Some(kept) = kept.as_mut() else {
            return;
        };
        let read_at = self.pages.read().held().get("").map(|page| page.read_at);
        let Some(read_at) = read_at else {
            return;
        };
        if !kept.due.now(&self.root, read_at, (self.clock)()) {
            return;
        }
        let open = match measured("asks", || store.asks(None)) {
            Ok(open) => open,
            Err(error) => {
                eprintln!("ASKS not read: {error}");
                return;
            }
        };
        if open.is_empty() {
            return;
        }
        let chats = if open.iter().any(|ask| ask.kind == "chat") {
            owner_asks::chats(&self.root)
                .map_err(|error| eprintln!("ASKS chat threads not read: {error}"))
                .ok()
        } else {
            None
        };
        // Who answered a card or a comment: without it an answer would be credited to no one, so
        // the tick waits for a read that has it.
        let replies = if open
            .iter()
            .any(|ask| matches!(ask.kind.as_str(), "verdict" | "message" | "comment"))
        {
            match owner_asks::replies(&self.root) {
                Ok(replies) => replies,
                Err(error) => {
                    eprintln!("ASKS replies not read: {error}");
                    return;
                }
            }
        } else {
            Vec::new()
        };
        // The queue, the cards and when they were read, together: a refresh since the check reads
        // later lines.
        let Some((agent, held, read_at, unscanned)) =
            self.pages.read().held().get("").map(|page| {
                (
                    page.queue.agent.clone(),
                    std::sync::Arc::clone(&page.held),
                    page.read_at,
                    (*page.unscanned).clone(),
                )
            })
        else {
            return;
        };
        let reading = Reading {
            root: self.root.clone(),
            agent,
            held,
            read_at,
            trees: self.trees.read().held().clone(),
            listed: self.listed.load(Ordering::SeqCst),
            unscanned,
            chats,
            replies,
        };
        for line in owner_asks::keep(store, &open, &reading) {
            eprintln!("ASKS {line}");
        }
    }
}
