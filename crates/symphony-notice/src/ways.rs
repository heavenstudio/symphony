//! The ways a notice goes by, the address each way takes, and the kinds of notice
//! (docs/engineering/NOTICES.md, "Ways and addresses" and "Kinds").
//!
//! One source for every reader of an address. The notice crate's senders check by it the address
//! a notice hands them, and the board checks by it the address a person's profile is given, so a
//! profile holds no address its sender refuses. The board carries this file as a copy
//! (`crates/tasks/src/people/ways.rs` of `crates/symphony-notice/src/ways.rs`), as the board is
//! published and the notice crate is not, held equal byte for byte by
//! `crates/symphony/tests/packages.rs`: it is std alone and names nothing of either crate.
//!
//! A refusal says the rule, never the value it was given.

/// The way a notice goes by mail: its address is an email address.
pub const EMAIL: &str = "email";
/// The way a notice goes by Slack: its address is the person's Slack member id (`U024BE7LH`).
pub const SLACK: &str = "slack";
/// The way a notice goes by WhatsApp: its address is the person's number with its country code
/// (`+15551234567`).
pub const WHATSAPP: &str = "whatsapp";

/// The ways a person's profile holds an address for, in the order they are said.
pub const WAYS: [&str; 3] = [EMAIL, SLACK, WHATSAPP];

/// The kinds of notice, by the words a message writes them in: a card waits on the person, an
/// agent's question waits on them, a reviewer gave a verdict.
pub const KINDS: [&str; 3] = ["card", "question", "verdict"];

/// What [`email`] takes, as a refusal says it.
pub const EMAIL_RULE: &str = "ASCII `local@domain`, the local part letters, digits, dots and \
     `!#$%&'*+-/=?^_`{|}~`, the domain's labels letters, digits and inner dashes";

/// What [`slack`] takes, as a refusal says it.
pub const SLACK_RULE: &str =
    "`U`, or `W` in an Enterprise organization, then capital letters and digits";

/// Whether `text` is a plain email address: `local@domain` in ASCII, the local part a dot-atom
/// (RFC 5322: no quoted string, no comment) of at most 64 characters, the domain a host name of
/// labels of letters, digits and inner dashes, at most 254 characters in all. What a header or an
/// SMTP command could read as more than one address (`<`, `>`, `,`, `;`, a space, a quote) is in
/// neither part.
pub fn email(text: &str) -> bool {
    let Some((local, domain)) = text.split_once('@') else {
        return false;
    };
    let atext = |c: char| c.is_ascii_alphanumeric() || "!#$%&'*+-/=?^_`{|}~".contains(c);
    let label = |label: &str| {
        (1..=63).contains(&label.len())
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    text.len() <= 254
        && (1..=64).contains(&local.len())
        && local
            .split('.')
            .all(|atom| !atom.is_empty() && atom.chars().all(atext))
        && domain.split('.').all(label)
}

/// Whether `text` is a Slack member id: `U`, or `W` in an Enterprise organization, then capital
/// letters and digits. A channel's id (`C…`, `G…`, `D…`) is none, so a notice is never posted to a
/// whole channel.
pub fn slack(text: &str) -> bool {
    let mut chars = text.chars();
    matches!(chars.next(), Some('U' | 'W'))
        && text.len() > 1
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// The WhatsApp number `text` is, as `+` and its digits; else why it is none. It is written with
/// its `+` and country code: without the `+`, Meta puts the country code of the number it is sent
/// from before it, which may make another person's number. Spaces, dashes and parentheses between
/// the digits are taken, as Meta takes them, and 7 to 15 digits.
///
/// # Errors
///
/// When it does not start with `+`, holds anything but digits, spaces, dashes and parentheses
/// after it, or has fewer than 7 digits or more than 15: said in words that repeat none of it.
pub fn whatsapp(text: &str) -> Result<String, &'static str> {
    let Some(rest) = text.strip_prefix('+') else {
        return Err("it does not start with `+` and its country code");
    };
    if !(rest.chars()).all(|c| c.is_ascii_digit() || matches!(c, ' ' | '-' | '(' | ')')) {
        return Err("it holds more than digits, spaces, dashes and parentheses");
    }
    let digits: String = rest.chars().filter(char::is_ascii_digit).collect();
    if !(7..=15).contains(&digits.len()) {
        return Err("it has fewer than 7 digits or more than 15");
    }
    Ok(format!("+{digits}"))
}

/// The address `text` as a profile holds it for `way`, one of [`WAYS`]: an email address and a
/// Slack member id as written, a WhatsApp number as `+` and its digits.
///
/// # Errors
///
/// When `way` is none of [`WAYS`], or `text` is no address of it: the way's rule said, never the
/// text, nor a way that is none.
pub fn address(way: &str, text: &str) -> Result<String, String> {
    match way {
        EMAIL if email(text) => Ok(text.to_string()),
        EMAIL => Err(format!("the email address is no plain one: {EMAIL_RULE}")),
        SLACK if slack(text) => Ok(text.to_string()),
        SLACK => Err(format!(
            "the Slack member id is none: {SLACK_RULE}, as U024BE7LH"
        )),
        WHATSAPP => whatsapp(text).map_err(|why| format!("the WhatsApp number is none: {why}")),
        _ => Err(format!(
            "an address is by one of {}, and that way is none of them",
            WAYS.join(", ")
        )),
    }
}
