//! The test binary's PostgreSQL servers, one per major and certificate, each started at the first
//! test that asks for it: `initdb`, a certificate authority and a server certificate of the
//! binary's own (`openssl`), TLS and SCRAM on a loopback port, and the superuser trusted on a
//! socket under `/tmp` alone, which the tests provision through as a platform's operator would.
//! A server runs under a shell that stops it once this process's pipe to it closes, so it ends
//! with the binary however that ends.
//!
//! PostgreSQL 18's programs are the machine's, found as the board finds them. PostgreSQL 16's are
//! the folder `SYMPHONY_POSTGRESQL_16` names, or Homebrew's keg (`brew install postgresql@16`,
//! which starts nothing and changes no `postgres` on `PATH`).

use crate::children::Child as _;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use symphony_board::pg::external::Settings;
use symphony_board::pg::hosted::{Client, Environment};
use symphony_board::pg::{self, Step};

/// The certificate a server presents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Certificate {
    /// For `localhost` and `127.0.0.1`, signed by the authority the tests trust.
    Good,
    /// Signed by that authority, for another name.
    OtherName,
    /// Signed by that authority, for the right names, and expired.
    Expired,
    /// None: the server speaks no TLS.
    None,
}

/// Which of `localhost`'s two addresses a server listens on, the other left to no one. A name's
/// addresses come in an order the machine chooses, so a server on each shows a client both: the
/// address nothing holds asked before the server's, and after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Loopback {
    V4,
    V6,
}

impl Loopback {
    /// The address, and the block of it a `pg_hba.conf` line names.
    fn address(self) -> (&'static str, &'static str) {
        match self {
            Loopback::V4 => ("127.0.0.1", "127.0.0.1/32"),
            Loopback::V6 => ("::1", "::1/128"),
        }
    }
}

pub struct Cluster {
    pub port: u16,
    pub socket: PathBuf,
    /// The authority's certificate, which a client trusts.
    pub ca: PathBuf,
    /// Another authority's, which signed nothing here.
    pub other_ca: PathBuf,
    dir: PathBuf,
    shell: Mutex<symphony_board::sys::Child>,
}

/// Each server started, or why it did not start: a start that failed is not tried again.
static CLUSTERS: Mutex<BTreeMap<(u32, Certificate, Loopback), Started>> =
    Mutex::new(BTreeMap::new());
/// A server, or why it did not start.
type Started = Result<&'static Cluster, String>;
static NAMES: AtomicUsize = AtomicUsize::new(0);

/// Both majors a hosted board is proved on.
pub const MAJORS: [u32; 2] = [16, 18];
const READY: Duration = test_time::backstop(Duration::from_secs(10));

/// The folder of `major`'s programs.
fn programs(major: u32) -> PathBuf {
    if major == 18 {
        return symphony_board::server::postgres::discover_programs(&crate::store::project_root())
            .unwrap_or_else(|why| panic!("PostgreSQL 18 is needed: {why}"));
    }
    let folder = symphony_board::server::postgres::hosted_programs_16();
    assert!(
        folder.join("postgres").is_file() && folder.join("initdb").is_file(),
        "the hosted tests need PostgreSQL 16's programs and found none in {}: install them \
         (`brew install postgresql@16`, which starts nothing) or name their folder with \
         SYMPHONY_POSTGRESQL_16. They are not skipped: a hosted board is proved on 16 and 18",
        folder.display()
    );
    folder
}

/// `program` run to its end, and what it said: a fixture's step that must not fail.
fn run(command: &mut Command) -> String {
    let said = command
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|why| panic!("{command:?} did not start: {why}"));
    assert!(
        said.status.success(),
        "{command:?} failed: {}{}",
        String::from_utf8_lossy(&said.stdout),
        String::from_utf8_lossy(&said.stderr)
    );
    String::from_utf8_lossy(&said.stdout).into_owned()
}

fn openssl(dir: &Path, args: &[&str]) {
    run(Command::of("openssl").current_dir(dir).args(args));
}

/// The authority `name` (`name.key`, `name.pem`) made in `dir`.
fn authority(dir: &Path, name: &str) {
    openssl(
        dir,
        &[
            "req",
            "-x509",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:prime256v1",
            "-nodes",
            "-keyout",
            &format!("{name}.key"),
            "-out",
            &format!("{name}.pem"),
            "-days",
            "3",
            "-subj",
            &format!("/CN=symphony hosted tests {name}"),
            "-addext",
            "basicConstraints=critical,CA:TRUE",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
        ],
    );
}

