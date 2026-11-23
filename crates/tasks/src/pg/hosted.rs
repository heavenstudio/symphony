//! A board in a managed PostgreSQL: a database the platform made for the project, reached over
//! TCP with verified TLS as the project's own restricted login. Built only with the
//! `hosted-postgresql` feature; the socket client ([`super::Client`]) and a local board are as
//! they are without it.
//!
//! A [`Client`] is a handle on its process's one pool for that database and class ([`pool`]): every client
//! of the same endpoint, database and login shares it, so another client, a bounded read, a
//! callback and a health read make no pool of their own. The pool is the class's share of the
//! deployment's budget ([`super::external`]): `pool_max` connections here, and each connection
//! holding one of the class's slots in the database ([`session`]). The secrets are read each
//! time a connection is made ([`secret`]); a rotation is a new generation, whose first reading
//! retires the idle connections of the one before and never touches one a request holds.
//!
//! Nothing here makes a database, a role or a server, and nothing falls back to another board
//! when this one cannot be reached. An error says its kind, its SQLSTATE and where the server's
//! log has it ([`Refusal`]), never the server's or the driver's own words, a statement, a
//! parameter or any part of the URL; a statement's own raised words are the board's and are kept.

pub mod pool;
mod secret;
mod session;

pub use pool::Counts;
pub use secret::Environment;
pub use session::SUPPORTED;

use super::external::{Settings, Share, TABLE};
use super::{Answer, Step};
use crate::db::DbError;
use crate::lock::Recovered as _;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::time::{Duration, Instant};

/// Why a hosted board did not answer, by kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The settings' key, and what is wrong with what it names.
    Configuration(&'static str, &'static str),
    /// The server could not be reached, or refused the connection.
    Unavailable(&'static str),
    /// The server, database or login is not one this build takes.
    Unsupported(String),
    /// No connection within the budget.
    Capacity(&'static str),
    /// The server refused a statement: its SQLSTATE, and where its log has it.
    Server { state: String, at: String },
    /// The connection ended before any commit was sent: nothing was committed.
    Lost(&'static str),
    /// The connection ended once a commit was sent: it may or may not have committed.
    Unknown(&'static str),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Configuration(key, why) => write!(f, "{TABLE} {key} {why}"),
            Refusal::Unavailable(why) => write!(f, "the hosted board's PostgreSQL: {why}"),
            Refusal::Unsupported(why) => {
                write!(
                    f,
                    "the hosted board's PostgreSQL is not one a board is kept in: {why}"
                )
            }
            Refusal::Capacity(why) => {
                write!(
                    f,
                    "the hosted board's PostgreSQL has no connection for this request: {why}"
                )
            }
            Refusal::Server { state, at } => write!(
                f,
                "the hosted board's PostgreSQL refused a statement: {} [{at}] (SQLSTATE {state})",
                class(state)
            ),
            Refusal::Lost(why) => write!(
                f,
                "the hosted board's PostgreSQL: {why}; nothing of the request was committed, and \
                 it was not sent again"
            ),
            Refusal::Unknown(why) => write!(
                f,
                "the hosted board's PostgreSQL: {why}; whether the request was committed is not \
                 known, and it was not sent again: read the board before deciding"
            ),
        }
    }
}

impl From<Refusal> for DbError {
    fn from(refusal: Refusal) -> DbError {
        DbError::Failed(refusal.to_string())
    }
}

/// What a SQLSTATE's class says, in fixed words.
fn class(state: &str) -> &'static str {
    match state.get(..2).unwrap_or("") {
        "08" => "a connection exception",
        "22" => "a value a statement could not take",
        "23" => "a constraint a row would break",
        "25" => "a statement its transaction's state does not allow",
        "28" => "a login it does not accept",
        "3D" | "3F" => "a database or schema that is not there",
        "40" => "a transaction it rolled back",
        "42" => "a statement it cannot run as this login, or cannot read",
        "53" => "a resource it has run out of",
        "54" => "a limit of its own",
        "55" => "an object not in a state for it",
        "57" => "an operator's intervention or a cancelled statement",
        "58" | "XX" => "an error of its own",
        _ => "an error",
    }
}

/// Where the server's log has what a connection did: its backend process and the second.
fn correlation(backend: i32) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    format!("backend {backend} at {now}")
}

/// How a request ended without its answers: what the store is told, whether the connection
/// may be lent again, and whether the server answered it, with what `said` says (the board's
/// health counts a request with no answer as failed, never one the server refused).
struct Ended {
    said: DbError,
    clean: bool,
    answered: bool,
}

