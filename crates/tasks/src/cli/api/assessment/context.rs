//! Connect-only compatibility entries retain assessment/1. A connection closure alone cannot
//! prove a selected publication target; only the native CLI supplies its effective context.

use crate::Store;
use std::io::{BufRead, Read, Write};

/// Read calls from `input`, answer each on `out`; exit 2 if the input is not a call at all.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "preserve the connect-only API entry")
)]
pub fn run<F: FnOnce() -> Result<Store, String>>(
    input: &mut dyn Read,
    out: &mut dyn Write,
    connect: F,
) -> i32 {
    super::run_in(input, out, connect, None)
}

/// Answer one call or list per nonblank input line, flushing each answer over one lazy connection.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "preserve the connect-only streaming entry")
)]
pub fn serve<F: FnOnce() -> Result<Store, String>>(
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    connect: F,
) -> i32 {
    super::serve_in(input, out, connect, None)
}
