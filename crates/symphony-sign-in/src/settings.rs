//! The service's two files in the machine folder, beside the server's [`SERVER`]: its settings,
//! [`SETTINGS`], which hold Google's client secret, and its private key, [`KEY`], which only it
//! holds. Nothing in a project's `symphony.toml` names either.
//!
//! Each is held to the checks the server's `sign-in.toml` is, and, as it holds a secret, to one
//! more: it may not be a link, it is this account's, and no other account may read it or write it
//! (mode 600). The mode checked is the mode of the file opened, so a link swapped in after a check
//! is never read. A refusal names the file and its rule, never a value: [`Secret`] and [`Key`]
//! print nothing of what they hold.
//!
//! [`make_key`] makes the pair: the private key written once, never over another, and the line
//! `sign-in.toml` takes printed. Where the server's [`SERVER`] is in the same folder, a start
//! checks it names that key and the same origin ([`matches_server`]), since a server holding
//! another key refuses every request the service signs.

use ed25519_dalek::SigningKey;
use serde::Deserialize;
use std::fmt;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, ErrorKind, Write as _};
use std::net::SocketAddr;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::path::Path;

/// The service's settings in the machine folder.
pub const SETTINGS: &str = "sign-in-service.toml";
/// The service's private key in the machine folder: the Ed25519 seed, 64 lowercase hex digits.
pub const KEY: &str = "sign-in-service.key";
/// The server's file naming the service's public key and the origin
/// (`docs/operations/SIGNED_IN.md`), as `symphony::signed_in::FILE` names it.
pub const SERVER: &str = "sign-in.toml";

/// The longest secret taken, in bytes.
const SECRET_BYTES: usize = 4096;
/// The longest client id taken, in bytes.
const CLIENT_ID_BYTES: usize = 256;

/// A client secret. It has no `Display`, and its `Debug` says only that it is one.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Secret(String);

impl Secret {
    /// The secret itself, for the one request that sends it.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }

    /// A test's secret, `text`.
    #[cfg(test)]
    pub(crate) fn stand_in(text: &str) -> Secret {
        Secret(text.into())
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(hidden)")
    }
}

/// The OAuth client the service signs visitors in as: its id and its secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Client {
    pub(crate) id: String,
    pub(crate) secret: Secret,
}

/// The settings, every value checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Settings {
    /// Where browsers' requests arrive, plain HTTP/1.1.
    pub(crate) listen: SocketAddr,
    /// The server behind, which every request but the service's own is passed on to.
    pub(crate) upstream: SocketAddr,
    /// Where browsers reach the server through the service, as `sign-in.toml`'s `origin`.
    pub(crate) origin: String,
    /// The Google client.
    pub(crate) google: Client,
}

impl Settings {
    /// The settings `text` names: `listen`, `upstream`, `origin` and a `[google]` table of
    /// `client-id` and `client-secret`, and nothing else.
    ///
    /// # Errors
    ///
    /// It does not read as TOML, lacks one, names another, or a value breaks its rule. Said by the
    /// rule and the line, never the value.
    pub(crate) fn parse(text: &str) -> Result<Settings, String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Named {
            listen: String,
            upstream: String,
            origin: String,
            google: NamedClient,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields, rename_all = "kebab-case")]
        struct NamedClient {
            client_id: String,
            client_secret: String,
        }
        let named: Named = toml::from_str(text).map_err(|error| unread(text, &error))?;
        let address = |name: &str, value: &str| {
            value.parse::<SocketAddr>().map_err(|error| {
                format!("{name} is not an address and a port, as 127.0.0.1:8766: {error}")
            })
        };
        let listen = address("listen", &named.listen)?;
        let upstream = address("upstream", &named.upstream)?;
        if listen == upstream {
            return Err(
                "listen and upstream are one address: the service listens in front of \
                        the server, never on its port"
                    .into(),
            );
        }
        let id = named.google.client_id;
        if id.is_empty() || id.len() > CLIENT_ID_BYTES || !id.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(format!(
                "google's client-id is not 1 to {CLIENT_ID_BYTES} visible ASCII characters"
            ));
        }
        let secret = named.google.client_secret;
        if secret.is_empty()
            || secret.len() > SECRET_BYTES
            || !secret.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(format!(
                "google's client-secret is not 1 to {SECRET_BYTES} visible ASCII characters"
            ));
        }
        Ok(Settings {
            listen,
            upstream,
            origin: checked_origin(&named.origin)?,
            google: Client {
                id,
                secret: Secret(secret),
            },
        })
    }

    /// The settings the machine folder `machine` holds in its [`SETTINGS`].
    ///
    /// # Errors
    ///
    /// It is not there, or it breaks [`kept`]'s or [`Settings::parse`]'s rules. Said by its path
    /// and the rule.
    pub(crate) fn read(machine: &Path) -> Result<Settings, String> {
        let path = machine.join(SETTINGS);
        let text = kept(&path)?.ok_or_else(|| {
            format!(
                "{} is not there: write the service's settings there, mode 600 \
                 (docs/operations/SIGN_IN_SERVICE.md)",
                path.display()
            )
        })?;
        Settings::parse(&text).map_err(|why| format!("{}: {why}", path.display()))
    }
}

