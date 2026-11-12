//! Time as a test reads it (RULES.md `tests/no-test-fails-on-load`): a test passes or fails on what
//! the code did, never on how long a loaded machine took to let it. A bound on a cost is a bound on
//! the CPU the work took ([`cpu`]) or on a count of the work; a wait for something is a wait on its
//! condition ([`wait`]), whose backstop on the wall only a stuck program reaches, and whose failure
//! says it was a stall and the machine's load then. A development dependency of the workspace's
//! test targets; no shipped binary links it.

#![expect(
    unsafe_code,
    reason = "the CPU clocks, a process's rusage and the load average are libc calls"
)]

pub mod cpu;
pub mod wait;

pub use wait::{FLOOR, Load, Stall, backstop, load, until, until_true};
