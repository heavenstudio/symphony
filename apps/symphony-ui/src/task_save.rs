//! A task draft is cleared only by a confirmed save response.

#[cfg(any(feature = "page-panel", test))]
use serde::Deserialize;

/// What a request the server did not answer in time fails with: it may still have reached it, so a
/// page that sent something says to check before sending again, and a read says it took too long.
pub(crate) const NO_ANSWER: &str = "✖ no answer from the server; check before sending again";

/// How long a request waits for the server's response, and then again for its body.
#[cfg(target_arch = "wasm32")]
pub(crate) const ANSWER_MS: i32 = 15_000;

/// `answer` raced against `deadline`, `None` when the deadline came first. The answer is polled
/// first, so one that came in while the page was busy wins over a deadline that passed meanwhile.
pub(crate) async fn in_time<A: Future, D: Future>(answer: A, deadline: D) -> Option<A::Output> {
    let mut answer = std::pin::pin!(answer);
    let mut deadline = std::pin::pin!(deadline);
    std::future::poll_fn(|cx| {
        if let std::task::Poll::Ready(value) = answer.as_mut().poll(cx) {
            return std::task::Poll::Ready(Some(value));
        }
        deadline.as_mut().poll(cx).map(|_| None)
    })
    .await
}

/// A known refusal releases the draft; an uncertain outcome needs confirmation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Failure {
    Known(String),
    Uncertain(String),
    /// The server has no such thing (a 404): a known refusal, which a page shows as missing
    /// rather than as a failed read.
    Missing(String),
}

impl Failure {
    pub(crate) fn known(text: impl Into<String>) -> Self {
        Self::Known(text.into())
    }

    pub(crate) fn uncertain(text: impl Into<String>) -> Self {
        Self::Uncertain(text.into())
    }

    /// A refusal by the server's status: a 404 is missing, a server error leaves the outcome
    /// unknown, and any other refusal is known.
    pub(crate) fn refused(status: u16, text: impl Into<String>) -> Self {
        match status {
            404 => Self::Missing(text.into()),
            500.. => Self::Uncertain(text.into()),
            _ => Self::Known(text.into()),
        }
    }

    pub(crate) fn into_text(self) -> String {
        match self {
            Self::Known(text) | Self::Uncertain(text) | Self::Missing(text) => text,
        }
    }
}

#[derive(Deserialize)]
#[cfg(any(feature = "page-panel", test))]
pub(crate) struct Reply {
    slug: Option<String>,
    #[serde(default)]
    pending: bool,
    error: Option<String>,
}

#[cfg(any(feature = "page-panel", test))]
pub(crate) enum Outcome {
    Saved(String),
    Checking(String),
}

#[cfg(any(feature = "page-panel", test))]
impl Reply {
    pub(crate) fn outcome(self) -> Result<Outcome, Failure> {
        if self.pending {
            return Ok(Outcome::Checking(
                self.error
                    .unwrap_or_else(|| "Checking whether the task was saved…".into()),
            ));
        }
        match self.slug.filter(|slug| !slug.trim().is_empty()) {
            Some(slug) => Ok(Outcome::Saved(slug)),
            None => Err(Failure::uncertain(
                "The server did not confirm a saved task",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_confirmed_task_can_clear_a_draft() {
        let reply = |text| serde_json::from_str::<Reply>(text).unwrap();
        assert!(
            matches!(reply(r#"{"slug":"inbox/task"}"#).outcome(), Ok(Outcome::Saved(slug)) if slug == "inbox/task")
        );
        assert!(
            matches!(reply(r#"{"pending":true,"error":"still checking","slug":"inbox/task"}"#).outcome(), Ok(Outcome::Checking(why)) if why == "still checking")
        );
        assert!(
            matches!(reply(r#"{"pending":true}"#).outcome(), Ok(Outcome::Checking(why)) if why.contains("Checking whether"))
        );
        for text in [
            "{}",
            r#"{"slug":""}"#,
            r#"{"slug":"  "}"#,
            r#"{"error":"unavailable"}"#,
        ] {
            let Err(failure) = reply(text).outcome() else {
                panic!("unconfirmed task: {text}")
            };
            assert!(matches!(failure, Failure::Uncertain(why) if why.contains("did not confirm")));
        }
        let failure = Failure::known("invalid priority");
        assert!(matches!(&failure, Failure::Known(why) if why == "invalid priority"));
        assert_eq!(failure.into_text(), "invalid priority");
        assert_eq!(Failure::uncertain("timed out").into_text(), "timed out");
    }

    /// A future that stays pending until its cell holds a value, as a fetch or a timer does.
    struct Gate(std::rc::Rc<std::cell::Cell<Option<u8>>>);

    impl Future for Gate {
        type Output = u8;
        fn poll(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<u8> {
            self.0
                .take()
                .map_or(std::task::Poll::Pending, std::task::Poll::Ready)
        }
    }

    fn poll<F: Future>(future: std::pin::Pin<&mut F>) -> std::task::Poll<F::Output> {
        future.poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
    }

    /// The fetch-first ordering: an answer that is in when the page takes the race up wins over a
    /// deadline that passed meanwhile; the deadline wins only while the answer is still out.
    #[test]
    fn an_answer_that_is_in_wins_over_a_deadline_that_passed_meanwhile() {
        use std::task::Poll;
        let both = std::pin::pin!(in_time(std::future::ready(1), std::future::ready(())));
        assert_eq!(poll(both), Poll::Ready(Some(1)), "both ready: the answer");
        let late = std::pin::pin!(in_time(
            std::future::pending::<u8>(),
            std::future::ready(())
        ));
        assert_eq!(poll(late), Poll::Ready(None), "the answer still out");

        let (answer, deadline) = (std::rc::Rc::default(), std::rc::Rc::default());
        let mut race = std::pin::pin!(in_time(
            Gate(std::rc::Rc::clone(&answer)),
            Gate(std::rc::Rc::clone(&deadline))
        ));
        assert_eq!(poll(race.as_mut()), Poll::Pending, "neither in");
        answer.set(Some(7));
        deadline.set(Some(0));
        assert_eq!(
            poll(race.as_mut()),
            Poll::Ready(Some(7)),
            "both in by the next poll"
        );

        let deadline = std::rc::Rc::default();
        let mut race = std::pin::pin!(in_time(
            std::future::pending::<u8>(),
            Gate(std::rc::Rc::clone(&deadline))
        ));
        assert_eq!(poll(race.as_mut()), Poll::Pending);
        deadline.set(Some(0));
        assert_eq!(poll(race.as_mut()), Poll::Ready(None));
    }

    #[test]
    fn a_refusal_is_missing_on_404_uncertain_on_a_server_error_and_known_otherwise() {
        assert_eq!(
            Failure::refused(404, "no task 'nope'"),
            Failure::Missing("no task 'nope'".into())
        );
        assert_eq!(
            Failure::refused(500, "snapshot unavailable"),
            Failure::uncertain("snapshot unavailable")
        );
        assert_eq!(Failure::refused(503, "busy"), Failure::uncertain("busy"));
        for status in [400, 403, 409, 499] {
            assert_eq!(
                Failure::refused(status, "refused"),
                Failure::known("refused"),
                "{status}"
            );
        }
        assert_eq!(Failure::refused(404, "gone").into_text(), "gone");
    }
}
