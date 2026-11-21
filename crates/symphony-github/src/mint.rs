//! The installation token of a GitHub App, made here where the settings name the App's
//! installation and private key beside a token file (docs/GITHUB.md, "Tokens it makes").
//!
//! A token file the settings give no App is read as it is ([`credential::read`]): its token is
//! sent, and one GitHub expired is answered 401. A token file they give an App is the program's
//! own. It holds the token on its first line, `expires_at` with GitHub's time on its second and
//! the permissions it was asked for on its third ([`asked`]), and [`token`] makes a new one when
//! the file is absent, expired, within [`MARGIN`] of expiring, holds no expiry this program
//! wrote, or was made for permissions other than the settings ask for now: a JSON Web Token signed by the App's key, sent to
//! `POST /app/installations/ID/access_tokens`, asking for the one repository the settings name
//! and the permissions of [`permissions`] alone.
//!
//! The JWT is signed by `openssl`, which reads the key from its file, so the key's bytes never
//! enter this program, and what `openssl` says is never shown. The JWT goes to `curl` by its
//! standard input, as every token does ([`crate::transport`]). The token GitHub answers with is
//! written to a file of its own beside the token file, readable by its owner alone, then renamed
//! over it: a reader finds the old token or the new, and two runs at once leave one of two valid
//! ones. A failure writes nothing, leaves the file as it was, and names what to put right
//! without a key, a JWT or a token.

use crate::config::{App, Config, PullRequests, Releases, Verification};
use crate::credential::{self, Secret};
use crate::reconcile::Context;
use crate::transport::{Failure, Request};
use data_encoding::BASE64URL_NOPAD;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

/// How long before its expiry a token is made again: a token used by a request that takes this
/// long still holds when GitHub reads it.
pub const MARGIN: i64 = 300;
/// How far back a JWT says it was made, for a clock behind GitHub's.
const SKEW: i64 = 60;
/// How long a JWT holds from now: GitHub takes none of more than ten minutes, counted from its
/// `iat`.
const LIFE: i64 = 540;
/// The fewest bytes of a signature by an RSA key of 2048 bits, the size GitHub makes.
const RSA_2048: usize = 256;

/// Why no token is had: what a person must put right, or what may be tried again later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    Refused(String),
    Unavailable(String),
}

impl Stop {
    /// The line it says.
    #[must_use]
    pub fn said(self) -> String {
        match self {
            Stop::Refused(why) | Stop::Unavailable(why) => why,
        }
    }
}

/// When GitHub expires a token: seconds since 1970, and the time as GitHub wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expiry {
    pub at: i64,
    pub text: String,
}

impl Expiry {
    /// `text`, GitHub's `YYYY-MM-DDTHH:MM:SSZ`; none for any other.
    #[must_use]
    pub fn parse(text: &str) -> Option<Expiry> {
        let at = seconds(text)?;
        Some(Expiry {
            at,
            text: text.into(),
        })
    }
}

/// Seconds since 1970 of `YYYY-MM-DDTHH:MM:SSZ`.
pub(crate) fn seconds(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    let shape = bytes.len() == 20
        && (bytes.iter().enumerate()).all(|(at, byte)| match at {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 | 16 => *byte == b':',
            19 => *byte == b'Z',
            _ => byte.is_ascii_digit(),
        });
    if !shape {
        return None;
    }
    let number = |from: usize, to: usize| text.get(from..to)?.parse::<i64>().ok();
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    if second > 60 {
        return None;
    }
    // Days from 1970-01-01 to the date (the proleptic Gregorian calendar, years from March).
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    let of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let days = era * 146_097 + of_era * 365 + of_era / 4 - of_era / 100 + of_year - 719_468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// What a token file the program keeps holds.
#[derive(Clone, Debug)]
pub enum Held {
    /// No file there.
    Absent,
    /// No token and expiry this program wrote: a token kept by hand, or nothing that reads.
    Unknown,
    /// A token, when GitHub expires it, and the permissions it was asked for ([`asked`]), where
    /// the file says them: a file an older build wrote does not.
    Token {
        token: Secret,
        expires: Expiry,
        permissions: Option<String>,
    },
}

impl Held {
    /// Whether its token holds for [`MARGIN`] past `now`.
    #[must_use]
    pub fn current(&self, now: i64) -> bool {
        matches!(self, Held::Token { expires, .. } if expires.at - now > MARGIN)
    }

    /// Whether its token holds for [`MARGIN`] past `now` and was asked for `asked`, the
    /// permissions the settings ask for now ([`asked`]): one made for others is made again.
    #[must_use]
    pub fn serves(&self, now: i64, asked: &str) -> bool {
        self.current(now)
            && matches!(self, Held::Token { permissions: Some(made), .. } if made == asked)
    }
}

/// What the file's `text` holds: its token on the first line, `expires_at TIME` on the second,
/// and `permissions ASKED` on a third where the file says them.
fn held(text: &str) -> Held {
    let mut lines = text.lines();
    let (Some(token), Some(expires)) = (lines.next(), lines.next()) else {
        return Held::Unknown;
    };
    let permissions = match (lines.next(), lines.next()) {
        (None, _) => None,
        (Some(line), None) => match line.strip_prefix("permissions ") {
            Some(asked) if !asked.is_empty() => Some(asked.to_string()),
            _ => return Held::Unknown,
        },
        (Some(_), Some(_)) => return Held::Unknown,
    };
    let expires = expires.strip_prefix("expires_at ").and_then(Expiry::parse);
    match (Secret::new(token), expires) {
        (Ok(token), Some(expires)) => Held::Token {
            token,
            expires,
            permissions,
        },
        _ => Held::Unknown,
    }
}

/// Where `file`, the settings' `key`, is: the file itself where it is there, else its name in
/// its folder; in neither case inside a folder of the repository.
fn place(file: &Path, folders: &[&Path], key: &str) -> Result<PathBuf, Stop> {
    let named = file.display();
    let refused = |why: String| Stop::Refused(format!("{key} {named}{why}"));
    if !file.is_absolute() {
        return Err(refused(" is not an absolute path".into()));
    }
    let real = match file.canonicalize() {
        Ok(real) => real,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let (Some(folder), Some(name)) = (file.parent(), file.file_name()) else {
                return Err(refused(" names no file".into()));
            };
            let folder = (folder.canonicalize())
                .map_err(|error| refused(format!(": its folder: {error}")))?;
            folder.join(name)
        }
        Err(error) => return Err(refused(format!(": {error}"))),
    };
    outside(&real, folders)
        .map_err(|()| refused(" is inside the repository; keep it outside".into()))?;
    Ok(real)
}

