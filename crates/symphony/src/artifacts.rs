//! Optional immutable working evidence. Nothing selects this core for existing uploads or routes.
//!
//! A trusted host supplies an authorized [`Scope`], a durable [`Metadata`] implementation and a
//! private [`Provider`]. [`Core`] saves intent before upload, verifies the exact generation before
//! availability, and arbitrates publication, fencing and reference ownership by metadata CAS.
//! Availability grants no task success, stamp or workflow transition. The operation fence here is
//! an explicit integration seam: a future accepted-result transaction must join the real attempt
//! admission checks, rather than calling storage after accepting a result.
//!
//! These types are internal storage contracts, never an HTTP response or authorization token.
//! Current membership must be checked by the trusted host on every call, including byte reads.

mod board;
mod core;
mod local;
mod model;

pub use board::BoardMetadata;
pub use core::Core;
pub use local::Local;
pub use model::{
    ArtifactId, Binding, Descriptor, Error, Failure, Generation, Ledger, Metadata, Operation,
    Permit, Producer, Provider, Publication, Revision, Scope, Snapshot, Status,
};

#[cfg(test)]
mod tests;
