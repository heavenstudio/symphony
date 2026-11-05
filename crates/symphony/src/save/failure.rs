//! What a refused save answers: its HTTP status and the reason, from each refusal it wraps.

use crate::{asks, edit};

pub struct Failure {
    pub status: u16,
    pub error: String,
}
impl From<String> for Failure {
    fn from(error: String) -> Self {
        Self { status: 400, error }
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
