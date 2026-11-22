//! The combined structural and session marker identifies cached board inputs.

use super::postgres::snapshot as pg;
use super::statement::both;
use super::surreal::snapshot as sql;
use super::{Board, Error, Snapshot, Store, View, Which, reading};
use serde_json::json;

impl Store {
    /// The mutation marker and snapshot are read in the same transaction. Cached inputs are
    /// reinterpreted through View on every tick so health, reservations and presence still age.
    pub(super) fn callback_board(&self) -> Result<(Board, u64), Error> {
        let marker = self.board_marker()?;
        let mut cached = self
            .callback_snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(snapshot) = cached.as_ref()
            && snapshot.callback_sequence == marker
        {
            return Ok((Self::board_view(View::new(self, snapshot.clone())), marker));
        }
        let (body, vars) = (sql::board(), reading::vars(&Which::All));
        let snapshot: Snapshot =
            self.retrying(|| self.transact(both(&body, pg::board()), vars.clone()))?;
        let sequence = snapshot.callback_sequence;
        *cached = Some(snapshot.clone());
        Ok((Self::board_view(View::new(self, snapshot)), sequence))
    }

    pub(super) fn board_marker(&self) -> Result<u64, Error> {
        let marker = sql::marker();
        self.retrying(|| self.read(both(&marker, pg::marker()), json!({})))
    }

    /// The board's marker: it moves with every commit a reader can see that changes what a board
    /// read shows, and with no other.
    pub fn marker(&self) -> Result<u64, Error> {
        self.board_marker()
    }

    /// Read cached board inputs, refreshing only when a committed board mutation moved the marker.
    pub fn clock_board(&self) -> Result<Board, Error> {
        self.callback_board().map(|(board, _)| board)
    }
}
