use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Stable tenant/project identities resolved by a trusted host, never display names or paths.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Scope {
    tenant: String,
    project: String,
}

impl Scope {
    pub fn new(tenant: &str, project: &str) -> Result<Self, Error> {
        if !plain(tenant) || !plain(project) {
            return Err(Error::Invalid);
        }
        Ok(Self {
            tenant: tenant.into(),
            project: project.into(),
        })
    }

    /// The metadata key's parts, kept separate so concatenation cannot alias two scopes.
    pub fn parts(&self) -> (&str, &str) {
        (&self.tenant, &self.project)
    }

    pub(super) fn valid(&self) -> bool {
        plain(&self.tenant) && plain(&self.project)
    }

    pub(super) fn key(&self) -> String {
        digest(&serde_json::to_vec(self).expect("a scope serializes"))
    }
}

/// Server-issued opaque identity; its bytes or a digest confer no authority.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ArtifactId(pub(super) String);

impl ArtifactId {
    pub fn parse(value: &str) -> Result<Self, Error> {
        if hex(value, 64) {
            Ok(Self(value.into()))
        } else {
            Err(Error::Invalid)
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Provider's opaque, immutable version, not an ETag or a request for the latest object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Generation(String);

impl Generation {
    pub fn new(value: &str) -> Result<Self, Failure> {
        if plain(value) {
            Ok(Self(value.into()))
        } else {
            Err(Failure::Incomplete)
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Exact committed input or delivered revision. Dirty-workspace manifests are a later integration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revision {
    repository: String,
    commit: String,
}

impl Revision {
    pub fn new(repository: &str, commit: &str) -> Result<Self, Error> {
        if !plain(repository) || !(hex(commit, 40) || hex(commit, 64)) {
            return Err(Error::Invalid);
        }
        Ok(Self {
            repository: repository.into(),
            commit: commit.into(),
        })
    }

    pub fn repository(&self) -> &str {
        &self.repository
    }
    pub fn commit(&self) -> &str {
        &self.commit
    }
    pub(super) fn valid(&self) -> bool {
        plain(&self.repository) && (hex(&self.commit, 40) || hex(&self.commit, 64))
    }
}

/// Provenance supplied by the host. Attempt identities remain distinct from artifact identities.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Producer {
    Human {
        actor: String,
    },
    /// Explicit identities for contract tests and future attempt admission, not a success receipt.
    Attempt {
        id: String,
        task: String,
        role: String,
        handle: String,
        epoch: u64,
        fence: String,
    },
}

/// Current source and producer for one operation, admitted by the host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub source: Revision,
    pub producer: Producer,
    pub operation: String,
}

/// A monotonic fence, including a cancelled operation's tombstone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Operation {
    pub revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binding: Option<Binding>,
}

/// Expected current operation identity; cancellation, restore and replacement advance its revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permit {
    pub owner: String,
    pub revision: u64,
}

/// Bounded provider outcomes. No error contains a credential, bucket key or internal URL.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Failure {
    Unknown,
    Unavailable,
    Missing,
    GenerationMismatch,
    Integrity,
    Forbidden,
    Incomplete,
}

/// A core refusal; provider and metadata uncertainty stays explicit and retryable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Conflict,
    Stale,
    Missing,
    NotAvailable,
    Protected,
    Busy,
    Metadata,
    Provider(Failure),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "artifact operation: {self:?}")
    }
}
impl std::error::Error for Error {}
impl From<Failure> for Error {
    fn from(value: Failure) -> Self {
        Self::Provider(value)
    }
}

/// Private byte storage. Every call is scoped and pinned to a server-issued operation identity.
///
/// `put` creates once, never overwrites; concurrent retries return the same generation. A lost
/// acknowledgement returns `Unknown`, reconciled by `inspect`. `read` and `delete` must never
/// substitute the latest generation. `read` returns complete bounded bytes (at most the upload
/// limit); the core hashes them rather than trusting a provider checksum or ETag. A hosted
/// implementation must document the durability boundary its acknowledged writes survive.
pub trait Provider: Send + Sync {
    fn binding(&self) -> &str;
    fn put(&self, scope: &Scope, id: &ArtifactId, bytes: &[u8]) -> Result<Generation, Failure>;
    fn inspect(&self, scope: &Scope, id: &ArtifactId) -> Result<Generation, Failure>;
    fn read(
        &self,
        scope: &Scope,
        id: &ArtifactId,
        generation: &Generation,
    ) -> Result<Vec<u8>, Failure>;
    fn delete(
        &self,
        scope: &Scope,
        id: &ArtifactId,
        generation: &Generation,
    ) -> Result<(), Failure>;
}

/// Sealed immutable byte identity. The provider binding/generation are private storage metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Descriptor {
    pub sha256: String,
    pub bytes: u64,
    pub media_type: String,
    pub name: String,
    pub provider: String,
    pub generation: Generation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Pending,
    Available,
    Failed,
    Deleting,
    Deleted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Record {
    pub id: ArtifactId,
    pub permit: Permit,
    pub binding: Binding,
    pub sha256: String,
    pub bytes: u64,
    pub media_type: String,
    pub name: String,
    pub provider: String,
    pub status: Status,
    pub intent_open: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sealed: Option<Descriptor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
}

/// An immutable reference/publication record; input and delivered revisions stay distinct.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Publication {
    pub owner: String,
    pub permit: Permit,
    pub artifacts: BTreeSet<ArtifactId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivered: Option<Revision>,
}

/// One scope's CAS aggregate. Its internals cannot be edited by callers of the core.
/// Serialization is for protected durable metadata, never for a public response.
#[derive(Clone, Serialize, Deserialize)]
pub struct Ledger {
    pub(super) format: u32,
    pub(super) operations: BTreeMap<String, Operation>,
    pub(super) records: BTreeMap<ArtifactId, Record>,
    pub(super) retries: BTreeMap<String, ArtifactId>,
    pub(super) publications: BTreeMap<String, Publication>,
    pub(super) references: BTreeMap<String, BTreeSet<ArtifactId>>,
    pub(super) exports: BTreeMap<String, BTreeSet<ArtifactId>>,
    pub(super) references_complete: bool,
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            format: 1,
            operations: BTreeMap::new(),
            records: BTreeMap::new(),
            retries: BTreeMap::new(),
            publications: BTreeMap::new(),
            references: BTreeMap::new(),
            exports: BTreeMap::new(),
            references_complete: false,
        }
    }
}

/// Revision zero means an absent ledger; a successful CAS advances it once.
pub struct Snapshot {
    pub revision: u64,
    pub ledger: Ledger,
}

/// Durable metadata, scoped by both tenant and project. CAS must be atomic across processes and
/// persist before acknowledging. A false answer is a conflict; an uncertain commit is `Metadata`
/// and the caller reloads. No implementation may replace a newer snapshot or reset a tombstone.
pub trait Metadata: Send + Sync {
    fn load(&self, scope: &Scope) -> Result<Snapshot, Error>;
    fn compare_exchange(&self, scope: &Scope, expected: u64, next: &Ledger) -> Result<bool, Error>;
}

pub(super) fn plain(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}
pub(super) fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(super) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub(super) fn random() -> Result<String, Error> {
    use std::io::Read;
    let mut bytes = [0; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|_error| Error::Provider(Failure::Unavailable))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
