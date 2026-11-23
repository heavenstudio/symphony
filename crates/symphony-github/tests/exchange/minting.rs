//! A GitHub App's installation token made by the program where the settings name the App's
//! installation and key (src/mint.rs). GitHub's side of the Apps is held here: it answers
//! `POST /app/installations/ID/access_tokens` to a JWT whose signature it verifies by `openssl`
//! with the public half of the App's key, as GitHub does with the one it keeps.

use crate::deliver::{APP, As, Connect, Delivery, HEAD, snapshot};
use crate::github::{Fake, Fault};
use crate::{Settings, TOKEN};
use data_encoding::BASE64URL_NOPAD;
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex, MutexGuard, OnceLock};
use symphony_github::config;
use symphony_github::credential::Secret;
use symphony_github::mint::{self, MARGIN};
use symphony_github::reconcile::Context;
use symphony_github::transport::{Failure, Request, Response, Transport};

/// An App's installation on the repository: its number and a hundred.
fn installation(app: i64) -> i64 {
    app + 100
}

/// The private keys of the App, of two others and of one that is none of theirs (`stranger`),
/// each beside
/// its public half: made by `openssl` once and kept for every later run, as making them takes
/// longer than the tests that use them.
fn keys() -> &'static Path {
    static KEYS: OnceLock<PathBuf> = OnceLock::new();
    KEYS.get_or_init(|| {
        let kept = std::env::temp_dir().join("symphony-github-app-keys");
        if kept.join("stranger.pub").is_file() {
            return kept.canonicalize().unwrap();
        }
        let made =
            std::env::temp_dir().join(format!("symphony-github-app-keys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&made);
        std::fs::create_dir_all(&made).unwrap();
        for name in ["42", "43", "44", "stranger"] {
            let key = made.join(format!("{name}.pem"));
            let ran = Command::new("openssl")
                .args(["genrsa", "-out"])
                .arg(&key)
                .arg("2048")
                .stderr(Stdio::null())
                .status()
                .unwrap();
            assert!(ran.success(), "openssl genrsa");
            std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
            let ran = Command::new("openssl")
                .args(["rsa", "-pubout", "-in"])
                .arg(&key)
                .arg("-out")
                .arg(made.join(format!("{name}.pub")))
                .stderr(Stdio::null())
                .status()
                .unwrap();
            assert!(ran.success(), "openssl rsa -pubout");
        }
        // Another run that made them first keeps its own.
        if std::fs::rename(&made, &kept).is_err() {
            let _ = std::fs::remove_dir_all(&made);
        }
        kept.canonicalize().unwrap()
    })
}

/// The key of `name`'s App.
fn key(name: &str) -> PathBuf {
    keys().join(format!("{name}.pem"))
}

/// `at`, seconds of 1970-01-01, as GitHub writes a time.
fn clock(at: i64) -> String {
    assert!((0..86_400).contains(&at), "{at} is on the first day");
    format!(
        "1970-01-01T{:02}:{:02}:{:02}Z",
        at / 3_600,
        at % 3_600 / 60,
        at % 60
    )
}

/// The settings that give the App its installation and key.
fn maker() -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "installation_id = {}\nkey_file = \"{}\"",
        installation(APP),
        key(&APP.to_string()).display()
    );
    text
}

/// What GitHub's side of the Apps was asked and made.
#[derive(Default)]
struct Made {
    /// Each request: the App its JWT verified as (0 for none), its path and its body.
    asked: Vec<(i64, String, Value)>,
    jwts: Vec<String>,
    /// Each token made, with its App.
    tokens: Vec<(String, i64)>,
    /// What the next request is answered.
    planted: Option<Planted>,
}

/// An answer planted for the next request.
#[derive(Clone)]
enum Planted {
    /// This status and body.
    Answer(u16, Value),
    /// This status and body, under these headers.
    Headed(u16, &'static [(&'static str, &'static str)], Value),
    /// None: the connection ends with no answer.
    Lost,
}

/// GitHub's side of the Apps.
pub(crate) struct Apps {
    now: AtomicI64,
    made: Mutex<Made>,
}

impl Apps {
    fn new(now: i64) -> Arc<Apps> {
        Arc::new(Apps {
            now: AtomicI64::new(now),
            made: Mutex::default(),
        })
    }

