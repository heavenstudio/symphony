//! One request's nonrenewable authority, shared with its feed and every response byte.

use axum::http::Request;
use futures_util::{Stream, StreamExt};
use tokio::sync::watch;

use super::{Boundary, Context, Grant, Identity, Refusal, Target};

#[derive(Clone)]
pub(crate) struct Admission {
    pub(super) context: Context,
    boundary: Boundary,
    changes: watch::Receiver<()>,
    deadline: tokio::time::Instant,
}

/// The admission of a request hosted admission let in, as its mark holds it
/// (`crate::front::Admitted::Hosted`); none for a request another gate marked, or none did.
pub(crate) fn admission<B>(request: &Request<B>) -> Option<&Admission> {
    match request.extensions().get::<crate::front::Admitted>()? {
        crate::front::Admitted::Hosted(admission) => Some(admission),
        _ => None,
    }
}

impl Admission {
    /// The authority the host established for this request: who, on what, under which policy.
    pub(crate) fn context(&self) -> &Context {
        &self.context
    }

    pub(super) async fn new(
        boundary: Boundary,
        identity: Identity,
        target: Target,
    ) -> Result<Self, Refusal> {
        let changes = boundary.host.changes();
        let grant = boundary.host.authorize(&identity.actor, &target).await?;
        if grant.actor != identity.actor || grant.target != target || grant.revision.is_empty() {
            return Err(Refusal::Denied);
        }
        let valid_until = grant.valid_until.min(identity.expires_at);
        let remaining = valid_until
            .duration_since((boundary.config.clock)())
            .ok()
            .filter(|left| !left.is_zero())
            .ok_or(Refusal::Denied)?;
        let admission = Self {
            context: Context {
                actor: identity.actor,
                target,
                policy_revision: grant.revision,
                valid_until,
            },
            boundary,
            changes,
            deadline: tokio::time::Instant::now() + remaining,
        };
        admission.current()?;
        Ok(admission)
    }

    fn current(&self) -> Result<(), Refusal> {
        if self.changes.has_changed().unwrap_or(true)
            || (self.boundary.config.clock)() >= self.context.valid_until
            || tokio::time::Instant::now() >= self.deadline
        {
            Err(Refusal::Denied)
        } else {
            Ok(())
        }
    }

    /// Recheck membership before dispatch and before every response/feed chunk. A revision
    /// change ends the old context; neither a refresh nor clock rollback extends its deadline.
    pub(crate) async fn check(&self) -> Result<(), Refusal> {
        self.current()?;
        let grant: Grant = tokio::select! {
            biased;
            () = self.ended() => return Err(Refusal::Denied),
            grant = self.boundary.host.authorize(&self.context.actor, &self.context.target) => grant?,
        };
        self.current()?;
        if grant.actor != self.context.actor
            || grant.target != self.context.target
            || grant.revision != self.context.policy_revision
            || grant.valid_until < self.context.valid_until
        {
            return Err(Refusal::Denied);
        }
        Ok(())
    }

    /// An event wait with the grant's expiry as its deadline, including when a stream is idle.
    pub(crate) async fn ended(&self) {
        if self.current().is_err() {
            return;
        }
        let mut changes = self.changes.clone();
        let _ = tokio::time::timeout_at(self.deadline, changes.changed()).await;
    }

    pub(crate) fn guard<S>(&self, stream: S) -> impl Stream<Item = S::Item> + Send + 'static
    where
        S: Stream + Send + 'static,
        S::Item: Send,
    {
        futures_util::stream::unfold(
            (self.clone(), Box::pin(stream)),
            |(admission, mut stream)| async move {
                let item = tokio::select! {
                    biased;
                    () = admission.ended() => return None,
                    item = stream.next() => item?,
                };
                admission.check().await.ok()?;
                Some((item, (admission, stream)))
            },
        )
    }
}
