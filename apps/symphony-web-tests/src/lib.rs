//! What the web client's browser tests name of the client itself (apps/symphony-web): values its
//! TypeScript holds, written here once and held to those sources by this file's tests, so a
//! browser test never carries a copy that has drifted.

#![forbid(unsafe_code)]

/// The src a document picture holds until it comes into view (`WAITING`, the client's
/// `src/media-picture.tsx`): an empty picture of no size, drawn with no request.
pub const WAITING: &str =
    "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='0' height='0'/%3E";

/// How long a batch of activity toasts stays up unless dismissed, in ms (`TOAST_MS`, the client's
/// `src/panel-model.ts`).
pub const TOAST_MS: i32 = 7_000;

/// How long a failed read of a document's questions, or of the chat's thread, waits before it is
/// asked again, in ms (`RETRY_MS`, the client's `src/asks.tsx` and `src/quick.tsx` alike).
pub const RETRY_MS: i32 = 3_000;

/// The longest a page stays under its skeleton waiting for its first screen's reads, in ms
/// (`SETTLE_MS`, the client's `src/viewport.tsx`).
pub const SETTLE_MS: i32 = 3_000;

#[cfg(test)]
mod tests {
    use super::{RETRY_MS, SETTLE_MS, TOAST_MS, WAITING};

    /// The value `source` exports as `export const NAME = …;`, its digits' underscores dropped.
    fn exported(source: &str, name: &str) -> Option<i32> {
        let prefix = format!("export const {name} = ");
        source
            .lines()
            .find_map(|line| line.strip_prefix(prefix.as_str()))
            .and_then(|rest| rest.strip_suffix(';'))
            .and_then(|value| value.replace('_', "").parse().ok())
    }

    #[test]
    fn the_waiting_picture_is_the_clients() {
        let media = include_str!("../../symphony-web/src/media-picture.tsx");
        assert!(
            media.contains(&format!("export const WAITING =\n  \"{WAITING}\";")),
            "src/media-picture.tsx no longer holds WAITING as {WAITING}"
        );
    }

    #[test]
    fn a_toasts_lifetime_is_the_clients() {
        let model = include_str!("../../symphony-web/src/panel-model.ts");
        assert_eq!(exported(model, "TOAST_MS"), Some(TOAST_MS));
    }

    #[test]
    fn the_asks_and_the_chats_retry_is_the_clients() {
        for (file, source) in [
            (
                "src/asks.tsx",
                include_str!("../../symphony-web/src/asks.tsx"),
            ),
            (
                "src/quick.tsx",
                include_str!("../../symphony-web/src/quick.tsx"),
            ),
        ] {
            assert_eq!(exported(source, "RETRY_MS"), Some(RETRY_MS), "{file}");
        }
    }

    #[test]
    fn a_pages_settle_deadline_is_the_clients() {
        let viewport = include_str!("../../symphony-web/src/viewport.tsx");
        assert_eq!(exported(viewport, "SETTLE_MS"), Some(SETTLE_MS));
    }

    #[test]
    fn an_exported_value_is_read_from_its_line_alone() {
        let source = "const TOAST_MS = 1;\nexport const TOAST_MS = 7_000;\nexport const NEXT = 2;";
        assert_eq!(exported(source, "TOAST_MS"), Some(7_000));
        assert_eq!(exported(source, "NEXT"), Some(2));
        assert_eq!(exported(source, "GONE"), None);
        assert_eq!(
            exported("export const ODD = 3", "ODD"),
            None,
            "no semicolon"
        );
        assert_eq!(exported("export const WORD = \"x\";", "WORD"), None);
    }
}
