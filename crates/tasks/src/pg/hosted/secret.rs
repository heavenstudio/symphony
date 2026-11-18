//! What a hosted connection is made from, read from the deployment's secrets each time a
//! connection is made: the PostgreSQL URI and the certificate files the settings name
//! ([`super::super::external`]). Nothing read here is ever said again: a refusal names the
//! settings' key and the kind of failure, and [`Target`]'s debug form names its fingerprints.

use super::Refusal;
use crate::pg::external::Settings;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::sync::Arc;
use tokio_postgres::config::{ChannelBinding, Host, SslMode};

/// The most bytes a URI may be, in its variable or its file.
const MOST_URL: u64 = 8 * 1024;
/// The most bytes a certificate file may be.
const MOST_PEM: u64 = 1024 * 1024;
/// The databases every cluster has, which are no project's board.
const MAINTENANCE: [&str; 3] = ["postgres", "template0", "template1"];

/// How a process's environment is read: a variable's value, when it is set.
pub type Environment = dyn Fn(&str) -> Option<String> + Send + Sync;

/// One reading of the secrets: where to connect, as whom, and what to trust.
pub(super) struct Target {
    pub(super) config: tokio_postgres::Config,
    pub(super) tls: rustls::ClientConfig,
    /// The endpoint, database and role, which no rotation changes: a digest, never the names.
    pub(super) identity: String,
    /// Everything read, so a rotation is seen: a digest.
    pub(super) generation: [u8; 32],
    pub(super) database: String,
    pub(super) user: String,
}

impl std::fmt::Debug for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Target({})", self.identity)
    }
}

/// `key`'s variable, refused when the environment does not hold it.
fn variable(key: &'static str, name: &str, env: &Environment) -> Result<String, Refusal> {
    env(name)
        .filter(|value| !value.is_empty())
        .ok_or(Refusal::Configuration(
            key,
            "names a variable that is not set",
        ))
}

/// The file `key`'s variable names, at most `most` bytes of it.
fn file(key: &'static str, name: &str, env: &Environment, most: u64) -> Result<Vec<u8>, Refusal> {
    let path = variable(key, name, env)?;
    let unread = Refusal::Configuration(key, "names a file that cannot be read");
    let opened = std::fs::File::open(path).map_err(|_| unread.clone())?;
    let mut bytes = Vec::new();
    opened
        .take(most + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unread)?;
    if bytes.len() as u64 > most {
        return Err(Refusal::Configuration(
            key,
            "names a file that is too large",
        ));
    }
    Ok(bytes)
}

impl Target {
    /// The secrets `settings` names, read from `env` and the files it names.
    pub(super) fn read(settings: &Settings, env: &Environment) -> Result<Target, Refusal> {
        let (key, url) = match (&settings.url_env, &settings.url_file_env) {
            (Some(name), None) => ("url_env", variable("url_env", name, env)?.into_bytes()),
            (None, Some(name)) => ("url_file_env", file("url_file_env", name, env, MOST_URL)?),
            _ => {
                return Err(Refusal::Configuration(
                    "url_env",
                    "is not the one URL named",
                ));
            }
        };
        if url.len() as u64 > MOST_URL {
            return Err(Refusal::Configuration(key, "holds a URL that is too long"));
        }
        let url = String::from_utf8(url)
            .map_err(|_| Refusal::Configuration(key, "holds a URL that is not UTF-8"))?;
        let ca = file("ca_file_env", &settings.ca_file_env, env, MOST_PEM)?;
        let client = match (
            &settings.client_cert_file_env,
            &settings.client_key_file_env,
        ) {
            (Some(cert), Some(private)) => Some((
                file("client_cert_file_env", cert, env, MOST_PEM)?,
                file("client_key_file_env", private, env, MOST_PEM)?,
            )),
            _ => None,
        };
        let mut generation = Sha256::new();
        for part in [Some(url.as_bytes()), Some(ca.as_slice())]
            .into_iter()
            .chain(
                client
                    .iter()
                    .flat_map(|(c, k)| [Some(&c[..]), Some(&k[..])]),
            )
            .flatten()
        {
            generation.update((part.len() as u64).to_be_bytes());
            generation.update(part);
        }
        let mut config = parsed(key, url.trim())?;
        let (Some(Host::Tcp(host)), Some(database), Some(user)) = (
            config.get_hosts().first().cloned(),
            config.get_dbname().map(str::to_string),
            config.get_user().map(str::to_string),
        ) else {
            unreachable!("parsed() holds a URL to one TCP host, a database and a user");
        };
        let port = config.get_ports().first().copied().unwrap_or(5432);
        let identity = Sha256::digest(format!("{host}\0{port}\0{database}\0{user}"));
        config
            .ssl_mode(SslMode::Require)
            .channel_binding(ChannelBinding::Require)
            .application_name("symphony-board")
            .connect_timeout(std::time::Duration::from_secs(
                settings.connect_timeout_seconds.into(),
            ));
        Ok(Target {
            config,
            tls: trust(&ca, client)?,
            identity: identity
                .iter()
                .take(6)
                .map(|b| format!("{b:02x}"))
                .collect(),
            generation: generation.finalize().into(),
            database,
            user,
        })
    }
}

