//! What the web client's browser tests name of the client itself (apps/symphony-web): two values
//! its TypeScript holds, written here once and held to those sources by this file's tests, so a
//! browser test never carries a copy that has drifted.

#![forbid(unsafe_code)]

/// The src a document picture holds until it comes into view (`WAITING`, the client's
/// `src/media.tsx`): an empty picture of no size, drawn with no request.
pub const WAITING: &str =
    "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='0' height='0'/%3E";

/// How long a batch of activity toasts stays up unless dismissed, in ms (`TOAST_MS`, the client's
/// `src/panel-model.ts`).
pub const TOAST_MS: i32 = 7_000;

#[cfg(test)]
mod tests {
    use super::{TOAST_MS, WAITING};

    #[test]
    fn the_waiting_picture_is_the_clients() {
        let media = include_str!("../../symphony-web/src/media.tsx");
        assert!(
            media.contains(&format!("export const WAITING =\n  \"{WAITING}\";")),
            "src/media.tsx no longer holds WAITING as {WAITING}"
        );
    }

    #[test]
    fn a_toasts_lifetime_is_the_clients() {
        let model = include_str!("../../symphony-web/src/panel-model.ts");
        let line = model
            .lines()
            .find_map(|line| line.strip_prefix("export const TOAST_MS = "))
            .and_then(|rest| rest.strip_suffix(';'))
            .expect("src/panel-model.ts names TOAST_MS");
        assert_eq!(line.replace('_', "").parse::<i32>().ok(), Some(TOAST_MS));
    }
}
