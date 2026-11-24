//! What a refused save answers: its HTTP status and the reason, from each refusal it wraps.

use crate::{asks, edit};

pub struct Failure {
    pub status: u16,
    pub error: String,
}

/// A refused workflow operation: its failure, and the guard that refused it where one did
/// ([`symphony_api::workflow::GuardRefusal`]), which the owner's route answers beside the
/// failure's words as `guard`.
pub struct Refused {
    pub failure: Failure,
    pub guard: Option<Box<symphony_api::workflow::GuardRefusal>>,
}

impl From<Failure> for Refused {
    fn from(failure: Failure) -> Self {
        Self {
            failure,
            guard: None,
        }
    }
}

impl Failure {
    /// A write refused as `why` says, the board's words on who may decide a thing
    /// ([`crate::owners::refused_task`]): a 403, as a write lacking a power is refused, with
    /// nothing written.
    pub fn forbidden(why: &str) -> Self {
        Self {
            status: 403,
            error: format!("Forbidden: {why}: nothing was written"),
        }
    }
}

/// How the board refuses a slug no task has (`no task 'x'`), as the board's reads refuse it.
const NO_TASK: &str = symphony_board::Error::NO_TASK;

/// A refusal in its words: 404 when it names a task the board lacks, as a read of that task
/// answers, else 400.
impl From<String> for Failure {
    fn from(error: String) -> Self {
        Self {
            status: if error.starts_with(NO_TASK) { 404 } else { 400 },
            error,
        }
    }
}
impl From<edit::EditError> for Failure {
    fn from(e: edit::EditError) -> Self {
        e.0.into()
    }
}
impl From<asks::AskError> for Failure {
    fn from(e: asks::AskError) -> Self {
        let error = e.to_string();
        Self {
            status: if error.starts_with("no ask") {
                404
            } else {
                400
            },
            error,
        }
    }
}
impl From<symphony_board::Error> for Failure {
    fn from(e: symphony_board::Error) -> Self {
        match e {
            symphony_board::Error::Refused(error) => error.into(),
            symphony_board::Error::Db(error) => Self {
                status: 503,
                error: format!("the task board cannot be reached: {error}"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Failure;

    fn said(failure: Failure) -> (u16, String) {
        (failure.status, failure.error)
    }

    #[test]
    fn a_task_the_board_lacks_is_not_found_and_any_other_refusal_is_a_bad_request() {
        let missing = symphony_board::Error::Refused("no task 'tools/x'".into());
        assert_eq!(said(missing.into()), (404, "no task 'tools/x'".into()));
        assert_eq!(
            said(String::from("no task 'tools/x'").into()),
            (404, "no task 'tools/x'".into()),
            "a refusal a helper passed on as words"
        );
        for refused in [
            "priority must be an integer from 0 to 4",
            "no session 'x'; register first (`just tasks register`)",
            "a field value is a number or text, not true",
            "the task 'no task x' is done",
        ] {
            let failure: Failure = symphony_board::Error::Refused(refused.into()).into();
            assert_eq!(said(failure), (400, refused.into()));
        }
        let down: Failure = symphony_board::Error::Db("connection refused".into()).into();
        assert_eq!(
            said(down),
            (
                503,
                "the task board cannot be reached: connection refused".into()
            )
        );
    }
}