/// `url` as the driver reads it, held to one TCP host, a database of the project's own and a
/// login with its password, and to nothing that would choose the session's settings or a
/// connection that is not verified. The driver's own words on a URL it cannot read are not
/// said: they can hold a part of it.
fn parsed(key: &'static str, url: &str) -> Result<tokio_postgres::Config, Refusal> {
    let refused = |why: &'static str| Refusal::Configuration(key, why);
    if !(url.starts_with("postgresql://") || url.starts_with("postgres://")) {
        return Err(refused("holds what is not a postgresql:// URL"));
    }
    let config: tokio_postgres::Config = url
        .parse()
        .map_err(|_| refused("holds a URL the driver cannot read"))?;
    match config.get_hosts() {
        [Host::Tcp(host)] if !host.is_empty() => {}
        [_] => return Err(refused("holds a URL to a socket, not a host")),
        _ => return Err(refused("holds a URL that does not name exactly one host")),
    }
    if config.get_ports().len() > 1 || !config.get_hostaddrs().is_empty() {
        return Err(refused("holds a URL that names more than one address"));
    }
    match config.get_dbname() {
        None | Some("") => return Err(refused("holds a URL that names no database")),
        Some(name) if MAINTENANCE.contains(&name) => {
            return Err(refused(
                "holds a URL to a maintenance database, which is no board's",
            ));
        }
        Some(_) => {}
    }
    if config.get_user().is_none_or(str::is_empty) {
        return Err(refused("holds a URL that names no login"));
    }
    if config.get_password().is_none_or(<[u8]>::is_empty) {
        return Err(refused("holds a URL with no password"));
    }
    if config.get_options().is_some() {
        return Err(refused(
            "holds a URL with startup options, which are refused",
        ));
    }
    if config.get_ssl_mode() == SslMode::Disable {
        return Err(refused(
            "holds a URL that turns TLS off; the connection is always verified",
        ));
    }
    Ok(config)
}