/// `real` is in none of `folders`.
fn outside(real: &Path, folders: &[&Path]) -> Result<(), ()> {
    for folder in folders {
        let folder = (folder.canonicalize()).unwrap_or_else(|_| folder.to_path_buf());
        if real.starts_with(&folder) {
            return Err(());
        }
    }
    Ok(())
}

/// What the token file at `real`, the settings' `key`, holds: refused where it is no plain file
/// or others may read it, as a token file kept by hand is.
fn kept(real: &Path, key: &str) -> Result<Held, Stop> {
    let named = real.display();
    let meta = match fs::metadata(real) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Held::Absent),
        Err(error) => return Err(Stop::Refused(format!("{key} {named}: {}", error.kind()))),
    };
    if !meta.is_file() {
        return Err(Stop::Refused(format!("{key} {named} is not a file")));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(Stop::Refused(format!(
            "{key} {named} is readable by others; chmod 600 it"
        )));
    }
    let text = fs::read_to_string(real)
        .map_err(|error| Stop::Refused(format!("{key} {named}: {}", error.kind())))?;
    Ok(held(&text))
}

/// What `file`, the token file, holds now, read and never made, for `tokens` to say:
/// [`Held::Unknown`] where the settings give it no App and its token reads.
///
/// # Errors
///
/// The file is refused, as a run that uses it would refuse it.
pub fn standing(ctx: &Context<'_>, config: &Config, file: &Path) -> Result<Held, String> {
    let folders: Vec<&Path> = ctx.repository.iter().map(PathBuf::as_path).collect();
    if config.app().is_none() {
        return credential::read(file, &folders).map(|_| Held::Unknown);
    }
    let real = place(file, &folders, TOKEN_FILE).map_err(Stop::said)?;
    kept(&real, TOKEN_FILE).map_err(Stop::said)
}

/// The settings' key of the file the token is kept in.
pub const TOKEN_FILE: &str = "token_file";

/// The permissions the token is asked for, each the least the calls it is sent with need
/// (docs/GITHUB.md, "The token"): a delivery's pushes, pull requests and checks, a release's
/// contents, a hosted verification's workflow run, and the issues the exchange reads or writes.
#[must_use]
pub fn permissions(config: &Config) -> BTreeMap<&'static str, &'static str> {
    let mut asked = BTreeMap::new();
    if config.pull_requests == PullRequests::Publish {
        asked.insert("contents", "write");
        asked.insert("pull_requests", "write");
        asked.insert("checks", "write");
    }
    if config.releases == Releases::Publish {
        asked.insert("contents", "write");
    }
    if config.verification == Verification::Hosted {
        asked.insert("actions", "write");
    }
    if config.issues.exports() || config.close_issues {
        asked.insert("issues", "write");
    } else if config.issues.imports() {
        asked.insert("issues", "read");
    }
    if asked.is_empty() {
        asked.insert("metadata", "read");
    }
    asked
}