impl Ended {
    /// A request whose connection ended, or whose wait ran out, before the server answered it.
    fn broken(refusal: Refusal) -> Ended {
        Ended {
            said: refusal.into(),
            clean: false,
            answered: false,
        }
    }

    /// A request the server answered with an error, the connection `clean` once its transaction
    /// is rolled back.
    fn refused(said: DbError, clean: bool) -> Ended {
        Ended {
            said,
            clean,
            answered: true,
        }
    }
}

/// The one thread a pool's connections are driven on, for the pool's life.
struct Driver {
    handle: tokio::runtime::Handle,
    /// Dropped to end the thread.
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Driver {
    fn start() -> Result<Driver, Refusal> {
        let unstarted = Refusal::Unavailable("no thread to drive its connections on");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|_| unstarted.clone())?;
        let handle = runtime.handle().clone();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let thread = std::thread::Builder::new()
            .name("board-pg-hosted".into())
            .spawn(move || {
                // Ends when the driver is dropped, which drops what it still drives.
                let _ = runtime.block_on(stopped);
            })
            .map_err(|_| unstarted)?;
        Ok(Driver {
            handle,
            stop: Some(stop),
            thread: Some(thread),
        })
    }

    /// `work` run on the driver's thread, and its outcome waited for here.
    fn run<T: Send + 'static>(
        &self,
        work: impl Future<Output = T> + Send + 'static,
    ) -> Result<T, Refusal> {
        let (done, outcome) = mpsc::sync_channel(1);
        self.handle.spawn(async move {
            let _ = done.send(work.await);
        });
        outcome
            .recv()
            .map_err(|_| Refusal::Unavailable("the thread that drives its connections ended"))
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Why a secret that came to name another board is refused.
const MOVED: &str = "now names another endpoint, database or login than this process connected \
    to; a board is moved by a deployment, never under a running process";

/// How a pool makes its connections: the settings and the environment its secrets are read
/// from, its class's share, and the generation the last reading was.
struct Hosting {
    settings: Settings,
    env: Box<Environment>,
    share: Share,
    identity: String,
    generation: Mutex<[u8; 32]>,
    driver: Driver,
}

impl Hosting {
    /// The secrets read again, held to the pool's own endpoint, database and login; a new
    /// generation is taken, which retires the idle connections of the one before.
    fn read(&self) -> Result<secret::Target, Refusal> {
        let target = secret::Target::read(&self.settings, &*self.env)?;
        if target.identity != self.identity {
            return Err(Refusal::Configuration("url_env", MOVED));
        }
        *self.generation.lock().recover() = target.generation;
        Ok(target)
    }
}

impl pool::Connector for Hosting {
    type Connection = Arc<session::Session>;
    type Error = Refusal;

    fn connect(&self, within: Duration) -> Result<Arc<session::Session>, Refusal> {
        let target = self.read()?;
        let share = self.share.clone();
        let connecting = async move {
            match tokio::time::timeout(within, session::Session::connect(target, share)).await {
                Ok(connected) => connected,
                Err(_) => Err(Refusal::Unavailable("the server did not answer in time")),
            }
        };
        self.driver.run(connecting)?.map(Arc::new)
    }

    fn usable(&self, connection: &Arc<session::Session>) -> bool {
        connection.open() && connection.generation == *self.generation.lock().recover()
    }

    fn close(&self, connection: Arc<session::Session>) {
        if let Some(session) = Arc::into_inner(connection) {
            let _ = self.driver.run(session.close());
        }
    }
}

struct Shared {
    pool: pool::Pool<Hosting>,
    log: Arc<crate::board_health::QueryLog>,
    label: String,
    class: String,
}

/// Every pool of this process, by its identity and its class: one per endpoint, database, login
/// and class of the budget, so a connect is never handed the pool, the share or the slots of a
/// class it did not name.
static POOLS: Mutex<Vec<Pooled>> = Mutex::new(Vec::new());

/// A pool of this process, with the identity and the class it is found by.
type Pooled = ((String, String), Weak<Shared>);

/// A hosted board's database, through its process's pool ([`self`]).
pub struct Client {
    shared: Arc<Shared>,
    acquire: Duration,
    connect: Duration,
    timeout: Duration,
    requests: AtomicUsize,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "hosted::Client({})", self.shared.label)
    }
}

