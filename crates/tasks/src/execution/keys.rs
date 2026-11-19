//! The identities the coordinator gives out: an attempt's UUID, an operation's provider key and
//! the digests its facts name. A key is a function of what it identifies alone, so a repeat of one
//! operation sends the same key however many workers send it, and no two operations share one.

use sha2::{Digest, Sha256};
use symphony_plugin::execution::{AttemptId, Operation};

/// A new attempt's UUID, random (version 4), as its 16 bytes and its 32 hex digits.
pub(crate) fn new_attempt() -> (AttemptId, String) {
    let hex: String = crate::launch::new_uuid()
        .chars()
        .filter(|c| *c != '-')
        .collect();
    let id = parse(&hex).expect("a new UUID is 32 hex digits");
    (id, hex)
}

/// Attempt `hex`'s 16 bytes, if it is 32 hex digits.
pub(crate) fn parse(hex: &str) -> Option<AttemptId> {
    if hex.len() != 32 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(AttemptId::new(bytes))
}

/// The provider key of operation `sequence` (of kind `kind`) of attempt `attempt`, under
/// configuration `configuration` of board `board` of project `project`: the same five give the
/// same key, and a key names nothing else.
pub(crate) fn operation(
    project: &str,
    board: &str,
    configuration: &str,
    attempt: &str,
    kind: Operation,
    sequence: u64,
) -> String {
    let kind = serde_json::to_string(&kind).expect("an operation serializes");
    hashed(&[
        project,
        board,
        configuration,
        attempt,
        &kind,
        &sequence.to_string(),
    ])
}

/// The key of the `number`th cleanup of attempt `attempt`, as [`operation`] scopes its keys.
pub(crate) fn cleanup(
    project: &str,
    board: &str,
    configuration: &str,
    attempt: &str,
    number: u64,
) -> String {
    hashed(&[
        project,
        board,
        configuration,
        attempt,
        "cleanup",
        &number.to_string(),
    ])
}

/// The digest of `value`'s JSON.
pub(crate) fn digest(value: &impl serde::Serialize) -> String {
    let text = serde_json::to_string(value).expect("a request serializes");
    hashed(&[&text])
}

/// The SHA-256 of `parts`, each its length then its bytes, so no two lists of parts meet.
fn hashed(parts: &[&str]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part.as_bytes());
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_attempt_reads_back_as_its_bytes_and_two_differ() {
        let (id, hex) = new_attempt();
        assert_eq!(hex.len(), 32);
        assert_eq!(parse(&hex), Some(id));
        assert_ne!(new_attempt().1, hex);
    }

    #[test]
    fn only_32_hex_digits_read_as_an_attempt() {
        assert!(parse("00112233445566778899aabbccddeeff").is_some());
        for refused in [
            "",
            "00112233445566778899aabbccddeef",
            "00112233445566778899aabbccddeeff0",
            "0011223344556677-899aabbccddeeff",
            "g0112233445566778899aabbccddeeff",
        ] {
            assert_eq!(parse(refused), None, "{refused}");
        }
    }

    #[test]
    fn a_key_is_its_operation_alone() {
        let key = |project, configuration, attempt, kind, sequence| {
            operation(project, "board", configuration, attempt, kind, sequence)
        };
        let first = key("p", "c", "a", Operation::Start, 1);
        assert_eq!(first, key("p", "c", "a", Operation::Start, 1));
        for other in [
            key("q", "c", "a", Operation::Start, 1),
            key("p", "d", "a", Operation::Start, 1),
            key("p", "c", "b", Operation::Start, 1),
            key("p", "c", "a", Operation::Cancel, 1),
            key("p", "c", "a", Operation::Start, 2),
        ] {
            assert_ne!(first, other);
        }
        // Parts are delimited by their lengths: moving a character between parts is another key.
        assert_ne!(
            operation("ab", "c", "x", "a", Operation::Start, 1),
            operation("a", "bc", "x", "a", Operation::Start, 1)
        );
    }
}
