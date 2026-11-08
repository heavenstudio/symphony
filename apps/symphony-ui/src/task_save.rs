//! A task draft is cleared only by a confirmed save response.

#[cfg(any(feature = "page-panel", test))]
use serde::Deserialize;

/// A known refusal releases the draft; an uncertain outcome needs confirmation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Failure {
    Known(String),
    Uncertain(String),
}

impl Failure {
    pub(crate) fn known(text: impl Into<String>) -> Self {
        Self::Known(text.into())
    }

    pub(crate) fn uncertain(text: impl Into<String>) -> Self {
        Self::Uncertain(text.into())
    }

    pub(crate) fn into_text(self) -> String {
        match self {
            Self::Known(text) | Self::Uncertain(text) => text,
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
}