/// The service's private key. Its `Debug` says only that it is one.
pub(crate) struct Key(SigningKey);

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Key(hidden)")
    }
}

impl Key {
    /// The key whose seed is `seed`.
    pub(crate) fn from_seed(seed: &[u8; 32]) -> Key {
        Key(SigningKey::from_bytes(seed))
    }

    /// The key the machine folder `machine` holds in its [`KEY`].
    ///
    /// # Errors
    ///
    /// It is not there, breaks [`kept`]'s rules, or is not 64 lowercase hex digits. Said by its
    /// path and the rule.
    pub(crate) fn read(machine: &Path) -> Result<Key, String> {
        let path = machine.join(KEY);
        let text = kept(&path)?.ok_or_else(|| {
            format!(
                "{} is not there: make it with `symphony-sign-in key`",
                path.display()
            )
        })?;
        let seed = unhex(text.trim()).ok_or_else(|| {
            format!(
                "{} is not 64 lowercase hex digits: make it with `symphony-sign-in key`",
                path.display()
            )
        })?;
        Ok(Key::from_seed(&seed))
    }

    pub(crate) fn signing(&self) -> &SigningKey {
        &self.0
    }

    /// The public half as `sign-in.toml`'s `key` names it: `ed25519:` and 64 lowercase hex digits.
    pub(crate) fn public(&self) -> String {
        format!("ed25519:{}", hex(&self.0.verifying_key().to_bytes()))
    }
}

/// Makes the service's key pair in the machine folder `machine`, making the folder where it is
/// not there (mode 700): its seed, 32 bytes of the system's randomness, written as 64 lowercase
/// hex digits to a new [`KEY`], mode 600. What it answers is the line `sign-in.toml` takes.
///
/// # Errors
///
/// The key is there already (a new one would leave `sign-in.toml` naming the old), the folder or
/// the file cannot be made, or the randomness not read.
pub(crate) fn make_key(machine: &Path) -> Result<String, String> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(machine)
        .map_err(|error| format!("{}: {error}", machine.display()))?;
    let path = machine.join(KEY);
    let seed: [u8; 32] = crate::random::bytes()
        .map_err(|error| format!("the system's randomness did not read: {error}"))?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .map_err(|error| match error.kind() {
            ErrorKind::AlreadyExists => format!(
                "{} is there already: a new key would leave sign-in.toml naming the old one, so \
                 remove it first to make a new pair",
                path.display()
            ),
            _ => format!("{}: {error}", path.display()),
        })?;
    writeln!(file, "{}", hex(&seed))
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(format!("key = \"{}\"", Key::from_seed(&seed).public()))
}

/// Where the machine folder `machine` holds the server's [`SERVER`] too, whether it names `key`'s
/// public half and `origin`; nothing is asked of a folder without one, as the server may run on
/// another machine.
///
/// # Errors
///
/// It does not read, or it names another key or another origin: said by its path and which.
pub(crate) fn matches_server(machine: &Path, key: &Key, origin: &str) -> Result<(), String> {
    #[derive(Deserialize)]
    struct Named {
        key: Option<String>,
        origin: Option<String>,
    }
    let path = machine.join(SERVER);
    let text = match fs::read_to_string(&path) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("{}: {error}", path.display())),
        Ok(text) => text,
    };
    let named: Named = toml::from_str(&text).map_err(|error| {
        format!(
            "{} does not read: {}",
            path.display(),
            unread(&text, &error)
        )
    })?;
    let public = key.public();
    if named.key.as_deref() != Some(public.as_str()) {
        return Err(format!(
            "{} names another key than {KEY}'s, so the server would refuse every request the \
             service signs: put this line there: key = \"{public}\"",
            path.display()
        ));
    }
    if named.origin.as_deref() != Some(origin) {
        return Err(format!(
            "{} names another origin than {SETTINGS}, so the server would refuse every request \
             the service signs: name one origin in both",
            path.display()
        ));
    }
    Ok(())
}