impl Client {
    /// The board `settings` names, its secrets read from `env`: the table judged, the secrets
    /// read once to know which board this is, and this process's pool for it found or made.
    /// No connection is made yet.
    pub fn connect(settings: &Settings, env: Box<Environment>) -> Result<Client, Refusal> {
        settings.validate().map_err(|_| {
            Refusal::Configuration(
                "budget",
                "or another key is refused: run the settings check",
            )
        })?;
        let class = env(&settings.class_env)
            .filter(|class| !class.is_empty())
            .ok_or(Refusal::Configuration(
                "class_env",
                "names a variable that is not set",
            ))?;
        let share = settings.share(&class).map_err(|_| {
            Refusal::Configuration("class_env", "names a class the budget does not have")
        })?;
        let target = secret::Target::read(settings, &*env)?;
        let seconds = |seconds: u32| Duration::from_secs(seconds.into());
        let client = |shared: Arc<Shared>| Client {
            shared,
            acquire: seconds(settings.acquire_timeout_seconds),
            connect: seconds(settings.connect_timeout_seconds),
            timeout: seconds(settings.request_timeout_seconds),
            requests: AtomicUsize::new(0),
        };
        let mut pools = POOLS.lock().recover();
        pools.retain(|(_, pool)| pool.strong_count() > 0);
        let key = (target.identity.clone(), class.clone());
        if let Some(shared) = pools
            .iter()
            .find(|(of, _)| *of == key)
            .and_then(|(_, pool)| pool.upgrade())
        {
            return Ok(client(shared));
        }
        let label = format!("postgresql-hosted:{}", target.identity);
        let (max, line) = (share.pool_max as usize, settings.queue_max as usize);
        let shared = Arc::new(Shared {
            log: crate::board_health::queries_of(&label, "", &label),
            pool: pool::Pool::new(
                Hosting {
                    settings: settings.clone(),
                    env,
                    share,
                    identity: target.identity,
                    generation: Mutex::new(target.generation),
                    driver: Driver::start()?,
                },
                max,
                line,
            ),
            label,
            class,
        });
        pools.push((key, Arc::downgrade(&shared)));
        Ok(client(shared))
    }

    /// This client, each request waited for at most `timeout` once it has its connection.
    pub fn waiting(mut self, timeout: Duration) -> Client {
        self.timeout = timeout;
        self
    }

    /// How long each request is waited for once it has its connection.
    pub fn wait(&self) -> Duration {
        self.timeout
    }

    /// A client of its own of the same board, on the same pool.
    pub fn another(&self) -> Client {
        Client {
            shared: Arc::clone(&self.shared),
            acquire: self.acquire,
            connect: self.connect,
            timeout: self.timeout,
            requests: AtomicUsize::new(0),
        }
    }

    /// A client of its own that waits at most `most` for a connection and for each answer, or
    /// this one's waits when they are shorter.
    pub fn bounded(&self, most: Duration) -> Client {
        Client {
            acquire: self.acquire.min(most),
            timeout: self.timeout.min(most),
            ..self.another()
        }
    }

    /// What names this board in an address, a log and a command: a fingerprint of its endpoint,
    /// database and login, none of which it says.
    /// The class of the budget this client's pool is the share of.
    pub fn class(&self) -> &str {
        &self.shared.class
    }

    pub fn label(&self) -> &str {
        &self.shared.label
    }

    /// The board's request and write telemetry, shared by all its clients.
    pub fn queries(&self) -> Arc<crate::board_health::QueryLog> {
        Arc::clone(&self.shared.log)
    }

