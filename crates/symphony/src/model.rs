//! A hull model card: [`is_model`].
//!
//! One card per hull model version, in the register of `docs/art/HULL_MODELS.md`. Only the owner
//! decides one: the triage lists it and never auto-approves it. Its text carries `*Model:* <HULL> v<N>`.

use std::sync::LazyLock;

use regex::Regex;

use crate::register::Decision;
use crate::text::S;

/// The tag counts only where `post` writes it: at the very end of the card line, before its own
/// `*Replaces:*`.
static MODEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"\*Model:\*{S}*(?P<hull>[A-Z]{{3}}-[A-Z]+\d+){S}+v(?P<version>\d+)(?:{S}+\*Replaces:\*{S}+[a-z0-9][a-z0-9-]*)?{S}*$"
    ))
    .unwrap()
});

/// (hull, version) a model card is for, or `None` for any other card.
pub fn model(card: &Decision) -> Option<(String, u32)> {
    let found = MODEL.captures(&card.text)?;
    Some((found["hull"].to_string(), found["version"].parse().ok()?))
}

pub fn is_model(card: &Decision) -> bool {
    model(card).is_some()
}