/// The [`permissions`] the settings ask for, as the token file says them:
/// `checks=write,contents=write,pull_requests=write`, each `NAME=ACCESS` in name order.
#[must_use]
pub fn asked(config: &Config) -> String {
    (permissions(config).iter())
        .map(|(name, access)| format!("{name}={access}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// The token `token_file` holds, made first where the settings give it an App and the
/// token it holds is not current: absent, expired, within [`MARGIN`] of expiring, of no
/// expiry this program wrote, or asked for permissions other than the settings ask for now
/// ([`Held::serves`]). A file they give no App is read as it is.
///
/// # Errors
///
/// What a person must put right ([`Stop::Refused`]), or GitHub gave no token now
/// ([`Stop::Unavailable`]); the file is then as it was.
pub fn token(ctx: &Context<'_>, config: &Config) -> Result<Secret, Stop> {
    let key = TOKEN_FILE;
    let file = (config.token_file.as_deref())
        .ok_or_else(|| Stop::Refused(format!("the settings name no {key}")))?;
    let folders: Vec<&Path> = ctx.repository.iter().map(PathBuf::as_path).collect();
    let Some(app) = config.app() else {
        return credential::read(file, &folders).map_err(Stop::Refused);
    };
    let real = place(file, &folders, key)?;
    let held = kept(&real, key)?;
    let asked = asked(config);
    if let Held::Token { token, .. } = &held
        && held.serves(ctx.now, &asked)
    {
        return Ok(token.clone());
    }
    let key_file = key_file(&app, &folders, &real)?;
    let (token, expires) = make(ctx, config, &app, &key_file)?;
    write(&real, &token, &expires, &asked, key)?;
    Ok(token)
}

/// The App's key file, held as a token file is: outside the repository, a plain file that only
/// its owner may read, and never the token file `token_file` it makes the token of.
fn key_file(app: &App, folders: &[&Path], token_file: &Path) -> Result<PathBuf, Stop> {
    let key = "key_file";
    let named = app.key_file.display();
    let refused = |why: String| Stop::Refused(format!("{key} {named}{why}"));
    let real = (app.key_file.canonicalize()).map_err(|error| refused(format!(": {error}")))?;
    outside(&real, folders)
        .map_err(|()| refused(" is inside the repository; keep it outside".into()))?;
    if real == token_file {
        return Err(refused(format!(
            " is {TOKEN_FILE}: a token made would be written over the key"
        )));
    }
    let meta = fs::metadata(&real).map_err(|error| refused(format!(": {}", error.kind())))?;
    if !meta.is_file() {
        return Err(refused(" is not a file".into()));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(refused(" is readable by others; chmod 600 it".into()));
    }
    Ok(real)
}

/// A JSON Web Token of App `id`, made at `now` and signed with the key at `key_file` by
/// `openssl`.
///
/// # Errors
///
/// `openssl` is not there, or did not sign with that key: a line that holds nothing it said.
pub fn jwt(openssl: &str, id: i64, key_file: &Path, now: i64, key: &str) -> Result<String, Stop> {
    let header = BASE64URL_NOPAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
    let claims = json!({"iat": now - SKEW, "exp": now + LIFE, "iss": id.to_string()});
    let input = format!(
        "{header}.{}",
        BASE64URL_NOPAD.encode(claims.to_string().as_bytes())
    );
    let signature = sign(openssl, key_file, input.as_bytes(), key)?;
    Ok(format!("{input}.{}", BASE64URL_NOPAD.encode(&signature)))
}

/// `input` signed with RS256 by the key at `key_file`, the settings' `key`: `openssl dgst
/// -sha256 -sign`, which reads the key itself, never asks for a passphrase, and whose standard
/// error is never read.
fn sign(openssl: &str, key_file: &Path, input: &[u8], key: &str) -> Result<Vec<u8>, Stop> {
    let named = key_file.display();
    let mut child = Command::new(openssl)
        .args(["dgst", "-sha256", "-passin", "pass:", "-sign"])
        .arg(key_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            Stop::Refused(if error.kind() == std::io::ErrorKind::NotFound {
                format!(
                    "{openssl} is not on PATH: a GitHub App's token is asked for with a \
                     signature it makes, so install it; nothing was sent"
                )
            } else {
                format!(
                    "{openssl} did not start: {}; nothing was sent",
                    error.kind()
                )
            })
        })?;
    let wrote = child.stdin.take().map(|mut stdin| stdin.write_all(input));
    let done = (child.wait_with_output()).map_err(|error| {
        Stop::Refused(format!(
            "{openssl} was not waited for: {}; nothing was sent",
            error.kind()
        ))
    })?;
    if !done.status.success() || matches!(wrote, Some(Err(_))) {
        return Err(Stop::Refused(format!(
            "{openssl} could not sign with {key} {named}: write the App's private key there as \
             GitHub gave it, a PEM file with no passphrase; nothing was sent"
        )));
    }
    if done.stdout.len() < RSA_2048 {
        return Err(Stop::Refused(format!(
            "{key} {named} is no RSA key of 2048 bits or more, which a GitHub App's key is; \
             nothing was sent"
        )));
    }
    Ok(done.stdout)
}

/// A token of `app` made by GitHub now, for the one repository the settings name and the
/// permissions the settings ask for ([`permissions`]), with its expiry.
fn make(
    ctx: &Context<'_>,
    config: &Config,
    app: &App,
    key_file: &Path,
) -> Result<(Secret, Expiry), Stop> {
    let key = TOKEN_FILE;
    let repository = (config.repository.as_ref())
        .ok_or_else(|| Stop::Refused("the settings name no repository".into()))?;
    let jwt = jwt(ctx.openssl, app.id, key_file, ctx.now, "key_file")?;
    let jwt = Secret::new(&jwt).map_err(Stop::Refused)?;
    let transport = (ctx.connect)(jwt.clone()).map_err(Stop::Unavailable)?;
    let asked = permissions(config);
    let request = Request {
        method: "POST",
        path: format!("/app/installations/{}/access_tokens", app.installation),
        body: Some(json!({"repositories": [repository.name], "permissions": asked}).to_string()),
    };
    let making = format!("making {key}'s token as App {}", app.id);
    let response = transport.send(&request).map_err(|failure| match failure {
        Failure::NotSent(why) | Failure::Unknown(why) => {
            Stop::Unavailable(format!("{making}: {}", jwt.redact(&why)))
        }
    })?;
    let value: Value = serde_json::from_str(&response.body).unwrap_or(Value::Null);
    let message = || {
        let said = value["message"].as_str().unwrap_or_default();
        let said: String = (said.chars())
            .filter(|c| !c.is_control())
            .take(200)
            .collect();
        jwt.redact(&said)
    };
    let limited = response
        .headers
        .get("x-ratelimit-remaining")
        .map(String::as_str)
        == Some("0")
        || response.headers.contains_key("retry-after");
    match response.status {
        201 => {
            let token = (value["token"].as_str())
                .and_then(|token| Secret::new(token).ok())
                .ok_or_else(|| {
                    Stop::Unavailable(format!("{making}: GitHub's answer holds no token"))
                })?;
            let expires = (value["expires_at"].as_str())
                .and_then(Expiry::parse)
                .ok_or_else(|| {
                    Stop::Unavailable(format!("{making}: GitHub's answer says no expiry it reads"))
                })?;
            Ok((token, expires))
        }
        401 => Err(Stop::Refused(format!(
            "{making}: GitHub refused the App's JWT (401: {}): key_file is not App {}'s \
             private key, app_id is not that App's number, or this machine's clock is \
             more than a minute ahead of GitHub's or nine minutes behind it",
            message(),
            app.id
        ))),
        404 => Err(Stop::Refused(format!(
            "{making}: GitHub found no installation {} of App {} (404): \
             installation_id is not its number, or the App is not installed",
            app.installation, app.id
        ))),
        422 | 403 if !limited => Err(Stop::Refused(format!(
            "{making}: GitHub refused a token for {} with {} ({}: {}): give App {} those \
             permissions and install it on that repository",
            repository.full_name(),
            asked
                .iter()
                .map(|(name, level)| format!("{name} {level}"))
                .collect::<Vec<_>>()
                .join(", "),
            response.status,
            message(),
            app.id
        ))),
        status => Err(Stop::Unavailable(format!(
            "{making}: GitHub answered {status}; nothing was written, and the next use asks again"
        ))),
    }
}

/// `token`, its expiry and the permissions it was `asked` for written to the token file at
/// `real`: to a file of its own beside it, readable by its owner alone, then renamed over it.
fn write(
    real: &Path,
    token: &Secret,
    expires: &Expiry,
    asked: &str,
    key: &str,
) -> Result<(), Stop> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let named = real.display();
    let refused = |error: std::io::Error| {
        Stop::Refused(format!(
            "{key} {named}: the token made was not written: {}",
            error.kind()
        ))
    };
    let (Some(folder), Some(name)) = (real.parent(), real.file_name()) else {
        return Err(Stop::Refused(format!("{key} {named} names no file")));
    };
    let beside = folder.join(format!(
        ".{}.{}-{}.new",
        name.to_string_lossy(),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&beside)?;
        write!(
            file,
            "{}\nexpires_at {}\npermissions {asked}\n",
            token.expose(),
            expires.text
        )?;
        file.sync_all()?;
        fs::rename(&beside, real)
    })();
    written.map_err(|error| {
        let _ = fs::remove_file(&beside);
        refused(error)
    })
}

#[cfg(test)]
#[path = "mint_tests.rs"]
mod tests;