    fn made(&self) -> MutexGuard<'_, Made> {
        self.made
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// What the next request is answered.
    fn plant(&self, answer: Planted) {
        self.made().planted = Some(answer);
    }

    fn asked(&self) -> Vec<(i64, String, Value)> {
        self.made().asked.clone()
    }

    fn tokens(&self) -> Vec<(String, i64)> {
        self.made().tokens.clone()
    }

    /// Every token and JWT made, and each line of every key: none may be said or kept.
    fn secrets(&self) -> Vec<String> {
        let made = self.made();
        let mut secrets: Vec<String> = (made.tokens.iter().map(|(token, _)| token.clone()))
            .chain(made.jwts.iter().cloned())
            .collect();
        for name in ["42", "43", "44", "stranger"] {
            let text = std::fs::read_to_string(key(name)).unwrap();
            secrets.extend(
                (text.lines())
                    .filter(|line| !line.starts_with("-----"))
                    .map(str::to_string),
            );
        }
        secrets
    }

    /// The App `jwt` is of, where its signature is that App's key's and it holds now.
    fn verified(&self, jwt: &str) -> Option<i64> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let parts: Vec<&str> = jwt.split('.').collect();
        let [header, claims, signature] = parts.as_slice() else {
            return None;
        };
        let decode = |part: &str| BASE64URL_NOPAD.decode(part.as_bytes()).ok();
        let read = |part: &str| serde_json::from_slice::<Value>(&decode(part)?).ok();
        if read(header)? != json!({"alg": "RS256", "typ": "JWT"}) {
            return None;
        }
        let claims_read = read(claims)?;
        let app: i64 = claims_read["iss"].as_str()?.parse().ok()?;
        let (iat, exp) = (claims_read["iat"].as_i64()?, claims_read["exp"].as_i64()?);
        let now = self.now.load(Ordering::SeqCst);
        if !(iat <= now && now < exp && exp - iat <= 600) {
            return None;
        }
        let public = keys().join(format!("{app}.pub"));
        if !public.is_file() {
            return None;
        }
        let signed = std::env::temp_dir().join(format!(
            "symphony-github-signature-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&signed, decode(signature)?).unwrap();
        let mut child = Command::new("openssl")
            .args(["dgst", "-sha256", "-verify"])
            .arg(&public)
            .arg("-signature")
            .arg(&signed)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin
            .write_all(format!("{header}.{claims}").as_bytes())
            .unwrap();
        drop(stdin);
        let good = child.wait().unwrap().success();
        let _ = std::fs::remove_file(&signed);
        good.then_some(app)
    }

    /// GitHub's answer to `request` carrying `jwt`.
    fn answer(&self, jwt: &str, request: &Request) -> Result<Response, Failure> {
        let app = self.verified(jwt);
        let now = self.now.load(Ordering::SeqCst);
        let mut made = self.made();
        let body = (request.body.as_deref())
            .and_then(|body| serde_json::from_str(body).ok())
            .unwrap_or(Value::Null);
        made.asked
            .push((app.unwrap_or(0), request.path.clone(), body));
        made.jwts.push(jwt.into());
        let respond = |status: u16, body: Value| Response {
            status,
            headers: std::collections::BTreeMap::new(),
            body: body.to_string(),
        };
        if let Some(planted) = made.planted.take() {
            return match planted {
                Planted::Answer(status, body) => Ok(respond(status, body)),
                Planted::Headed(status, headers, body) => Ok(Response {
                    headers: (headers.iter())
                        .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                        .collect(),
                    ..respond(status, body)
                }),
                Planted::Lost => Err(Failure::Unknown(format!(
                    "the connection ended with no answer to Bearer {jwt}"
                ))),
            };
        }
        let Some(app) = app else {
            return Ok(respond(
                401,
                json!({"message": "A JSON web token could not be decoded"}),
            ));
        };
        let wanted = format!("/app/installations/{}/access_tokens", installation(app));
        if request.method != "POST" || request.path != wanted {
            return Ok(respond(404, json!({"message": "Not Found"})));
        }
        let token = format!("ghs_minted{app}n{}SECRET", made.tokens.len());
        made.tokens.push((token.clone(), app));
        Ok(respond(
            201,
            json!({
                "token": token, "expires_at": clock(now + 3_600),
                "permissions": {}, "repository_selection": "selected",
            }),
        ))
    }

    /// What makes a request: to these Apps with a JWT, else to the repository as the App whose
    /// token it carries, a token made here or one of the files' own.
    fn connect(self: &Arc<Apps>, github: &Arc<Fake>) -> Connect {
        let (apps, github) = (Arc::clone(self), Arc::clone(github));
        Box::new(move |token: Secret| -> Result<Box<dyn Transport>, String> {
            let text = token.expose().to_string();
            if text.split('.').count() == 3 {
                return Ok(Box::new(Minting(Arc::clone(&apps), text)));
            }
            let made = (apps.tokens().into_iter()).find(|(made, _)| *made == text);
            let app = made.map_or_else(|| if text == TOKEN { APP } else { 0 }, |(_, app)| app);
            Ok(Box::new(As(Arc::clone(&github), app)))
        })
    }
}

/// A request carrying a JWT, to GitHub's side of the Apps.
struct Minting(Arc<Apps>, String);

impl Transport for Minting {
    fn send(&self, request: &Request) -> Result<Response, Failure> {
        self.0.answer(&self.1, request)
    }
}

/// A delivery whose App's installation and key are set where `made` says, with `more` settings,
/// and its requests made through GitHub's side of the Apps.
fn delivery(name: &str, made: bool, more: &str) -> (Delivery, Arc<Apps>) {
    let maker = if made { maker() } else { String::new() };
    let mut d = Delivery::unverified(name, &format!("{maker}{more}"));
    let apps = Apps::new(d.rig.now.get());
    d.connect = apps.connect(&d.rig.github);
    d.verify().ok();
    (d, apps)
}

fn write(file: &Path, text: &str) {
    std::fs::write(file, text).unwrap();
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600)).unwrap();
}

