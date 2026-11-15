//! What the program's reads say of one that did not bring what it asked for: the server could not
//! be reached, or it answered with an error of its own, which is said with its status, so the chat
//! says what the server said rather than blaming the connection.

/// What a read fails with when no answer came at all (`request.rs`): the fetch itself failed.
pub const UNREACHABLE: &str = "The server could not be reached";

/// What a read fails with when the server answered `status` (`request.rs`): its own `error`, when
/// it gave one, then the status, so a page says what the server said rather than blaming the
/// connection.
pub fn answered(status: u16, error: Option<&str>) -> String {
    match error.map(|error| error.trim().trim_end_matches('.')) {
        Some(error) if !error.is_empty() => format!("{error} (the server answered {status})"),
        _ => format!("The server answered {status}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_read_says_the_servers_own_error_and_its_status() {
        assert_eq!(
            answered(503, Some("Board snapshot unavailable.")),
            "Board snapshot unavailable (the server answered 503)"
        );
        assert_eq!(answered(500, Some("  ")), "The server answered 500");
        assert_eq!(answered(502, None), "The server answered 502");
    }
}
