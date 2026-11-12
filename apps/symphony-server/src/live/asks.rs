//! The shared server's part in the owner's asks, on its board clock after the chat journal is
//! handed over: the verdict tasks filed before asks turned into asks once
//! ([`symphony::verdicts::to_asks`]), and the comments already open too when
//! [`symphony::owner_asks::COMMENTS_OPEN_BEFORE`] says so; then the open asks whose answers the
//! documents or the ask store hold closed ([`symphony::owner_asks`]), when the documents or the ask
//! store changed and a minute after it last looked. A private server leaves them alone.

use super::{Hub, measured};
use symphony::owner_asks::{self, Due, Reading};

/// What the shared server keeps of the owner's asks from one tick to the next.
#[derive(Default)]
pub(crate) struct Kept {
    /// Whether this process has turned the verdict tasks into asks.
    turned: bool,
    due: Due,
}

impl Hub {
    pub(super) fn keep_asks(&self, store: &symphony_board::Store) {
        let mut kept = self.asks.lock().unwrap_or_else(|e| e.into_inner());
        let Some(kept) = kept.as_mut() else {
            return;
        };
        if !kept.turned {
            kept.turned = true;
            let by = || symphony::verdicts::app_handle(store).map_err(|e| e.to_string());
            match measured("verdict tasks", || {
                symphony::verdicts::to_asks(&self.root, store, by)
            }) {
                Ok(done) => {
                    for line in done {
                        eprintln!("ASKS {line}");
                    }
                }
                Err(error) => eprintln!("ASKS verdict tasks not turned into asks: {error}"),
            }
            if owner_asks::COMMENTS_OPEN_BEFORE {
                let agent = self
                    .pages
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .get("")
                    .map(|page| page.queue.agent.clone())
                    .unwrap_or_default();
                match owner_asks::open_comments(&self.root, store, &agent) {
                    Ok(opened) => {
                        for line in opened {
                            eprintln!("ASKS {line}");
                        }
                    }
                    Err(error) => eprintln!("ASKS open comments not asked: {error}"),
                }
            }
        }
        let read_at = self
            .pages
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get("")
            .map(|page| page.read_at);
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
        // The queue and when it was read, together: a refresh since the check reads later lines.
        let Some((agent, read_at)) = self
            .pages
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get("")
            .map(|page| (page.queue.agent.clone(), page.read_at))
        else {
            return;
        };
        let reading = Reading {
            root: self.root.clone(),
            agent,
            read_at,
            trees: self.trees.read().unwrap_or_else(|e| e.into_inner()).clone(),
            chats,
        };
        for line in owner_asks::close_answered(store, &open, &reading) {
            eprintln!("ASKS {line}");
        }
    }
}