/// What a TOML error in `text` says: its line, and its message where that names a key, never
/// where it could quote a value.
fn unread(text: &str, error: &toml::de::Error) -> String {
    let line = error.span().map_or(1, |span| {
        (text.as_bytes().get(..span.start))
            .map_or(1, |before| before.split(|&b| b == b'\n').count())
    });
    let message = error.message();
    let named = ["unknown field", "missing field", "duplicate key"]
        .iter()
        .any(|start| message.starts_with(start));
    if named {
        format!("line {line}: {message}")
    } else {
        format!("line {line}: a value that does not read: each is a quoted string")
    }
}

/// The text of the file at `path`: none where it is not there.
///
/// # Errors
///
/// It is a link, not a plain file, another account's, or another account may read or write it;
/// it does not read.
fn kept(path: &Path) -> Result<Option<String>, String> {
    let shown = path.display();
    let file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) if error.raw_os_error() == Some(libc::ELOOP) => {
            return Err(format!("{shown} is a link: keep the file itself there"));
        }
        Err(error) => return Err(format!("{shown}: {error}")),
        Ok(file) => file,
    };
    let meta = file
        .metadata()
        .map_err(|error| format!("{shown}: {error}"))?;
    if !meta.is_file() {
        return Err(format!("{shown} is not a file"));
    }
    if meta.uid() != effective_uid() {
        return Err(format!(
            "{shown} is another account's: keep a file of the account the service runs as"
        ));
    }
    if meta.mode() & 0o077 != 0 {
        return Err(format!(
            "{shown} may be read or written by another account, and it holds a secret: chmod 600 it"
        ));
    }
    io::read_to_string(file)
        .map(Some)
        .map_err(|error| format!("{shown}: {error}"))
}

/// The account this process runs as.
#[expect(
    unsafe_code,
    reason = "geteuid: libc's own call, which reads nothing of ours"
)]
fn effective_uid() -> u32 {
    // SAFETY: geteuid takes no argument, cannot fail and touches no memory of ours.
    unsafe { libc::geteuid() }
}

/// `bytes` as lowercase hex digits.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    })
}

/// The 32 bytes 64 lowercase hex digits `text` name: none for any other text.
fn unhex(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let digit = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    };
    let mut seed = [0; 32];
    for (at, [high, low]) in text.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        *seed.get_mut(at)? = digit(*high)? << 4 | digit(*low)?;
    }
    Some(seed)
}

/// `text` as an origin words are made for, by the server's rule (`symphony::signed_in`'s
/// `checked_origin`, repeated here so the service links no core crate; the tests hold the two the
/// same): `http://` or `https://`, a host of lowercase letters, digits, `.` and `-` or an IPv6
/// address in brackets, a port or none, and nothing after it; the scheme's own port left out, as a
/// browser never names it.
pub(crate) fn checked_origin(text: &str) -> Result<String, String> {
    const RULE: &str = "the origin is not http:// or https:// and a lowercase host, with a port or \
                        none and nothing after it, as https://board.example.com, and names no \
                        scheme's own port";
    let (authority, own_port) = match text.strip_prefix("https://") {
        Some(authority) => (authority, 443),
        None => (text.strip_prefix("http://").ok_or(RULE)?, 80),
    };
    let (host, port) = match authority.strip_prefix('[') {
        Some(rest) => {
            let (address, after) = rest.split_once(']').ok_or(RULE)?;
            let v6 = !address.is_empty()
                && (address.bytes())
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b':');
            (v6.then_some(address).ok_or(RULE)?, after)
        }
        None => match authority.find(':') {
            Some(at) => (
                authority.get(..at).ok_or(RULE)?,
                authority.get(at..).ok_or(RULE)?,
            ),
            None => (authority, ""),
        },
    };
    let named = !host.is_empty()
        && (host.bytes()).all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-' || b == b':'
        });
    let ported = port.is_empty()
        || port
            .strip_prefix(':')
            .filter(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|digits| digits.parse::<u16>().ok())
            .is_some_and(|port| port != 0 && port != own_port);
    (named && ported)
        .then(|| text.to_string())
        .ok_or_else(|| RULE.into())
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
