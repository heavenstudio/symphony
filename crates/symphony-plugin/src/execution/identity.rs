use serde::{Deserialize, Serialize};

macro_rules! opaque {
    ($name:ident, $inner:ty, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name($inner);

        impl $name {
            pub fn new(value: $inner) -> Self {
                Self(value)
            }

            pub fn value(&self) -> &$inner {
                &self.0
            }
        }
    };
}

opaque!(
    AttemptId,
    [u8; 16],
    "One attempt UUID, supplied by the coordinator; never a pane or conversation."
);
opaque!(
    ResourceId,
    String,
    "An opaque provider resource name, meaningful only within its configuration."
);
opaque!(
    ConversationId,
    String,
    "An optional runtime conversation, distinct from resource and attempt identity."
);

/// Stable project and board identities, not paths, aliases or request-selected tenant names.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Scope {
    pub project: String,
    pub board: String,
}

impl Scope {
    pub fn new(project: String, board: String) -> Self {
        Self { project, board }
    }
}

/// An attempt is always addressed within its project, board and task.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AttemptRef {
    pub scope: Scope,
    pub task: String,
    pub id: AttemptId,
}

impl AttemptRef {
    pub fn new(scope: Scope, task: String, id: AttemptId) -> Self {
        Self { scope, task, id }
    }
}

/// Immutable content identified by a reference and digest; not a signed URL or storage API.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ContentRef {
    pub reference: String,
    pub digest: String,
}

impl ContentRef {
    pub fn new(reference: String, digest: String) -> Self {
        Self { reference, digest }
    }
}

/// Pinned task input; changing any of it requires a new attempt and operation key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Input {
    pub task_revision: String,
    pub digest: String,
    pub repository_revision: String,
    pub manifest: ContentRef,
}

impl Input {
    pub fn new(
        task_revision: String,
        digest: String,
        repository_revision: String,
        manifest: ContentRef,
    ) -> Self {
        Self {
            task_revision,
            digest,
            repository_revision,
            manifest,
        }
    }
}

/// A resource binding recorded by the coordinator, including its exact generation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Resource {
    pub attempt: AttemptRef,
    pub configuration: String,
    pub id: ResourceId,
    pub generation: String,
}

impl Resource {
    pub fn new(
        attempt: AttemptRef,
        configuration: String,
        id: ResourceId,
        generation: String,
    ) -> Self {
        Self {
            attempt,
            configuration,
            id,
            generation,
        }
    }
}

/// Authentication establishes the kind; an agent claiming to be an owner remains an agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PrincipalKind {
    Human,
    Service,
    Agent,
}

/// Stable authenticated identity, without credentials. It confers no grants on its own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Principal {
    pub kind: PrincipalKind,
    pub issuer: String,
    pub subject: String,
}

impl Principal {
    pub fn new(kind: PrincipalKind, issuer: String, subject: String) -> Self {
        Self {
            kind,
            issuer,
            subject,
        }
    }
}

/// A host's current authorization of exactly one operation on exactly one attempt.
///
/// Construct only after authentication and project/task authorization. Deserialization is for a
/// trusted host channel, never HTTP input, Submission, a claimed handle or a payload owner field.
/// The kit cannot authenticate its caller. A process boundary must authenticate that channel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Authorization {
    principal: Principal,
    target: AttemptRef,
    operation: super::Operation,
    policy_revision: String,
}

impl Authorization {
    pub fn from_host(
        principal: Principal,
        target: AttemptRef,
        operation: super::Operation,
        policy_revision: String,
    ) -> Self {
        Self {
            principal,
            target,
            operation,
            policy_revision,
        }
    }

    pub fn principal(&self) -> &Principal {
        &self.principal
    }
    pub fn policy_revision(&self) -> &str {
        &self.policy_revision
    }

    pub(crate) fn permits(&self, call: &super::Call) -> bool {
        self.target == call.identity.attempt && self.operation == call.request.operation()
    }
}
