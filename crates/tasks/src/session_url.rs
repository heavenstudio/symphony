//! A session's web address: where Go to session links for an agent that runs on the web (a cloud
//! agent, a Claude Code session with a remote link), set at `register --session-url` or later with
//! `callback --session-url` (docs/workflow/TASKS.md, "Going to a session"). The board keeps it off the
//! `session` row, which an older reader refuses once a row carries a field it does not know, in the
//! schemaless `callback_state` as `['session_url', HANDLE]`; `leave` deletes it.

/// The longest address kept, in bytes.
pub const MAX_BYTES: usize = 2048;

/// `given` as the board keeps it: `None` for an empty one, which clears the address; else an
/// `http://` or `https://` URL naming a host, with no space or control character in it, at most
/// [`MAX_BYTES`] long. Anything else is refused, saying why: a page links to it as it is, so no
/// `javascript:` or other scheme ever reaches one.
pub fn checked(given: &str) -> Result<Option<String>, String> {
    let url = given.trim();
    if url.is_empty() {
        return Ok(None);
    }
    if url.len() > MAX_BYTES {
        return Err(format!(
            "a session URL is at most {MAX_BYTES} bytes; this one is {}",
            url.len()
        ));
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("a session URL holds no space or control character".into());
    }
    let lower = url.to_ascii_lowercase();
    let Some(rest) = ["https://", "http://"]
        .iter()
        .find_map(|scheme| lower.strip_prefix(scheme))
    else {
        return Err(format!(
            "a session URL starts with https:// or http://, not {:?}",
            url.split(':').next().unwrap_or(url)
        ));
    };
    if rest.split(['/', '?', '#']).next().is_none_or(str::is_empty) {
        return Err("a session URL names a host after its scheme".into());
    }
    Ok(Some(url.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{MAX_BYTES, checked};

    #[test]
    fn a_web_address_is_kept_as_given_less_the_space_around_it() {
        for (given, kept) in [
            (
                "https://claude.ai/code/session_01abc",
                "https://claude.ai/code/session_01abc",
            ),
            (
                "  http://mac.tail.ts.net:8080/x?y=1#z \n",
                "http://mac.tail.ts.net:8080/x?y=1#z",
            ),
            ("HTTPS://Example.com", "HTTPS://Example.com"),
            ("https://h", "https://h"),
        ] {
            assert_eq!(checked(given), Ok(Some(kept.to_string())), "{given:?}");
        }
    }

    #[test]
    fn an_empty_address_clears_it() {
        assert_eq!(checked(""), Ok(None));
        assert_eq!(checked("  \t"), Ok(None));
    }

    #[test]
    fn another_scheme_no_host_a_space_or_a_control_character_is_refused() {
        for (given, why) in [
            (
                "javascript:alert(1)",
                "starts with https:// or http://, not \"javascript\"",
            ),
            ("file:///etc/passwd", "starts with https:// or http://"),
            ("claude.ai/code/x", "starts with https:// or http://"),
            ("ftp://host/x", "not \"ftp\""),
            ("https://", "names a host"),
            ("https:///path", "names a host"),
            ("https://?q", "names a host"),
            ("https://a b", "no space or control character"),
            ("https://a\u{7}b", "no space or control character"),
            ("https://a\u{a0}b", "no space or control character"),
        ] {
            let refused = checked(given).unwrap_err();
            assert!(refused.contains(why), "{given:?}: {refused}");
        }
    }

    #[test]
    fn an_address_longer_than_the_cap_is_refused_and_one_at_it_kept() {
        let at = format!("https://h/{}", "a".repeat(MAX_BYTES - "https://h/".len()));
        assert_eq!(at.len(), MAX_BYTES);
        assert_eq!(checked(&at), Ok(Some(at.clone())));
        let over = format!("{at}a");
        let refused = checked(&over).unwrap_err();
        assert!(
            refused.contains(&format!("at most {MAX_BYTES} bytes")),
            "{refused}"
        );
    }
}