/// The server's key and its certificate, as `certificate` says, signed by `dir`'s authority.
fn server_certificate(dir: &Path, certificate: Certificate) {
    openssl(
        dir,
        &[
            "req",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:prime256v1",
            "-nodes",
            "-keyout",
            "server.key",
            "-out",
            "server.csr",
            "-subj",
            "/CN=symphony hosted tests",
        ],
    );
    let names = match certificate {
        Certificate::OtherName => "subjectAltName=DNS:elsewhere.invalid,IP:192.0.2.1",
        _ => "subjectAltName=DNS:localhost,IP:127.0.0.1",
    };
    std::fs::write(dir.join("san.cnf"), names).unwrap();
    let mut args = vec![
        "x509",
        "-req",
        "-in",
        "server.csr",
        "-CA",
        "ca.pem",
        "-CAkey",
        "ca.key",
        "-CAcreateserial",
        "-out",
        "server.pem",
        "-extfile",
        "san.cnf",
    ];
    if certificate == Certificate::Expired {
        args.extend([
            "-not_before",
            "20200101000000Z",
            "-not_after",
            "20200102000000Z",
        ]);
    } else {
        args.extend(["-days", "3"]);
    }
    openssl(dir, &args);
}

/// The server of `major` that presents `certificate`, started once for the binary.
pub fn cluster_with(major: u32, certificate: Certificate) -> &'static Cluster {
    cluster_on(major, certificate, Loopback::V4)
}

/// [`cluster_with`], the server listening on `loopback` alone.
pub fn cluster_on(major: u32, certificate: Certificate, loopback: Loopback) -> &'static Cluster {
    let mut clusters = CLUSTERS.lock().unwrap_or_else(|held| held.into_inner());
    static AT_EXIT: std::sync::Once = std::sync::Once::new();
    AT_EXIT.call_once(|| crate::store::on_exit(stop));
    let started = clusters
        .entry((major, certificate, loopback))
        .or_insert_with(|| {
            start(major, certificate, loopback).map(|cluster| &*Box::leak(Box::new(cluster)))
        });
    match started {
        Ok(cluster) => cluster,
        Err(why) => panic!("{why}"),
    }
}

/// At the test binary's exit: each server stopped, by its shell's input closed, and its folders
/// removed.
extern "C" fn stop() {
    let clusters = CLUSTERS.lock().unwrap_or_else(|held| held.into_inner());
    for cluster in clusters.values().flatten() {
        let mut shell = cluster
            .shell
            .lock()
            .unwrap_or_else(|held| held.into_inner());
        drop(shell.stdin.take());
        let _ = shell.wait();
        let _ = std::fs::remove_dir_all(&cluster.dir);
        let _ = std::fs::remove_dir_all(&cluster.socket);
    }
}

/// The server of `major` with a certificate a client verifies.
pub fn cluster(major: u32) -> &'static Cluster {
    cluster_with(major, Certificate::Good)
}