/// The permissions the App's token is asked for in a delivery: never administration.
fn permissions() -> Value {
    json!({"checks": "write", "contents": "write", "pull_requests": "write"})
}

/// Those permissions as the token file says them.
const ASKED: &str = "checks=write,contents=write,pull_requests=write";

/// A token file as the program writes it: `token`, expiring at `at`, made for [`ASKED`].
fn made(token: &str, at: i64) -> String {
    format!("{token}\nexpires_at {}\npermissions {ASKED}\n", clock(at))
}

/// The delivery answered published, and each request to the repository carried the token of
/// the App the protection holds it to.
fn published(d: &Delivery, answer: &Value) {
    assert_eq!(answer["state"], "published", "{answer}");
    assert_eq!(d.rig.github.pulls().len(), 1);
    let checks = d.rig.github.checks(HEAD);
    assert!(
        checks.iter().all(|(.., app)| *app == APP),
        "every check by the App: {checks:?}"
    );
    assert!(
        !d.rig.github.asked_as(APP).is_empty(),
        "nothing asked as {APP}"
    );
    assert!(
        d.rig.github.asked_as(0).is_empty(),
        "nothing asked with no App's token"
    );
}

#[test]
fn an_absent_token_is_made_once_and_the_delivery_goes_on() {
    let (d, apps) = delivery("mint-absent", true, "");
    let file = d.rig.token_file.clone();
    std::fs::remove_file(&file).unwrap();
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    published(&d, &answer);
    let asked = apps.asked();
    assert_eq!(asked.len(), 1, "{asked:?}");
    let (by, path, body) = &asked[0];
    assert_eq!(*by, APP);
    assert_eq!(
        *path,
        format!("/app/installations/{}/access_tokens", installation(APP))
    );
    assert_eq!(
        *body,
        json!({"repositories": ["n"], "permissions": permissions()})
    );
    let (token, _) = apps.tokens().remove(0);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        made(&token, d.rig.now.get() + 3_600)
    );
    let mode = std::fs::metadata(&file).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "{}", file.display());
    // Nothing is left beside the file but the file.
    let beside: Vec<_> = std::fs::read_dir(d.rig.dir.0.join("secret"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(beside, ["token"]);
    // Current now, the token is used again and none is asked for.
    let again = d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(again, answer);
    assert_eq!(apps.asked().len(), 1);
    d.assert_none_of(
        &apps
            .secrets()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
}

#[test]
fn an_expired_a_nearly_expired_an_unknown_and_an_other_token_are_each_made_again_once() {
    let now = 2_000;
    for (case, kept) in [
        ("expired", made("ghs_old", now - 1)),
        ("expiring", made("ghs_old", now + MARGIN)),
        ("by hand", "ghs_old\n".to_string()),
        // Current, but made before the settings asked for checks: a land would get 403 on them.
        (
            "made for less",
            format!(
                "ghs_old\nexpires_at {}\npermissions contents=write,pull_requests=write\n",
                clock(now + 3_600)
            ),
        ),
        (
            "of an older build",
            format!("ghs_old\nexpires_at {}\n", clock(now + 3_600)),
        ),
    ] {
        let (d, apps) = delivery("mint-again", true, "");
        d.rig.now.set(now);
        apps.now.store(now, Ordering::SeqCst);
        write(&d.rig.token_file, &kept);
        let answer = d.publish(&snapshot(HEAD, "met", "r1"));
        published(&d, &answer);
        assert_eq!(apps.asked().len(), 1, "{case}: one token");
        let text = std::fs::read_to_string(&d.rig.token_file).unwrap();
        assert!(text.starts_with("ghs_minted"), "{case}: {text}");
        assert!(
            text.ends_with(&format!("\npermissions {ASKED}\n")),
            "{case}: {text}"
        );
        assert_eq!(apps.asked()[0].2["permissions"], permissions(), "{case}");
        d.publish(&snapshot(HEAD, "met", "r1"));
        assert_eq!(apps.asked().len(), 1, "{case}: made once");
        d.assert_none_of(
            &apps
                .secrets()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
    }
}

#[test]
fn a_current_token_makes_no_call() {
    let (d, apps) = delivery("mint-current", true, "");
    write(
        &d.rig.token_file,
        &made(TOKEN, d.rig.now.get() + MARGIN + 1),
    );
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    published(&d, &answer);
    assert_eq!(apps.asked().len(), 0, "{:?}", apps.asked());
    d.assert_no_token();
}

/// A delivery whose App makes its token, the file holding an expired one, refused for
/// `why` with the file as it was and nothing pushed or written to the repository.
fn refused(d: &Delivery, apps: &Apps, why: &[&str]) -> Value {
    let file = &d.rig.token_file;
    let old = format!("ghs_old\nexpires_at {}\n", clock(d.rig.now.get() - 1));
    write(file, &old);
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(answer["state"], "refused", "{answer}");
    let said = answer["why"].as_str().unwrap();
    for part in why {
        assert!(said.contains(part), "{part:?} in {said}");
    }
    assert_eq!(
        std::fs::read_to_string(file).unwrap(),
        old,
        "the old file is kept"
    );
    assert!(d.remote.pushed.borrow().is_empty());
    assert!(d.writes().is_empty(), "{:?}", d.writes());
    d.assert_none_of(
        &apps
            .secrets()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    answer
}

#[test]
fn a_loose_key_file_is_refused_before_anything_is_asked() {
    let loose =
        std::env::temp_dir().join(format!("symphony-github-loose-{}.pem", std::process::id()));
    std::fs::copy(key("42"), &loose).unwrap();
    std::fs::set_permissions(&loose, std::fs::Permissions::from_mode(0o644)).unwrap();
    let more = format!(
        "installation_id = {}\nkey_file = \"{}\"\n",
        installation(APP),
        loose.display()
    );
    let (d, apps) = delivery("mint-loose", false, &more);
    refused(
        &d,
        &apps,
        &["key_file", "is readable by others; chmod 600 it"],
    );
    assert_eq!(apps.asked().len(), 0, "{:?}", apps.asked());
    let _ = std::fs::remove_file(&loose);
}

#[test]
fn a_key_of_another_app_is_refused_by_github_and_said_so() {
    let more = format!(
        "installation_id = {}\nkey_file = \"{}\"\n",
        installation(APP),
        key("stranger").display()
    );
    let (d, apps) = delivery("mint-stranger", false, &more);
    refused(
        &d,
        &apps,
        &[
            "GitHub refused the App's JWT (401: A JSON web token could not be decoded)",
            "key_file is not App 42's private key, app_id is not that App's number, or this \
             machine's clock is more than a minute ahead of GitHub's or nine minutes behind it",
        ],
    );
    assert_eq!(apps.asked().len(), 1, "asked once, and refused");
}

#[test]
fn a_jwt_github_reads_as_of_another_time_says_its_words_and_the_clock() {
    let (d, apps) = delivery("mint-clock", true, "");
    apps.plant(Planted::Answer(
        401,
        json!({"message": "'Issued at' claim ('iat') must be an Integer representing a time in the past"}),
    ));
    refused(
        &d,
        &apps,
        &[
            "GitHub refused the App's JWT (401: 'Issued at' claim ('iat') must be an Integer \
             representing a time in the past)",
            "or this machine's clock is more than a minute ahead of GitHub's",
        ],
    );
}

#[test]
fn a_release_run_asks_its_token_for_contents_write_and_publishes() {
    let rig = crate::Rig::off("mint-release");
    std::fs::remove_file(&rig.token_file).unwrap();
    rig.enable(&format!(
        "releases = \"publish\"\napp_id = {APP}\n{}",
        maker()
    ));
    rig.github.tag("release-2026-11-19", HEAD);
    let apps = Apps::new(rig.now.get());
    let connect = apps.connect(&rig.github);
    let file = rig.dir.0.join("release.json");
    std::fs::write(
        &file,
        json!({
            "tag": "release-2026-11-19", "commit": HEAD, "latest": true, "by": "release run",
            "body": "## release-2026-11-19\n",
        })
        .to_string(),
    )
    .unwrap();
    let ran = crate::run(
        &["release", "--request", file.to_str().unwrap()],
        &rig.board,
        &rig.settings,
        &connect,
        rig.now.get(),
        vec![rig.dir.0.join("repository")],
    );
    assert_eq!(ran.code, 0, "{ran:?}");
    let answer: Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(answer["state"], "published", "{answer}");
    assert_eq!(rig.github.releases().len(), 1);
    let asked = apps.asked();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(
        asked[0].2,
        json!({"repositories": ["n"], "permissions": {"contents": "write"}})
    );
    assert!(
        !rig.github.asked_as(APP).is_empty(),
        "the release made as the App"
    );
    for secret in apps.secrets() {
        assert!(!ran.out.contains(&secret) && !ran.err.contains(&secret));
    }
}

#[test]
fn a_file_that_is_no_key_is_refused_and_nothing_of_it_is_said() {
    let marker = "MARKERaaaaBEGINprivateKEYzzzz";
    let planted =
        std::env::temp_dir().join(format!("symphony-github-nokey-{}.pem", std::process::id()));
    write(
        &planted,
        &format!("-----BEGIN RSA PRIVATE KEY-----\n{marker}\n-----END RSA PRIVATE KEY-----\n"),
    );
    let more = format!(
        "installation_id = {}\nkey_file = \"{}\"\n",
        installation(APP),
        planted.display()
    );
    // The openssl on PATH, and one that says the key back on both its outputs before it fails.
    let echo = std::env::temp_dir().join(format!("symphony-github-echo-{}", std::process::id()));
    write(
        &echo,
        "#!/bin/sh\nfor last; do :; done\ncat \"$last\" >&2\ncat \"$last\"\nexit 1\n",
    );
    std::fs::set_permissions(&echo, std::fs::Permissions::from_mode(0o700)).unwrap();
    let echo: &'static str = Box::leak(echo.display().to_string().into_boxed_str());
    for openssl in ["openssl", echo] {
        let (mut d, apps) = delivery("mint-nokey", false, &more);
        d.openssl = openssl;
        let answer = refused(&d, &apps, &["could not sign with key_file", "PEM"]);
        assert!(!answer.to_string().contains(marker), "{answer}");
        assert_eq!(apps.asked().len(), 0, "{:?}", apps.asked());
    }
    let _ = std::fs::remove_file(&planted);
    let _ = std::fs::remove_file(echo);
}

#[test]
fn no_openssl_is_refused_naming_what_to_install() {
    let (mut d, apps) = delivery("mint-no-openssl", true, "");
    d.openssl = "/nowhere/openssl";
    refused(
        &d,
        &apps,
        &[
            "/nowhere/openssl is not on PATH",
            "install it; nothing was sent",
        ],
    );
    assert_eq!(apps.asked().len(), 0, "{:?}", apps.asked());
}

#[test]
fn github_refusing_the_token_says_its_words_and_what_to_fix() {
    let (d, apps) = delivery("mint-422", true, "");
    apps.plant(Planted::Answer(
        422,
        json!({"message": "The permissions requested are not granted to this installation."}),
    ));
    refused(
        &d,
        &apps,
        &[
            "GitHub refused a token for o/n with checks write, contents write, pull_requests write",
            "422: The permissions requested are not granted to this installation.",
            "give App 42 those permissions and install it on that repository",
        ],
    );
    // A refusal is one whatever it says of the rate limit: a 422 is never the limit, though the
    // last request a window allows says none remains, and neither is a 403 with requests left.
    let refusals: [(u16, &'static [(&str, &str)]); 4] = [
        (422, &[("x-ratelimit-remaining", "0")]),
        (422, &[("retry-after", "30")]),
        (403, &[("x-ratelimit-remaining", "12")]),
        (403, &[]),
    ];
    for (status, headers) in refusals {
        let (d, apps) = delivery("mint-refused-headed", true, "");
        apps.plant(Planted::Headed(
            status,
            headers,
            json!({"message": "Resource not accessible by integration"}),
        ));
        refused(
            &d,
            &apps,
            &[
                &format!("{status}: Resource not accessible by integration"),
                "give App 42 those permissions and install it on that repository",
            ],
        );
    }
    // An installation that is not the App's.
    let more = format!(
        "installation_id = 999\nkey_file = \"{}\"\n",
        key("42").display()
    );
    let (d, apps) = delivery("mint-404", false, &more);
    refused(
        &d,
        &apps,
        &[
            "GitHub found no installation 999 of App 42 (404)",
            "installation_id is not its number, or the App is not installed",
        ],
    );
}

#[test]
fn github_not_answering_is_unavailable_and_the_old_file_kept() {
    for planted in [
        Planted::Answer(502, json!({"message": "Bad Gateway"})),
        Planted::Answer(429, json!({"message": "slow down"})),
        // The rate limit as a 403: no request remains, or a wait is named.
        Planted::Headed(
            403,
            &[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "900")],
            json!({"message": "API rate limit exceeded"}),
        ),
        Planted::Headed(
            403,
            &[("retry-after", "30")],
            json!({"message": "You have exceeded a secondary rate limit."}),
        ),
        Planted::Answer(201, json!({"expires_at": clock(9_000)})),
        Planted::Answer(201, json!({"token": "ghs_odd", "expires_at": "tomorrow"})),
        Planted::Lost,
    ] {
        let (d, apps) = delivery("mint-unavailable", true, "");
        let old = format!("ghs_old\nexpires_at {}\n", clock(d.rig.now.get() - 1));
        write(&d.rig.token_file, &old);
        apps.plant(planted.clone());
        let answer = d.publish(&snapshot(HEAD, "met", "r1"));
        assert_eq!(answer["state"], "unavailable", "{answer}");
        assert!(
            answer["why"]
                .as_str()
                .unwrap()
                .starts_with("making token_file's token as App 42: "),
            "{answer}"
        );
        assert_eq!(std::fs::read_to_string(&d.rig.token_file).unwrap(), old);
        assert!(d.remote.pushed.borrow().is_empty());
        d.assert_none_of(
            &apps
                .secrets()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
    }
}

#[test]
fn unset_a_token_file_is_read_as_it_is_and_an_expired_one_is_unavailable() {
    let (d, apps) = delivery("mint-unset", false, "");
    let before = std::fs::read(&d.rig.token_file).unwrap();
    // The token, expired: GitHub answers it 401, and nothing makes another.
    d.rig
        .github
        .fault("GET", "/repos/o/n", Fault::Status(401, Vec::new()));
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(answer["state"], "unavailable", "{answer}");
    assert!(
        answer["why"].as_str().unwrap().contains("(401)"),
        "{answer}"
    );
    assert_eq!(apps.asked().len(), 0, "{:?}", apps.asked());
    let after = std::fs::read(&d.rig.token_file).unwrap();
    assert_eq!(before, after, "no file is written");
    // A file of the shape the program writes is one token too many where it makes none.
    write(&d.rig.token_file, &made(TOKEN, 5_000));
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(answer["state"], "refused", "{answer}");
    assert!(
        answer["why"]
            .as_str()
            .unwrap()
            .ends_with("holds more than one token of printable characters"),
        "{answer}"
    );
    assert_eq!(apps.asked().len(), 0, "{:?}", apps.asked());
    d.assert_no_token();
}

#[test]
fn two_runs_at_once_leave_one_valid_token_and_no_torn_file() {
    let (d, apps) = delivery("mint-together", true, "");
    let file = d.rig.token_file.clone();
    let text = crate::settings(&file, &format!("app_id = {APP}\n{}", maker()));
    let config = config::load(&text).unwrap().config;
    let github = Arc::clone(&d.rig.github);
    let (now, state, repository) = (
        d.rig.now.get(),
        d.rig.dir.0.join("state"),
        d.rig.dir.0.join("repository"),
    );
    for _ in 0..3 {
        std::fs::remove_file(&file).unwrap();
        let starts = Barrier::new(4);
        let made: Vec<String> = std::thread::scope(|scope| {
            let runs: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        let settings = Settings::default();
                        let board = crate::memory::MemBoard::new(state.clone());
                        let connect = apps.connect(&github);
                        let ctx = Context {
                            source: &settings,
                            board: &board,
                            connect: &connect,
                            now,
                            repository: vec![repository.clone()],
                            openssl: "openssl",
                            gh: "gh",
                            rest: &|_| {},
                        };
                        starts.wait();
                        let token = mint::token(&ctx, &config).unwrap();
                        token.expose().to_string()
                    })
                })
                .collect();
            runs.into_iter().map(|run| run.join().unwrap()).collect()
        });
        let kept = std::fs::read_to_string(&file).unwrap();
        let (token, rest) = kept.split_once('\n').unwrap();
        assert!(
            made.contains(&token.to_string()),
            "{kept} is one of {made:?}"
        );
        assert!(apps.tokens().iter().any(|(made, _)| made == token));
        // Settings that publish nothing ask for metadata alone.
        assert_eq!(
            rest,
            format!(
                "expires_at {}\npermissions metadata=read\n",
                clock(now + 3_600)
            )
        );
        let beside = std::fs::read_dir(file.parent().unwrap()).unwrap().count();
        assert_eq!(beside, 1, "no file left half made beside it");
    }
}

#[test]
fn the_exchange_asks_its_token_for_the_issues_permission_alone() {
    for (issues, close, permissions) in [
        ("import", false, json!({"issues": "read"})),
        ("export", false, json!({"issues": "write"})),
        ("import", true, json!({"issues": "write"})),
    ] {
        let rig = crate::Rig::off("mint-exchange");
        std::fs::remove_file(&rig.token_file).unwrap();
        rig.enable(&format!(
            "issues = \"{issues}\"\nimport_labels = [\"symphony\"]\nclose_issues = {close}\n\
             app_id = {APP}\n{}",
            maker()
        ));
        let apps = Apps::new(rig.now.get());
        let connect = apps.connect(&rig.github);
        let ran = crate::run(
            &["preview"],
            &rig.board,
            &rig.settings,
            &connect,
            rig.now.get(),
            vec![rig.dir.0.join("repository")],
        );
        assert_eq!(ran.code, 0, "{ran:?}");
        let asked = apps.asked();
        assert_eq!(asked.len(), 1, "{asked:?}");
        assert_eq!(
            asked[0].2,
            json!({"repositories": ["n"], "permissions": permissions})
        );
        for secret in apps.secrets() {
            assert!(!ran.out.contains(&secret) && !ran.err.contains(&secret));
        }
    }
}

#[test]
fn tokens_says_whether_the_token_is_current_and_never_the_token() {
    let (d, apps) = delivery("mint-tokens", true, "");
    let now = 3_000;
    d.rig.now.set(now);
    let file = d.rig.token_file.display().to_string();
    write(&d.rig.token_file, &made(TOKEN, now + 2_820));
    let ran = d.run(&["tokens"]);
    assert_eq!(
        ran.out,
        format!("token_file {file}: current until 1970-01-01T01:37:00Z (in 47 min)\n")
    );
    assert_eq!(ran.code, 0, "{ran:?}");
    // Made for other permissions, expiring, expired, of no expiry, and absent.
    for (kept, said) in [
        (
            Some(format!(
                "{TOKEN}\nexpires_at {}\npermissions contents=write\n",
                clock(now + 2_820)
            )),
            "current until 1970-01-01T01:37:00Z (in 47 min), made for permissions contents=write \
             where the settings ask for checks=write,contents=write,pull_requests=write; App 42 \
             makes another at its next use",
        ),
        (
            Some(format!("{TOKEN}\nexpires_at {}\n", clock(now + 2_820))),
            "current until 1970-01-01T01:37:00Z (in 47 min), made for permissions it does not \
             record where the settings ask for checks=write,contents=write,pull_requests=write; \
             App 42 makes another at its next use",
        ),
        (
            Some(format!("{TOKEN}\nexpires_at {}\n", clock(now + 240))),
            "expires at 1970-01-01T00:54:00Z (in 4 min); App 42 makes another at its next use",
        ),
        (
            Some(format!("{TOKEN}\nexpires_at {}\n", clock(now - 60))),
            "expired at 1970-01-01T00:49:00Z; App 42 makes another at its next use",
        ),
        (
            Some(format!("{TOKEN}\n")),
            "holds no expiry this program wrote; App 42 makes another at its next use",
        ),
        (None, "absent; App 42 makes one at its next use"),
    ] {
        match &kept {
            Some(kept) => write(&d.rig.token_file, kept),
            None => std::fs::remove_file(&d.rig.token_file).unwrap(),
        }
        let ran = d.run(&["tokens"]);
        assert_eq!(ran.out, format!("token_file {file}: {said}\n"), "{ran:?}");
        assert_eq!(ran.code, 0);
    }
    write(&d.rig.token_file, &format!("{TOKEN}\n"));
    std::fs::set_permissions(&d.rig.token_file, std::fs::Permissions::from_mode(0o640)).unwrap();
    let ran = d.run(&["tokens"]);
    assert_eq!(
        ran.out,
        format!(
            "token_file {file}: refused: token_file {file} is readable by others; chmod 600 it\n"
        ),
        "{ran:?}"
    );
    assert_eq!(ran.code, 1);
    // A token no App the settings give makes is kept by hand.
    let (by_hand, _) = delivery("mint-tokens-by-hand", false, "");
    let ran = by_hand.run(&["tokens"]);
    assert_eq!(
        ran.out,
        format!(
            "token_file {}: kept by hand: when it expires is not known here\n",
            by_hand.rig.token_file.display()
        )
    );
    assert_eq!(ran.code, 0);
    // Settings that do not read, and settings that name no token file.
    d.rig.settings.set(Some("enabled = maybe\n".into()));
    let ran = d.run(&["tokens"]);
    assert!(
        ran.out
            .starts_with("turned off: its settings do not read: "),
        "{ran:?}"
    );
    assert_eq!(ran.code, 1);
    d.rig.settings.set(None);
    let ran = d.run(&["tokens"]);
    assert_eq!(
        (ran.out.as_str(), ran.code),
        ("the committed settings name no token file\n", 0)
    );
    // It reads no remote and writes nothing.
    assert_eq!(apps.asked().len(), 0, "{:?}", apps.asked());
    assert_eq!(d.rig.github.asked().len(), 0);
    for text in [&ran.out, &ran.err] {
        assert!(!text.contains(TOKEN), "{text}");
    }
}