    /// How many requests this client has sent.
    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }

    /// What the pool holds now.
    pub fn counts(&self) -> Counts {
        self.shared.pool.counts()
    }

    /// The pool closed to every client of it: its line refused, its idle connections closed,
    /// and its lent ones waited for at most `within`. Whether none was left.
    pub fn shut_down(&self, within: Duration) -> bool {
        self.shared.pool.shut_down();
        self.shared.pool.drained(within)
    }

    /// The server's major version, read from a connection of the pool.
    pub fn server_major(&self) -> Result<u32, DbError> {
        let mut lease = self.lease()?;
        let major = lease.connection().major;
        lease.keep();
        Ok(major)
    }

    fn lease(&self) -> Result<pool::Lease<'_, Hosting>, DbError> {
        // A rotation is seen before the pool looks at its idle connections; a reading that
        // fails leaves them as they are, and the next connection made says why. But a secret
        // that names another board is refused at once: an idle connection to the board it no
        // longer names is not the one this process is now told to use.
        if let Err(moved @ Refusal::Configuration(_, MOVED)) = self.shared.pool.connector().read() {
            return Err(moved.into());
        }
        self.shared
            .pool
            .acquire(self.acquire, self.connect)
            .map_err(|refused| {
                DbError::from(match refused {
                    pool::Refused::LineFull => {
                        Refusal::Capacity("as many requests wait as the pool's line holds")
                    }
                    pool::Refused::Deadline => {
                        Refusal::Capacity("none came free within the wait for one")
                    }
                    pool::Refused::Closed => Refusal::Capacity("the pool is shut down"),
                    pool::Refused::Forked => Refusal::Capacity(
                        "this process is a fork of the one that connected, and makes its own",
                    ),
                    pool::Refused::Nested => Refusal::Capacity(
                        "this thread holds one already, and would wait behind itself",
                    ),
                    pool::Refused::Connect(refusal) => refusal,
                })
            })
    }

    /// Each of `steps`' answers, in order, all of them in one transaction ([`session`]).
    #[track_caller]
    pub fn request(&self, steps: &[Step]) -> Result<Vec<Answer>, DbError> {
        let owned: Vec<(String, Option<String>)> = steps
            .iter()
            .map(|step| (step.sql.to_string(), step.param.map(str::to_string)))
            .collect();
        let write = steps.iter().any(|step| crate::schema::mutates(step.sql));
        let within = self.timeout;
        self.measured(write, move |session| async move {
            session.request(owned, within).await
        })
    }

    /// `sql`'s statements, which take no parameter and say their own transactions, as one
    /// simple query: a migration. It answers no rows.
    #[track_caller]
    pub fn simple(&self, sql: &str) -> Result<Vec<Answer>, DbError> {
        let (sql, within) = (sql.to_string(), self.timeout);
        self.measured(crate::schema::mutates(&sql), move |session| async move {
            session.simple(sql, within).await.map(|()| Vec::new())
        })
    }

    /// `work` on a connection of the pool, counted once in the board's telemetry, failed only when
    /// it got no answer (no connection, one lost, a wait run out), never for an error the server
    /// answered: the connection returned to the pool only when the work left it clean.
    #[track_caller]
    fn measured<F, W>(&self, write: bool, work: F) -> Result<Vec<Answer>, DbError>
    where
        F: FnOnce(Arc<session::Session>) -> W,
        W: Future<Output = Result<Vec<Answer>, Ended>> + Send + 'static,
    {
        let source = std::panic::Location::caller();
        self.requests.fetch_add(1, Ordering::SeqCst);
        let began = Instant::now();
        // The outer error is a request with no answer, the inner the server's answer.
        let sent = (|| -> Result<Result<Vec<Answer>, DbError>, DbError> {
            let mut lease = self.lease()?;
            let session = Arc::clone(lease.connection());
            let driver = &self.shared.pool.connector().driver;
            match driver.run(work(session)) {
                Ok(Ok(answers)) => {
                    lease.keep();
                    Ok(Ok(answers))
                }
                Ok(Err(ended)) => {
                    if ended.clean {
                        lease.keep();
                    }
                    if ended.answered {
                        Ok(Err(ended.said))
                    } else {
                        Err(ended.said)
                    }
                }
                Err(refusal) => Err(refusal.into()),
            }
        })();
        self.shared.log.record(crate::board_health::Query {
            at: crate::sweep::trigger::now(),
            ms: began.elapsed().as_secs_f64() * 1000.0,
            source,
            write,
            failed: sent.is_err(),
        });
        sent.flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_says_its_kind_its_state_and_where_the_log_has_it_and_ends_as_the_store_reads_one()
    {
        let server = Refusal::Server {
            state: "42501".into(),
            at: "backend 77 at 1700000000".into(),
        };
        assert_eq!(
            server.to_string(),
            "the hosted board's PostgreSQL refused a statement: a statement it cannot run as \
             this login, or cannot read [backend 77 at 1700000000] (SQLSTATE 42501)"
        );
        let DbError::Failed(said) = DbError::from(server) else {
            panic!("a refusal is a failure");
        };
        assert!(!crate::pg::no_database(&said));
        let lost = Refusal::Lost("the connection was lost before any commit").to_string();
        assert!(
            lost.contains("nothing of the request was committed"),
            "{lost}"
        );
        let unknown = Refusal::Unknown("the server did not answer the commit in time").to_string();
        assert!(
            unknown.contains("is not known") && unknown.contains("not sent again"),
            "{unknown}"
        );
        assert_eq!(
            Refusal::Configuration("url_env", "names a variable that is not set").to_string(),
            "[board.external_postgresql] url_env names a variable that is not set"
        );
        for (state, said) in [
            ("08006", "a connection exception"),
            ("23505", "a constraint a row would break"),
            ("53300", "a resource it has run out of"),
            ("3D000", "a database or schema that is not there"),
            ("XX000", "an error of its own"),
            ("", "an error"),
            ("é", "an error"),
        ] {
            assert_eq!(class(state), said);
        }
        assert!(correlation(12).starts_with("backend 12 at "));
    }

    #[test]
    fn clients_of_one_board_share_its_pool_and_another_board_has_its_own() {
        let dir = secret::tests::temp("pools");
        let settings = secret::tests::settings("");
        let at = |url: &str| Client::connect(&settings, secret::tests::env(&dir, &[("URL", url)]));
        let first = at("postgresql://board:one@db.invalid/board_a").unwrap();
        let rotated = at("postgresql://board:two@db.invalid/board_a").unwrap();
        let other = at("postgresql://board:one@db.invalid/board_b").unwrap();
        assert!(Arc::ptr_eq(&first.shared, &rotated.shared));
        assert!(Arc::ptr_eq(&first.shared, &first.another().shared));
        assert!(Arc::ptr_eq(
            &first.shared,
            &first.bounded(Duration::from_secs(1)).shared
        ));
        assert!(!Arc::ptr_eq(&first.shared, &other.shared));
        // Another class of the same board has a pool, a share and slots of its own.
        let maintenance = Client::connect(
            &settings,
            secret::tests::env(
                &dir,
                &[
                    ("URL", "postgresql://board:one@db.invalid/board_a"),
                    ("CLASS", crate::pg::external::MAINTENANCE),
                ],
            ),
        )
        .unwrap();
        assert!(!Arc::ptr_eq(&first.shared, &maintenance.shared));
        assert_eq!(maintenance.class(), crate::pg::external::MAINTENANCE);
        assert_eq!(first.class(), rotated.class());
        assert_ne!(first.class(), maintenance.class());
        assert_eq!(first.label(), maintenance.label());
        assert_eq!(first.label(), rotated.label());
        assert!(first.label().starts_with("postgresql-hosted:"));
        let shown = format!("{first:?} {}", first.label());
        for secret in ["one", "db.invalid", "board_a"] {
            assert!(!shown.contains(secret), "{shown}");
        }
        assert_eq!(first.counts(), Counts::default());
        assert_eq!(
            first.bounded(Duration::from_secs(1)).timeout,
            Duration::from_secs(1)
        );
        assert_eq!(
            first.another().waiting(Duration::from_secs(7)).timeout,
            Duration::from_secs(7)
        );
    }

    #[test]
    fn a_process_of_no_class_or_an_unknown_one_connects_to_nothing() {
        let dir = secret::tests::temp("class");
        let settings = secret::tests::settings("");
        let url = ("URL", "postgresql://board:one@db.invalid/board_c");
        for (class, why) in [("", "is not set"), ("cron", "the budget does not have")] {
            let refused = Client::connect(
                &settings,
                secret::tests::env(&dir, &[url, ("CLASS", class)]),
            )
            .unwrap_err();
            assert!(refused.to_string().contains(why), "{refused}");
        }
        let mut no_budget = settings.clone();
        no_budget.budget = None;
        let refused = Client::connect(&no_budget, secret::tests::env(&dir, &[url])).unwrap_err();
        assert!(refused.to_string().contains("settings check"), "{refused}");
    }

    #[test]
    fn an_unreachable_board_fails_by_its_kind_within_its_wait_and_is_not_tried_again() {
        let dir = secret::tests::temp("down");
        // A port nothing listens on: bound, then let go.
        let port = {
            let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            held.local_addr().unwrap().port()
        };
        let url = format!("postgresql://board:hunter2@127.0.0.1:{port}/board_d");
        let client = Client::connect(
            &secret::tests::settings("connect_timeout_seconds = 5"),
            secret::tests::env(&dir, &[("URL", &url)]),
        )
        .unwrap();
        let Err(DbError::Failed(said)) = client.request(&[Step::bare("SELECT 1")]) else {
            panic!("nothing listens there");
        };
        assert_eq!(
            said,
            "the hosted board's PostgreSQL: the server cannot be reached"
        );
        assert_eq!(client.requests(), 1);
        assert_eq!(
            client.counts(),
            Counts::default(),
            "nothing is left counted"
        );
        assert!(client.shut_down(Duration::from_secs(5)));
        let Err(DbError::Failed(closed)) = client.simple("SELECT 1") else {
            panic!("the pool is shut down");
        };
        assert!(closed.contains("the pool is shut down"), "{closed}");
    }
}