fn start(major: u32, certificate: Certificate, loopback: Loopback) -> Result<Cluster, String> {
    let bin = programs(major);
    let tag = format!(
        "{major}-{}{}-{}",
        certificate as u8,
        loopback as u8,
        std::process::id()
    );
    let (address, block) = loopback.address();
    let dir = std::env::temp_dir().join(format!("board-hosted-{tag}"));
    // A socket's path is short: under /tmp, as the board's own PostgreSQL keeps its.
    let socket = PathBuf::from(format!("/tmp/bh-{tag}"));
    for folder in [&dir, &socket] {
        if folder.exists() {
            std::fs::remove_dir_all(folder).unwrap();
        }
        std::fs::create_dir_all(folder).unwrap();
    }
    authority(&dir, "ca");
    authority(&dir, "other");
    server_certificate(&dir, certificate);
    let data = dir.join("data");
    let mut initdb = Command::of(bin.join("initdb"));
    initdb.arg("-D").arg(&data).args([
        "--encoding=UTF8",
        "--auth-local=trust",
        "--auth-host=scram-sha-256",
        "--no-instructions",
    ]);
    initdb.arg(format!("--username={}", pg::USER));
    if major >= 17 {
        initdb.args([
            "--no-locale",
            "--locale-provider=builtin",
            "--builtin-locale=C.UTF-8",
        ]);
    } else {
        // What a managed PostgreSQL 16 is commonly provisioned with: the C library's locale.
        initdb.arg("--locale=en_US.UTF-8");
    }
    run(&mut initdb);
    let port = {
        let held = std::net::TcpListener::bind((address, 0)).unwrap();
        held.local_addr().unwrap().port()
    };
    let at = |name: &str| dir.join(name).display().to_string();
    let (tls, host) = match certificate {
        Certificate::None => ("off", "host"),
        _ => ("on", "hostssl"),
    };
    std::fs::write(
        data.join("postgresql.conf"),
        format!(
            "listen_addresses = '{address}'\nport = {port}\n\
             unix_socket_directories = '{}'\nmax_connections = 60\n\
             ssl = {tls}\nssl_cert_file = '{}'\nssl_key_file = '{}'\n\
             password_encryption = 'scram-sha-256'\nfsync = off\n\
             log_connections = on\nlog_line_prefix = '%m [%p] %u@%d '\n",
            socket.display(),
            at("server.pem"),
            at("server.key"),
        ),
    )
    .unwrap();
    // The operator's socket, and TLS with a password for everyone else; two logins the server is
    // told to let in by weaker means, which a client must refuse (`refusals`).
    std::fs::write(
        data.join("pg_hba.conf"),
        format!(
            "local all {} trust\n\
             {host} all weak_trust {block} trust\n\
             {host} all weak_md5 {block} md5\n\
             {host} all weak_password {block} password\n\
             {host} all all {block} scram-sha-256\n",
            pg::USER
        ),
    )
    .unwrap();
    // The board's socket client reaches the socket PostgreSQL names for its default port: a link
    // to this server's, which is named for the port it listens on.
    std::os::unix::fs::symlink(
        socket.join(format!(".s.PGSQL.{port}")),
        socket.join(symphony_board::server::postgres::SOCKET_FILE),
    )
    .unwrap();
    // The server under a shell that stops it once its input, this process's pipe, closes.
    let mut shell = symphony_board::sys::Command::of("/bin/sh");
    shell
        .args([
            "-c",
            "\"$1\" -D \"$2\" & server=$!; cat >/dev/null; kill -INT $server; wait $server",
        ])
        .arg("sh")
        .arg(bin.join("postgres"))
        .arg(&data)
        .stdout(symphony_board::sys::Stdio::null())
        .stderr(std::fs::File::create(dir.join("server.log")).unwrap());
    let shell = crate::store::spawn_with_input(&mut shell).unwrap();
    let cluster = Cluster {
        port,
        socket,
        ca: dir.join("ca.pem"),
        other_ca: dir.join("other.pem"),
        dir,
        shell: Mutex::new(shell),
    };
    let up = test_time::wait::until_true("the hosted tests' PostgreSQL to answer", READY, || {
        cluster
            .operator("postgres")
            .request(&[Step::bare("SELECT 1")])
            .is_ok()
    });
    match up {
        Ok(()) => Ok(cluster),
        Err(stall) => Err(format!(
            "PostgreSQL {major} did not start: {stall}\n{}",
            std::fs::read_to_string(cluster.dir.join("server.log")).unwrap_or_default()
        )),
    }
}

/// What a platform provisioned for one project: its login, its password and its database.
pub struct Project {
    pub cluster: &'static Cluster,
    pub name: String,
    pub password: String,
}

impl Cluster {
    /// The operator's client of `database`: the superuser, on the socket.
    pub fn operator(&self, database: &str) -> pg::Client {
        pg::Client::new(&self.socket, database)
    }

    /// One value the operator reads.
    pub fn value(&self, database: &str, sql: &str) -> String {
        let answers = self
            .operator(database)
            .request(&[Step::bare(sql)])
            .unwrap_or_else(|why| panic!("{sql}: {why}"));
        answers[0].value().unwrap_or("").to_string()
    }

    /// The operator's statements, run.
    pub fn operate(&self, database: &str, sql: &str) {
        self.operator(database)
            .simple(sql)
            .unwrap_or_else(|why| panic!("{sql}: {why}"));
    }

    /// A project provisioned as the analysis asks: a login that is no superuser and can make no
    /// database or role, and a UTF-8 database it owns and no other login may connect to.
    pub fn project(&'static self) -> Project {
        let name = format!(
            "p{}_{}",
            std::process::id(),
            NAMES.fetch_add(1, Ordering::SeqCst)
        );
        self.project_of(
            &name,
            "NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS",
            "ENCODING 'UTF8'",
        )
    }