/// The TLS a connection speaks: the deployment's authority alone trusted, the endpoint's name
/// checked against its certificate, and the client's certificate offered when there is one.
fn trust(ca: &[u8], client: Option<(Vec<u8>, Vec<u8>)>) -> Result<rustls::ClientConfig, Refusal> {
    let unread = Refusal::Configuration("ca_file_env", "names a file with no certificate in it");
    let mut roots = rustls::RootCertStore::empty();
    for certificate in CertificateDer::pem_slice_iter(ca) {
        let certificate = certificate.map_err(|_| unread.clone())?;
        roots.add(certificate).map_err(|_| unread.clone())?;
    }
    if roots.is_empty() {
        return Err(unread);
    }
    let builder = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| Refusal::Configuration("ca_file_env", "meets a TLS setup that cannot be made"))?
    .with_root_certificates(roots);
    let Some((certificates, private)) = client else {
        return Ok(builder.with_no_client_auth());
    };
    let unread = Refusal::Configuration(
        "client_cert_file_env",
        "names a client certificate or key that cannot be read",
    );
    let chain = CertificateDer::pem_slice_iter(&certificates)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| unread.clone())?;
    let private = PrivateKeyDer::from_pem_slice(&private).map_err(|_| unread.clone())?;
    builder
        .with_client_auth_cert(chain, private)
        .map_err(|_| unread)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A certificate that reads as one: the pool never verifies with it in these tests.
    pub(in crate::pg::hosted) const CA: &str = include_str!("test-ca.pem");

    pub(in crate::pg::hosted) fn settings(more: &str) -> Settings {
        toml::from_str(&format!(
            "url_env = \"URL\"\nca_file_env = \"CA\"\nclass_env = \"CLASS\"\n{more}\n\
             [budget]\nrole_connection_cap = 8\nmaintenance = 1\nheadroom = 1\n\
             [budget.classes]\nweb = {{ processes = 2, pool_max = 3 }}\n"
        ))
        .unwrap()
    }

    /// An environment of `vars`, the certificate authority's file among them.
    pub(in crate::pg::hosted) fn env(
        dir: &std::path::Path,
        vars: &[(&str, &str)],
    ) -> Box<Environment> {
        let ca = dir.join("ca.pem");
        std::fs::write(&ca, CA).unwrap();
        let mut held: BTreeMap<String, String> = BTreeMap::new();
        held.insert("CA".into(), ca.display().to_string());
        held.insert("CLASS".into(), "web".into());
        for (name, value) in vars {
            held.insert((*name).into(), (*value).into());
        }
        Box::new(move |name| held.get(name).cloned())
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "symphony-hosted-secret-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const URL: &str = "postgresql://board:s3cr%40t-word@db.example:6543/project_board";

    fn refusal(url: &str) -> String {
        let dir = temp("refusal");
        let refused = Target::read(&settings(""), &env(&dir, &[("URL", url)])).unwrap_err();
        let said = format!("{refused} {refused:?}");
        for secret in ["s3cr", "t-word", "db.example", "project_board", "6543"] {
            assert!(!said.contains(secret), "{said}");
        }
        refused.to_string()
    }

    #[test]
    fn a_url_gives_its_one_host_database_and_login_and_says_none_of_them() {
        let dir = temp("whole");
        let target = Target::read(&settings(""), &env(&dir, &[("URL", URL)])).unwrap();
        assert_eq!(target.database, "project_board");
        assert_eq!(target.user, "board");
        assert_eq!(target.config.get_password(), Some(&b"s3cr@t-word"[..]));
        assert_eq!(target.config.get_ports(), [6543]);
        assert_eq!(target.config.get_ssl_mode(), SslMode::Require);
        assert_eq!(target.config.get_channel_binding(), ChannelBinding::Require);
        let shown = format!("{target:?}");
        assert_eq!(shown, format!("Target({})", target.identity));
        assert_eq!(target.identity.len(), 12);
        // An IPv6 host and a default port read too.
        let v6 = Target::read(
            &settings(""),
            &env(&dir, &[("URL", "postgres://u:p@[::1]/board")]),
        )
        .unwrap();
        assert_eq!(v6.config.get_hosts(), [Host::Tcp("::1".into())]);
        assert_ne!(v6.identity, target.identity);
    }

    #[test]
    fn a_rotated_password_is_a_new_generation_of_the_same_identity() {
        let dir = temp("rotation");
        let read = |url: &str| Target::read(&settings(""), &env(&dir, &[("URL", url)])).unwrap();
        let (before, same, after) = (
            read(URL),
            read(URL),
            read(&URL.replace("s3cr%40t-word", "another")),
        );
        assert_eq!(before.generation, same.generation);
        assert_ne!(before.generation, after.generation);
        assert_eq!(before.identity, after.identity);
    }

    #[test]
    fn a_url_that_is_not_one_verified_host_and_database_is_refused_by_its_kind() {
        for (url, why) in [
            ("", "names a variable that is not set"),
            (
                "host=db.example dbname=x user=u password=p",
                "not a postgresql:// URL",
            ),
            (
                "postgresql://u:p@a.example,b.example/board",
                "exactly one host",
            ),
            (
                "postgresql://u:p@%2Fvar%2Frun/board",
                "a socket, not a host",
            ),
            // The driver reads a second port as a second host.
            ("postgresql://u:p@db.example:1,2/board", "exactly one host"),
            (
                "postgresql://u:p@db.example/board?hostaddr=10.0.0.1",
                "more than one address",
            ),
            ("postgresql://u:p@db.example", "names no database"),
            (
                "postgresql://u:p@db.example/postgres",
                "a maintenance database",
            ),
            (
                "postgresql://u:p@db.example/template1",
                "a maintenance database",
            ),
            ("postgresql://:p@db.example/board", "names no login"),
            ("postgresql://u@db.example/board", "no password"),
            (
                "postgresql://u:p@db.example/board?options=-c%20search_path%3Dx",
                "startup options",
            ),
            (
                "postgresql://u:p@db.example/board?sslmode=disable",
                "turns TLS off",
            ),
            (
                "postgresql://u:p@db.example/board?sslmode=verify-ca",
                "the driver cannot read",
            ),
            (
                "postgresql://u:p@db.example/board?nonsense=1",
                "the driver cannot read",
            ),
        ] {
            let said = refusal(url);
            assert!(said.contains(why), "{url}: {said}");
            assert!(
                said.contains("[board.external_postgresql] url_env"),
                "{said}"
            );
        }
        let long = format!("postgresql://u:p@db.example/{}", "b".repeat(9000));
        assert!(refusal(&long).contains("too long"));
    }

    #[test]
    fn a_url_file_and_the_certificates_are_read_bounded_and_refused_by_their_key() {
        let dir = temp("files");
        let url = dir.join("url");
        std::fs::write(&url, format!("{URL}\n")).unwrap();
        let by_file = settings("").clone();
        let by_file = Settings {
            url_env: None,
            url_file_env: Some("URL_FILE".into()),
            ..by_file
        };
        let path = url.display().to_string();
        let target = Target::read(&by_file, &env(&dir, &[("URL_FILE", &path)])).unwrap();
        assert_eq!(target.database, "project_board");
        std::fs::write(&url, vec![b'x'; 9000]).unwrap();
        let large = Target::read(&by_file, &env(&dir, &[("URL_FILE", &path)])).unwrap_err();
        assert_eq!(
            large.to_string(),
            "[board.external_postgresql] url_file_env names a file that is too large"
        );
        std::fs::write(&url, [0xff, 0xfe]).unwrap();
        let binary = Target::read(&by_file, &env(&dir, &[("URL_FILE", &path)])).unwrap_err();
        assert!(binary.to_string().contains("not UTF-8"), "{binary}");
        let gone = dir.join("gone").display().to_string();
        let missing = Target::read(&by_file, &env(&dir, &[("URL_FILE", &gone)])).unwrap_err();
        assert!(missing.to_string().contains("cannot be read"), "{missing}");
        assert!(!missing.to_string().contains(&gone), "{missing}");

        let empty = dir.join("empty.pem");
        std::fs::write(&empty, "no certificate here\n").unwrap();
        let no_ca = Target::read(
            &settings(""),
            &env(&dir, &[("URL", URL), ("CA", &empty.display().to_string())]),
        )
        .unwrap_err();
        assert_eq!(
            no_ca.to_string(),
            "[board.external_postgresql] ca_file_env names a file with no certificate in it"
        );
        let with_client =
            settings("client_cert_file_env = \"CERT\"\nclient_key_file_env = \"KEY\"");
        let bad = Target::read(
            &with_client,
            &env(
                &dir,
                &[
                    ("URL", URL),
                    ("CERT", &dir.join("ca.pem").display().to_string()),
                    ("KEY", &empty.display().to_string()),
                ],
            ),
        )
        .unwrap_err();
        assert!(
            bad.to_string()
                .contains("client certificate or key that cannot be read"),
            "{bad}"
        );
    }
}
