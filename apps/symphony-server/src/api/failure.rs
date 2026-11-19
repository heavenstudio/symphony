//! HTTP statuses for board read failures.

use axum::http::StatusCode;

pub(crate) fn task_failure(error: symphony_board::Error) -> (StatusCode, String) {
    match error {
        symphony_board::Error::Refused(_) => (StatusCode::NOT_FOUND, error.to_string()),
        symphony_board::Error::Db(_) => unavailable(&error.to_string(), ""),
    }
}

/// A read route's board failure ([`symphony::taskview::read_failure`]): a board that answered too
/// late is slow, 504; any other failure is 503, its words after `unreachable`.
pub(crate) fn unavailable(error: &str, unreachable: &str) -> (StatusCode, String) {
    let (status, said) = symphony::taskview::read_failure(error, unreachable);
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::SERVICE_UNAVAILABLE);
    (status, said)
}