    /// [`Cluster::project`], the login `name` made with `role`'s words and its database with
    /// `database`'s.
    pub fn project_of(&'static self, name: &str, role: &str, database: &str) -> Project {
        let password = format!("pw-{name}-%/@:?# é");
        self.operate(
            "postgres",
            &format!("CREATE ROLE {name} LOGIN PASSWORD '{password}' {role}"),
        );
        // CREATE DATABASE runs in no transaction block: a query of its own.
        self.operate(
            "postgres",
            &format!("CREATE DATABASE {name} OWNER {name} {database} TEMPLATE template0"),
        );
        self.operate(
            "postgres",
            &format!("REVOKE ALL ON DATABASE {name} FROM PUBLIC"),
        );
        Project {
            cluster: self,
            name: name.to_string(),
            password,
        }
    }

    /// How many sessions `login` holds on the server now, by the server's own count.
    pub fn sessions(&self, login: &str) -> i64 {
        self.value(
            "postgres",
            &format!("SELECT count(*) FROM pg_stat_activity WHERE usename = '{login}'"),
        )
        .parse()
        .unwrap()
    }

    /// Every session of `login` ended by the server, as a failover or an operator ends them.
    pub fn terminate(&self, login: &str) {
        self.value(
            "postgres",
            &format!(
                "SELECT count(pg_terminate_backend(pid)) FROM pg_stat_activity \
                 WHERE usename = '{login}'"
            ),
        );
        test_time::wait::until_true("the login's sessions to end", READY, || {
            self.sessions(login) == 0
        })
        .unwrap();
    }
}

/// `text` as a part of a URL: everything but letters, digits, `-`, `.` and `_` escaped.
pub fn escaped(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The settings every test starts from: a budget of `web` (two processes of three), `worker`
/// (one of two) and maintenance, with `more` keys before the budget.
pub fn settings(more: &str) -> Settings {
    budgeted(more, 1)
}

/// [`settings`], the budget's maintenance connections `maintenance`.
pub fn budgeted(more: &str, maintenance: u32) -> Settings {
    toml::from_str(&format!(
        "url_env = \"URL\"\nca_file_env = \"CA\"\nclass_env = \"CLASS\"\n{more}\n\
         [budget]\nrole_connection_cap = {}\nmaintenance = {maintenance}\nheadroom = 2\n\
         [budget.classes]\nweb = {{ processes = 2, pool_max = 3 }}\n\
         worker = {{ processes = 1, pool_max = 2 }}\n\
         solo = {{ processes = 1, pool_max = 1 }}\n",
        11 + maintenance
    ))
    .unwrap()
}

/// An environment that holds `vars`.
pub fn environment(vars: &[(&str, String)]) -> Box<Environment> {
    let held: BTreeMap<String, String> = vars
        .iter()
        .map(|(name, value)| ((*name).to_string(), value.clone()))
        .collect();
    Box::new(move |name| held.get(name).cloned())
}

impl Project {
    /// The project's URL at `host`, as its deployment's secret holds it.
    pub fn url_at(&self, host: &str) -> String {
        format!(
            "postgresql://{}:{}@{host}:{}/{}",
            self.name,
            escaped(&self.password),
            self.cluster.port,
            self.name
        )
    }

    pub fn url(&self) -> String {
        self.url_at("localhost")
    }

    /// The environment of a process of `class` that reaches this project.
    pub fn environment(&self, class: &str) -> Box<Environment> {
        self.environment_at("localhost", class)
    }

    /// [`Project::environment`], the server named `host`: a pool of its own, as another
    /// process of the deployment has.
    pub fn environment_at(&self, host: &str, class: &str) -> Box<Environment> {
        environment(&[
            ("URL", self.url_at(host)),
            ("CA", self.cluster.ca.display().to_string()),
            ("CLASS", class.to_string()),
        ])
    }

    /// A client of this project's board, of `class`.
    pub fn client(&self, class: &str) -> Client {
        Client::connect(&settings(""), self.environment(class))
            .unwrap_or_else(|why| panic!("{why}"))
    }

    /// One value the project's own login reads through `client`.
    pub fn value(client: &Client, sql: &str) -> String {
        let answers = client
            .request(&[Step::bare(sql)])
            .unwrap_or_else(|why| panic!("{sql}: {why}"));
        answers[0].value().unwrap_or("").to_string()
    }
}

/// What a request was refused with.
pub fn refused(answered: Result<Vec<pg::Answer>, symphony_board::rpc::DbError>) -> String {
    match answered {
        Err(symphony_board::rpc::DbError::Failed(said)) => said,
        other => panic!("refused, not {other:?}"),
    }
}
