//! Who may reach the Symphony, judged on the client's own address before any route answers.
//! Loopback is allowed without asking; an address in the Tailscale ranges only when the last
//! `tailscale whois` that answered for it named a node of this tailnet; anything else is refused.
//! A whois that fails, hangs, names no node or finds no `tailscale` to run is not an answer: the
//! peer's last answer stays, and it is asked again after [`FAILED_TTL`]. Only Tailscale saying the
//! address is no peer refuses a peer allowed before; an address never answered for is refused.
//!
//! Native routes also judge Host here: a loopback name, or one of this machine's own tailnet names
//! as the last `tailscale status` that listed any gave them. A status call that fails, hangs or
//! lists no names is not an answer: the names it last gave stay, and it is asked again after
//! [`FAILED_TTL`]. Only an answer that lists names, none of them this one, refuses a tailnet Host.
//!
//! The gate is the local host's admission, so it also says who a request it lets in is from
//! ([`Gate::visitor`]): the person at the machine for loopback, for one of this machine's own
//! tailnet addresses, and for a node under the machine's own Tailscale login, the user the last
//! status that listed its names gave it (`Self.UserID`); that login's person for a node under
//! another (a whois answer's `UserProfile`); nobody's for a tagged node, or one whose answer names
//! no user. A status call that gives no answer keeps the login it last gave, as it keeps the
//! names. Until one has answered only loopback is the person at the machine, as this machine's
//! own tailnet addresses are known from that answer too. On a machine that is itself tagged, which
//! is under no login, no node but this machine's own is its person.
//!
//! What a visitor reaches is each board's word ([`Gate::member`], the roster): the actor their
//! writes there take, or none for a board that has not added them, which is then to them a project
//! the server does not hold. The person at the machine is on every board, as the gate's person
//! ([`Gate::person`], [`symphony_board::workflow::local_actor`]). The gate on this machine's own
//! Tailscale asks the machine's profiles and each board's people ([`crate::roster`]); a gate made
//! without a roster adds nobody else to any board.

use std::collections::{BTreeSet, HashMap};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use symphony_board::lock::Recovered as _;
use symphony_plugin::Actor;

/// How long a whois answer, yes or no, and a status answer's names are trusted.
pub const TTL: Duration = Duration::from_secs(30);
/// How long a whois or status call that gave no answer stands before it is asked again: seconds,
/// so a tailscaled hiccup neither keeps a new peer out nor keeps a stale answer for the whole TTL.
pub const FAILED_TTL: Duration = Duration::from_secs(3);

/// `tailscale whois --json IP`: its output, `None` when Tailscale says the address is no peer, an
/// error when the lookup itself failed, did not answer in time or found no `tailscale` to run.
pub type Whois = Arc<dyn Fn(&str) -> Result<Option<String>, String> + Send + Sync>;
/// `tailscale status --json`: its output, or an error when the call failed or did not answer in
/// time.
pub type Status = Arc<dyn Fn() -> Result<String, String> + Send + Sync>;
/// The gate's clock.
pub type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;
/// A board's word on a visitor who is not the person at the machine: the actor their writes on
/// the board at the root given take, or none when that board has not added them. It is asked on
/// each request a visitor sends, so it answers from what it holds.
pub type Roster = Arc<dyn Fn(&Path, &Visitor) -> Option<Actor> + Send + Sync>;

/// Who a request the gate lets in is from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Visitor {
    /// The person at the machine: loopback, one of this machine's own tailnet addresses, or a node
    /// under the machine's own Tailscale login (the owner's phone).
    Machine,
    /// A node under another Tailscale login: that login's person.
    Login(Login),
    /// A node under no login, by its name: a tagged node, or one whose answer names no user.
    /// Nobody's.
    Node(String),
}

/// A Tailscale login, as a whois answer's `UserProfile` names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Login {
    /// Tailscale's id of the user.
    pub id: u64,
    /// The login's name (an email, say): the subject a board knows them by.
    pub name: String,
    /// How Tailscale shows them: their display name, else the login's name.
    pub display: String,
}

/// The client-address gate, with each tailnet peer's last whois answer, and the Host gate, with
/// this machine's tailnet names and login as the last status answer that listed any gave them.
pub struct Gate {
    whois: Whois,
    status: Status,
    clock: Clock,
    person: Actor,
    roster: Roster,
    /// The machine folder whose profiles the roster reads, and the People page reads and writes.
    machine: Option<PathBuf>,
    seen: Mutex<HashMap<IpAddr, Kept<Option<Peer>>>>,
    own: Mutex<Kept<Own>>,
}

/// A node of this tailnet, as a whois answer names it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Peer {
    /// Its name, without the trailing dot.
    node: String,
    /// The login it is under: none for a tagged node, or an answer that names no user.
    login: Option<Login>,
}

/// This machine, as a status answer that lists its names gives it.
#[derive(Debug, PartialEq, Eq)]
struct Own {
    /// Its tailnet names ([`names_in`]).
    names: BTreeSet<String>,
    /// The Tailscale user it is under: none when it is tagged, or the answer names none.
    user: Option<u64>,
}

/// The last answer a call gave, kept through the calls after it that gave none.
struct Kept<T> {
    /// That answer, and when it was asked.
    known: Option<(Instant, T)>,
    /// When the last call that gave no answer was asked.
    failed: Option<Instant>,
}

impl<T> Default for Kept<T> {
    fn default() -> Kept<T> {
        Kept {
            known: None,
            failed: None,
        }
    }
}

impl<T> Kept<T> {
    /// Whether the call is made again at `now`: once the answer's TTL is spent, or with none
    /// known, and never within FAILED_TTL of a call that gave none.
    fn due(&self, now: Instant) -> bool {
        let spent = |at: Instant, ttl: Duration| now.saturating_duration_since(at) >= ttl;
        self.known.as_ref().is_none_or(|(at, _)| spent(*at, TTL))
            && self.failed.is_none_or(|at| spent(at, FAILED_TTL))
    }

    /// Take a call's outcome: an answer replaces the known one; `None`, a call that gave no
    /// answer, leaves it as it is.
    fn record(&mut self, now: Instant, answer: Option<T>) {
        match answer {
            Some(answer) => {
                self.known = Some((now, answer));
                self.failed = None;
            }
            None => self.failed = Some(now),
        }
    }

    /// The answer kept, when a call has given one.
    fn answer(&self) -> Option<&T> {
        self.known.as_ref().map(|(_, answer)| answer)
    }
}

impl Gate {
    /// A gate that asks `whois` about peers and this machine's own `tailscale status` for its names.
    pub fn new(whois: Whois, clock: Clock) -> Gate {
        Gate {
            whois,
            status: Arc::new(tailscale_status),
            clock,
            person: symphony_board::workflow::local_actor(),
            roster: Arc::new(|_, _| None),
            machine: None,
            seen: Mutex::new(HashMap::new()),
            own: Mutex::new(Kept::default()),
        }
    }

    /// The same gate, asking `status` for this machine's tailnet names and login.
    #[must_use]
    pub fn with_status(self, status: Status) -> Gate {
        Gate { status, ..self }
    }

    /// The same gate, admitting the person at the machine as `person`: a test host's actor in
    /// place of the local one.
    #[must_use]
    pub fn admitting(self, person: Actor) -> Gate {
        Gate { person, ..self }
    }

    /// The same gate, asking `roster` which boards have added a visitor who is not the person at
    /// the machine: the machine's people ([`crate::roster::of`]), or a test host's.
    #[must_use]
    pub fn with_roster(self, roster: Roster) -> Gate {
        Gate { roster, ..self }
    }

    /// The same gate over the machine folder `machine`: its profiles the People page's, and its
    /// people the roster ([`crate::roster::of`]).
    #[must_use]
    pub fn with_machine(self, machine: PathBuf) -> Gate {
        Gate {
            roster: crate::roster::of(machine.clone()),
            machine: Some(machine),
            ..self
        }
    }

    /// The machine folder the profiles are kept in: none for a gate given none, whose roster adds
    /// nobody but as a test's says.
    pub fn machine(&self) -> Option<&Path> {
        self.machine.as_deref()
    }

    /// The person at the machine: the actor their writes take.
    pub fn person(&self) -> &Actor {
        &self.person
    }

    /// Whether `visitor` is on the board at `root`, and as whom: the person at the machine is on
    /// every board, as [`Gate::person`]; anyone else as the roster says, on none by default.
    pub fn member(&self, root: &Path, visitor: &Visitor) -> Option<Actor> {
        match visitor {
            Visitor::Machine => Some(self.person.clone()),
            other => (self.roster)(root, other),
        }
    }

    /// The gate on this machine's own Tailscale.
    pub fn tailscale() -> Gate {
        Gate::tailscale_within(CLI_TIMEOUT)
    }

    /// [`Gate::tailscale`], each `tailscale whois` given `patience` to answer where the server gives
    /// it [`CLI_TIMEOUT`]: a test's, on a machine whose load a deadline must not judge. Its machine
    /// folder is the one the environment names, its roster that folder's people
    /// ([`crate::roster`]).
    pub fn tailscale_within(patience: Duration) -> Gate {
        let gate = Gate::new(
            Arc::new(move |ip: &str| whois_by(find_tailscale(), ip, patience)),
            Arc::new(Instant::now),
        );
        match crate::roster::this_machine() {
            Some(machine) => gate.with_machine(machine),
            None => gate,
        }
    }

    /// Host validation for the routes served here. A loopback client can still be a browser
    /// rebound from another site's name, so its address alone is insufficient.
    pub fn allows_host(&self, host: &str) -> bool {
        let name = if let Some(rest) = host.strip_prefix('[') {
            let Some((name, port)) = rest.split_once(']') else {
                return false;
            };
            if !port.is_empty()
                && !port
                    .strip_prefix(':')
                    .is_some_and(|p| p.parse::<u16>().is_ok())
            {
                return false;
            }
            name
        } else {
            let (name, port) = host
                .split_once(':')
                .map_or((host, None), |(n, p)| (n, Some(p)));
            if port.is_some_and(|p| p.parse::<u16>().is_err()) {
                return false;
            }
            name
        }
        .trim_end_matches('.')
        .to_lowercase();
        if ["localhost", "127.0.0.1", "::1"].contains(&name.as_str()) {
            return true;
        }
        !name.is_empty() && self.own_name(&name)
    }

    /// Whether a request from `ip` comes from this machine itself: loopback, or one of its own
    /// tailnet addresses as the last `tailscale status` that listed any gave them (a browser on this
    /// machine that names it by its tailnet name). A Go to session asks the Herdr app here to show
    /// a pane only for such a request; a page on any other machine is shown the pane's screen.
    pub fn is_this_machine(&self, ip: IpAddr) -> bool {
        let ip = unmapped(ip);
        ip.is_loopback() || (in_tailnet(ip) && self.own_name(&ip.to_string()))
    }

    /// Whether `name`, lowercase, is one of this machine's tailnet names.
    fn own_name(&self, name: &str) -> bool {
        self.own(|own| own.names.contains(name)).unwrap_or(false)
    }

    /// `read` of this machine as the last status answer that listed its names gave it, asking
    /// `status` when that answer is due; none while no call has given one.
    fn own<R>(&self, read: impl FnOnce(&Own) -> R) -> Option<R> {
        let now = (self.clock)();
        // The call is made under the lock, so requests that find it due wait for one call.
        let mut own = self.own.lock().recover();
        if own.due(now) {
            let answer = (self.status)().ok().and_then(|text| own_in(&text));
            own.record(now, answer);
        }
        own.answer().map(read)
    }

    /// Whether a request from `ip` may be answered: loopback, or an address of the tailnet's
    /// ranges whose last whois answer named a node.
    pub fn allows(&self, ip: IpAddr) -> bool {
        let ip = unmapped(ip);
        ip.is_loopback() || (in_tailnet(ip) && self.peer(ip).is_some())
    }

    /// Who a request from `ip` is from, or none when it may not be answered ([`Gate::allows`]).
    /// Loopback asks nothing, and only a node of the tailnet asks `status`.
    pub fn visitor(&self, ip: IpAddr) -> Option<Visitor> {
        let ip = unmapped(ip);
        if ip.is_loopback() {
            return Some(Visitor::Machine);
        }
        if !in_tailnet(ip) {
            return None;
        }
        let peer = self.peer(ip)?;
        let address = ip.to_string();
        let (own_address, own_user) = self
            .own(|own| (own.names.contains(&address), own.user))
            .unwrap_or((false, None));
        Some(match peer.login {
            _ if own_address => Visitor::Machine,
            Some(login) if own_user == Some(login.id) => Visitor::Machine,
            Some(login) => Visitor::Login(login),
            None => Visitor::Node(peer.node),
        })
    }

    /// The node `ip`, an address of the tailnet's ranges, is as the last whois that answered for
    /// it named it, asking `whois` when that answer is due; none when it said no peer, or while no
    /// call has answered.
    fn peer(&self, ip: IpAddr) -> Option<Peer> {
        let now = (self.clock)();
        if let Some(peer) = self.seen.lock().recover().get(&ip)
            && !peer.due(now)
        {
            return peer.answer().cloned().flatten();
        }
        // The lookup is made outside the lock, so one slow peer does not hold up another.
        let answer = verdict((self.whois)(&ip.to_string()));
        let mut seen = self.seen.lock().recover();
        let peer = seen.entry(ip).or_default();
        peer.record(now, answer);
        peer.answer().cloned().flatten()
    }
}

/// What a whois call says of an address: the node of this tailnet it is, `Some(None)` when it is no
/// peer, or `None` when the call gave no answer: it failed (with no `tailscale` to run, too), timed
/// out, or its output names no node.
fn verdict(whois: Result<Option<String>, String>) -> Option<Option<Peer>> {
    match whois {
        Ok(None) => Some(None),
        Ok(Some(output)) => peer_in(&output).map(Some),
        Err(_) => None,
    }
}

/// The node a whois answer names, with the login its `UserProfile` names unless the node is
/// tagged; none when it names no node. A user is named by a non-zero id and a login name.
fn peer_in(answer: &str) -> Option<Peer> {
    let body: serde_json::Value = serde_json::from_str(answer).ok()?;
    let node = body.get("Node").filter(|node| node.is_object())?;
    let profile = &body["UserProfile"];
    let login = profile["ID"]
        .as_u64()
        .filter(|id| *id != 0 && !tagged(node))
        .zip(
            profile["LoginName"]
                .as_str()
                .filter(|name| !name.is_empty()),
        )
        .map(|(id, name)| Login {
            id,
            name: name.to_string(),
            display: profile["DisplayName"]
                .as_str()
                .filter(|display| !display.is_empty())
                .unwrap_or(name)
                .to_string(),
        });
    Some(Peer {
        node: node["Name"]
            .as_str()
            .unwrap_or_default()
            .trim_end_matches('.')
            .to_string(),
        login,
    })
}

/// Whether a node, as a whois or status answer gives it, carries a tag: a tagged node is under no
/// login.
fn tagged(node: &serde_json::Value) -> bool {
    node["Tags"].as_array().is_some_and(|tags| !tags.is_empty())
}

/// This machine as a `tailscale status --json` answer gives it, when the answer lists any of its
/// names: those names, and the user it is under, `Self.UserID` when non-zero and it is untagged.
fn own_in(status: &str) -> Option<Own> {
    let names = names_in(status);
    if names.is_empty() {
        return None;
    }
    let status: serde_json::Value = serde_json::from_str(status).ok()?;
    let me = &status["Self"];
    let user = me["UserID"].as_u64().filter(|id| *id != 0 && !tagged(me));
    Some(Own { names, user })
}

/// The names a `tailscale status --json` answer gives this machine: its MagicDNS name, without the
/// trailing dot, and its tailnet addresses, all lowercase. Garbage gives none, as does an empty
/// name.
fn names_in(status: &str) -> BTreeSet<String> {
    let Ok(status) = serde_json::from_str::<serde_json::Value>(status) else {
        return BTreeSet::new();
    };
    let addresses = |list: &serde_json::Value| {
        list.as_array()
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    status["Self"]["DNSName"]
        .as_str()
        .map(|dns| dns.trim_end_matches('.').to_string())
        .into_iter()
        .chain(addresses(&status["TailscaleIPs"]))
        .chain(addresses(&status["Self"]["TailscaleIPs"]))
        .map(|name| name.to_lowercase())
        .filter(|name| !name.is_empty())
        .collect()
}

/// The address, with an IPv4-mapped IPv6 one unwrapped.
fn unmapped(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        IpAddr::V4(_) => ip,
    }
}

/// Whether `ip` is in Tailscale's ranges: 100.64.0.0/10 and fd7a:115c:a1e0::/48.
pub fn in_tailnet(ip: IpAddr) -> bool {
    match unmapped(ip) {
        IpAddr::V4(v4) => {
            let [first, second, ..] = v4.octets();
            first == 100 && second & 0xc0 == 64
        }
        IpAddr::V6(v6) => v6.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

/// The macOS app's own command line, when `tailscale` is not on the PATH.
const MACOS_APP: &str = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";
const CLI_TIMEOUT: Duration = Duration::from_secs(3);

fn find_tailscale() -> Option<std::path::PathBuf> {
    crate::sys::which("tailscale").or_else(|| {
        let app = std::path::PathBuf::from(MACOS_APP);
        app.is_file().then_some(app)
    })
}

/// `tailscale whois --json IP` run by `binary`, given `patience` to answer: `None` when Tailscale
/// says the address is no peer; an error when there is no command line, or it hangs or fails
/// otherwise (a daemon down): not an answer.
fn whois_by(
    binary: Option<std::path::PathBuf>,
    ip: &str,
    patience: Duration,
) -> Result<Option<String>, String> {
    let binary = binary.ok_or("tailscale is not installed")?;
    let mut command = symphony_board::sys::Command::new(binary);
    command.args(["whois", "--json", ip]);
    let out = crate::sys::output_within(command, patience)?;
    if out.status.success() {
        return Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()));
    }
    let said = String::from_utf8_lossy(&out.stderr);
    if said.contains("peer not found") {
        return Ok(None);
    }
    Err(format!("tailscale whois failed: {}", said.trim()))
}

/// `tailscale status --json`: its output; an error when Tailscale is missing, or the call fails or
/// does not answer within its deadline.
fn tailscale_status() -> Result<String, String> {
    let binary = find_tailscale().ok_or("tailscale is not installed")?;
    let mut command = symphony_board::sys::Command::new(binary);
    command.args(["status", "--json"]);
    let out = crate::sys::output_within(command, CLI_TIMEOUT)?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    Err(format!(
        "tailscale status failed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    ))
}

/// This machine's MagicDNS name, without the trailing dot, when `tailscale status` gives one.
pub fn magicdns_name() -> Option<String> {
    let status: serde_json::Value = serde_json::from_str(&tailscale_status().ok()?).ok()?;
    let name = status
        .get("Self")?
        .get("DNSName")?
        .as_str()?
        .trim_end_matches('.');
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
#[path = "../tests/after_access/fixture.rs"]
mod access_fixture;

#[cfg(test)]
mod visitor_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fake {
        gate: Gate,
        asked: Arc<AtomicUsize>,
        now: Arc<Mutex<Instant>>,
    }

    fn fake(answer: Result<Option<&'static str>, &'static str>) -> Fake {
        let asked = Arc::new(AtomicUsize::new(0));
        let now = Arc::new(Mutex::new(Instant::now()));
        let (counter, clock) = (asked.clone(), now.clone());
        let gate = Gate::new(
            Arc::new(move |_ip: &str| {
                counter.fetch_add(1, Ordering::SeqCst);
                answer
                    .map(|a| a.map(str::to_string))
                    .map_err(str::to_string)
            }),
            Arc::new(move || *clock.lock().unwrap()),
        );
        Fake { gate, asked, now }
    }

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    const PEER: &str = r#"{"Node": {"Name": "phone"}, "UserProfile": {}}"#;

    #[test]
    fn loopback_is_allowed_without_asking_tailscale() {
        let fake = fake(Err("never asked"));
        for address in ["127.0.0.1", "127.8.9.10", "::1", "::ffff:127.0.0.1"] {
            assert!(fake.gate.allows(ip(address)), "{address}");
        }
        assert_eq!(fake.asked.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn an_address_outside_the_tailnet_is_refused_without_asking() {
        let fake = fake(Ok(Some(PEER)));
        for address in [
            "192.168.1.20",
            "10.0.0.3",
            "8.8.8.8",
            "fe80::1",
            "::ffff:192.168.1.20",
        ] {
            assert!(!fake.gate.allows(ip(address)), "{address}");
        }
        assert_eq!(fake.asked.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_tailnet_peer_tailscale_knows_is_allowed_and_its_answer_kept() {
        let fake = fake(Ok(Some(PEER)));
        assert!(fake.gate.allows(ip("100.101.102.103")));
        assert!(fake.gate.allows(ip("100.101.102.103")));
        assert!(fake.gate.allows(ip("fd7a:115c:a1e0::5")));
        assert_eq!(fake.asked.load(Ordering::SeqCst), 2);
        *fake.now.lock().unwrap() += TTL;
        assert!(fake.gate.allows(ip("100.101.102.103")));
        assert_eq!(
            fake.asked.load(Ordering::SeqCst),
            3,
            "asked again once its TTL is spent"
        );
    }

    #[test]
    fn a_tailnet_address_that_is_no_peer_or_answers_garbage_is_refused() {
        assert!(!fake(Ok(None)).gate.allows(ip("100.64.0.9")));
        assert!(!fake(Ok(Some("not json"))).gate.allows(ip("100.64.0.9")));
        assert!(
            !fake(Ok(Some(r#"{"Node": null}"#)))
                .gate
                .allows(ip("100.64.0.9"))
        );
    }

    /// Answers given in turn, one per call; a call past the last fails the test.
    type Script<T> = Arc<Mutex<std::collections::VecDeque<T>>>;

    fn script<T>(answers: impl IntoIterator<Item = T>) -> Script<T> {
        Arc::new(Mutex::new(answers.into_iter().collect()))
    }

    fn next<T>(script: &Script<T>, source: &str) -> T {
        script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| panic!("{source} asked more often than the case allows"))
    }

    /// A gate whose whois source gives `answers` in turn, for any address, one per call, and fails
    /// the test when asked once more; with the clock `now`.
    struct PeerGate {
        gate: Gate,
        left: Script<Step>,
        now: Arc<Mutex<Instant>>,
    }

    /// One call of a [`PeerGate`]'s whois source: its answer, or `None` for a call made with no
    /// `tailscale` to run.
    type Step = Option<Result<Option<String>, String>>;

    impl PeerGate {
        fn new(answers: &[Result<Option<&str>, &str>]) -> PeerGate {
            PeerGate::steps(
                answers
                    .iter()
                    .map(|answer| {
                        Some(
                            answer
                                .map(|output| output.map(str::to_string))
                                .map_err(str::to_string),
                        )
                    })
                    .collect(),
            )
        }

        fn steps(steps: Vec<Step>) -> PeerGate {
            let left = script(steps);
            let now = Arc::new(Mutex::new(Instant::now()));
            let (queue, clock) = (left.clone(), now.clone());
            let gate = Gate::new(
                Arc::new(move |ip: &str| {
                    next(&queue, "whois").unwrap_or_else(|| whois_by(None, ip, CLI_TIMEOUT))
                }),
                Arc::new(move || *clock.lock().unwrap()),
            )
            .with_status(Arc::new(|| panic!("a peer check asks no status")));
            PeerGate { gate, left, now }
        }

        fn after(&self, wait: Duration) -> &PeerGate {
            *self.now.lock().unwrap() += wait;
            self
        }

        fn allows(&self, address: &str) -> bool {
            self.gate.allows(ip(address))
        }

        fn unasked(&self) -> usize {
            self.left.lock().unwrap().len()
        }
    }

    const PHONE: &str = "100.101.102.103";

    #[test]
    fn an_allowed_peer_stays_allowed_through_lookups_that_give_no_answer() {
        let peer = PeerGate::new(&[
            Ok(Some(PEER)),
            Err(TIMED_OUT),
            Ok(Some("")),
            Ok(Some("not json {")),
            Ok(Some(r#"{"Node": null}"#)),
            Err("tailscale whois failed: tailscaled is down"),
            Ok(Some(PEER)),
        ]);
        assert!(peer.allows(PHONE));
        assert!(peer.after(TTL).allows(PHONE), "a timed-out lookup");
        assert_eq!(peer.unasked(), 5);
        assert!(
            peer.after(FAILED_TTL - Duration::from_nanos(1))
                .allows(&format!("::ffff:{PHONE}")),
            "within FAILED_TTL of the timeout"
        );
        assert_eq!(peer.unasked(), 5, "asked again only after FAILED_TTL");
        assert!(
            peer.after(Duration::from_nanos(1)).allows(PHONE),
            "an empty answer"
        );
        assert!(peer.after(FAILED_TTL).allows(PHONE), "garbage");
        assert!(peer.after(FAILED_TTL).allows(PHONE), "no node named");
        assert!(peer.after(FAILED_TTL).allows(PHONE), "a daemon down");
        assert_eq!(peer.unasked(), 1);
        assert!(peer.after(FAILED_TTL).allows(PHONE));
        assert_eq!(peer.unasked(), 0, "the retry that answered");
        assert!(
            peer.after(TTL - Duration::from_nanos(1)).allows(PHONE),
            "its answer is kept for the TTL"
        );
    }

    #[test]
    fn a_lookup_that_says_no_peer_refuses_a_peer_allowed_before() {
        let peer = PeerGate::new(&[Ok(Some(PEER)), Ok(None), Err(TIMED_OUT)]);
        assert!(peer.allows(PHONE));
        assert!(!peer.after(TTL).allows(PHONE), "no peer, after the TTL");
        assert!(
            !peer.after(TTL - Duration::from_nanos(1)).allows(PHONE),
            "the refusal is kept for the TTL"
        );
        assert_eq!(peer.unasked(), 1);
        assert!(
            !peer.after(Duration::from_nanos(1)).allows(PHONE),
            "a timeout keeps the refusal"
        );
        assert_eq!(peer.unasked(), 0);
    }

    #[test]
    fn a_tailscale_that_goes_missing_keeps_a_peer_allowed_until_an_answer_says_no_peer() {
        const GONE: Step = None;
        let answer = |output: Option<&str>| Some(Ok(output.map(str::to_string)));
        let peer = PeerGate::steps(vec![GONE, answer(Some(PEER)), GONE, GONE, answer(None)]);
        assert!(!peer.allows(PHONE), "never answered for, with no tailscale");
        assert!(
            peer.after(FAILED_TTL).allows(PHONE),
            "tailscale is there again"
        );
        assert!(peer.after(TTL).allows(PHONE), "tailscale gone");
        assert!(
            peer.after(FAILED_TTL - Duration::from_nanos(1))
                .allows(PHONE)
        );
        assert_eq!(peer.unasked(), 2, "asked again only after FAILED_TTL");
        assert!(
            peer.after(Duration::from_nanos(1)).allows(PHONE),
            "still gone"
        );
        assert!(
            !peer.after(FAILED_TTL).allows(PHONE),
            "the answer that says no peer"
        );
        assert_eq!(peer.unasked(), 0);
    }

    #[test]
    fn an_address_never_answered_for_is_refused_and_asked_again_after_failed_ttl() {
        let peer = PeerGate::new(&[
            Err(TIMED_OUT),
            Ok(Some("not json {")),
            Ok(Some("")),
            Ok(Some(PEER)),
        ]);
        assert!(!peer.allows(PHONE), "a timed-out lookup");
        assert!(
            !peer
                .after(FAILED_TTL - Duration::from_nanos(1))
                .allows(PHONE)
        );
        assert_eq!(peer.unasked(), 3, "asked once within FAILED_TTL");
        assert!(
            !peer.after(Duration::from_nanos(1)).allows(PHONE),
            "garbage"
        );
        assert!(!peer.after(FAILED_TTL).allows(PHONE), "an empty answer");
        assert_eq!(peer.unasked(), 1, "garbage is asked again after FAILED_TTL");
        assert!(peer.after(FAILED_TTL).allows(PHONE));
        assert_eq!(peer.unasked(), 0);
    }

    /// A gate whose status source gives `answers` in turn, one per call, and fails the test when
    /// asked once more; with the clock `now`.
    struct HostGate {
        gate: Gate,
        left: Script<Result<String, String>>,
        now: Arc<Mutex<Instant>>,
    }

    impl HostGate {
        fn new(answers: &[Result<&str, &str>]) -> HostGate {
            let left = script(
                answers
                    .iter()
                    .map(|answer| answer.map(str::to_string).map_err(str::to_string)),
            );
            let now = Arc::new(Mutex::new(Instant::now()));
            let (queue, clock) = (left.clone(), now.clone());
            let gate = Gate::new(
                Arc::new(|_ip: &str| panic!("a Host check asks no peer")),
                Arc::new(move || *clock.lock().unwrap()),
            )
            .with_status(Arc::new(move || next(&queue, "status")));
            HostGate { gate, left, now }
        }

        fn after(&self, wait: Duration) -> &HostGate {
            *self.now.lock().unwrap() += wait;
            self
        }

        fn allows(&self, host: &str) -> bool {
            self.gate.allows_host(host)
        }

        fn unasked(&self) -> usize {
            self.left.lock().unwrap().len()
        }
    }

    const STATUS: &str = r#"{"Self": {"DNSName": "Mac.Taile6311e.ts.net.", "TailscaleIPs": ["fd7a:115c:a1e0::15"]}, "TailscaleIPs": ["100.83.24.15"]}"#;
    const OTHER: &str = r#"{"Self": {"DNSName": "other.taile6311e.ts.net."}}"#;
    const HOST: &str = "mac.taile6311e.ts.net:8765";
    const TIMED_OUT: &str = "no answer within 3 s";

    #[test]
    fn a_status_answer_lists_the_names_of_this_machine() {
        let names = names_in(STATUS);
        assert_eq!(
            names.into_iter().collect::<Vec<_>>(),
            [
                "100.83.24.15",
                "fd7a:115c:a1e0::15",
                "mac.taile6311e.ts.net"
            ]
        );
        for garbage in [
            "",
            "not json {",
            "[]",
            "{}",
            r#"{"Self": {"DNSName": "", "TailscaleIPs": [""]}}"#,
            r#"{"Self": {"DNSName": "."}, "TailscaleIPs": null}"#,
            r#"{"Self": {"DNSName": 5, "TailscaleIPs": [5, null]}}"#,
        ] {
            assert!(names_in(garbage).is_empty(), "{garbage:?}");
        }
    }

    #[test]
    fn this_machine_is_loopback_or_its_own_tailnet_address_and_never_a_peer() {
        let here = HostGate::new(&[Ok(STATUS)]);
        for own in [
            "127.0.0.1",
            "::1",
            "::ffff:127.0.0.1",
            "100.83.24.15",
            "::ffff:100.83.24.15",
            "fd7a:115c:a1e0::15",
        ] {
            assert!(here.gate.is_this_machine(ip(own)), "{own}");
        }
        for other in [
            "100.83.24.16",
            "fd7a:115c:a1e0::16",
            "192.168.1.15",
            "8.8.8.8",
        ] {
            assert!(!here.gate.is_this_machine(ip(other)), "{other}");
        }
        assert_eq!(here.unasked(), 0, "one status answer serves every look");
        // A status that never answered names nothing of this machine but loopback.
        let silent = HostGate::new(&[Err(TIMED_OUT)]);
        assert!(!silent.gate.is_this_machine(ip("100.83.24.15")));
        assert!(silent.gate.is_this_machine(ip("127.0.0.1")));
    }

    #[test]
    fn a_status_that_answers_then_times_out_keeps_the_names_it_gave() {
        let names = HostGate::new(&[Ok(STATUS), Err(TIMED_OUT), Err(TIMED_OUT), Ok(STATUS)]);
        assert!(names.allows(HOST));
        assert!(names.after(TTL).allows(HOST), "a timed-out refresh");
        assert_eq!(names.unasked(), 2);
        for host in [
            "100.83.24.15:8765",
            "[fd7a:115c:a1e0::15]:8765",
            "MAC.taile6311e.ts.net.",
        ] {
            assert!(
                names.allows(host),
                "{host} within FAILED_TTL of the failure"
            );
        }
        assert_eq!(names.unasked(), 2, "asked again only after FAILED_TTL");
        assert!(names.after(FAILED_TTL).allows(HOST), "a second timeout");
        assert_eq!(names.unasked(), 1);
        assert!(names.after(FAILED_TTL).allows(HOST));
        assert_eq!(names.unasked(), 0, "the retry that answered");
        assert!(
            names.after(TTL - Duration::from_nanos(1)).allows(HOST),
            "its answer is kept for the TTL"
        );
    }

    #[test]
    fn a_status_that_answers_with_no_names_keeps_the_names_it_gave() {
        let names = HostGate::new(&[
            Ok(STATUS),
            Ok("{}"),
            Ok("not json"),
            Err("tailscaled is down"),
        ]);
        assert!(names.allows(HOST));
        assert!(names.after(TTL).allows(HOST), "an answer with no names");
        assert!(names.after(FAILED_TTL).allows(HOST), "garbage");
        assert!(names.after(FAILED_TTL).allows(HOST), "a daemon down");
        assert_eq!(names.unasked(), 0);
    }

    #[test]
    fn a_status_that_answers_without_the_name_refuses_it() {
        let names = HostGate::new(&[Ok(STATUS), Ok(OTHER)]);
        assert!(names.allows(HOST));
        assert!(!names.allows("other.taile6311e.ts.net"));
        assert!(!names.allows("100.83.24.16"), "another tailnet address");
        names.after(TTL);
        assert!(!names.allows(HOST), "the answer that no longer lists it");
        assert!(!names.allows("100.83.24.15"));
        assert!(names.allows("other.taile6311e.ts.net:8765"));
        assert_eq!(names.unasked(), 0);
    }

    #[test]
    fn with_no_answer_yet_only_loopback_is_allowed_and_status_asked_again_in_seconds() {
        let names = HostGate::new(&[Err(TIMED_OUT), Ok(STATUS)]);
        assert!(!names.allows(HOST));
        assert!(
            !names
                .after(FAILED_TTL - Duration::from_nanos(1))
                .allows(HOST)
        );
        for host in ["localhost:8765", "127.0.0.1", "[::1]:8765", "LOCALHOST."] {
            assert!(names.allows(host), "{host}");
        }
        assert_eq!(names.unasked(), 1, "asked once within FAILED_TTL");
        assert!(names.after(Duration::from_nanos(1)).allows(HOST));
        assert_eq!(names.unasked(), 0);
    }

    #[test]
    fn a_malformed_or_loopback_host_is_judged_without_asking_status() {
        let names = HostGate::new(&[]);
        for host in [
            "",
            "mac.taile6311e.ts.net:bad",
            "[::1",
            "[::1]x",
            "localhost:65536",
        ] {
            assert!(!names.allows(host), "{host:?}");
        }
        assert!(names.allows("localhost"));
    }

    // Preserve whois stdout on success; distinguish unknown peers from lookup failures.
    #[test]
    fn native_whois_returns_stdout_or_the_lookup_result() {
        use super::access_fixture::{Fixture, child_case};

        const ADDRESS: &str = "100.84.246.59";
        const STDOUT: &str = " \n{\"Node\": {\"Name\": \"phone.ts.net.\"}}\n\n";
        if let Some(case) = child_case() {
            let expected = match case.as_str() {
                "stdout" => Ok(Some(STDOUT.to_string())),
                "lossy-utf8" => Ok(Some("hi\u{fffd}\n".to_string())),
                "unknown" => Ok(None),
                "daemon-error" => Err("tailscale whois failed: tailscaled is down".to_string()),
                "peer-error" => Err("tailscale whois failed: peer unreachable".to_string()),
                "empty-error" => Err("tailscale whois failed: ".to_string()),
                other => panic!("unknown access fixture: {other}"),
            };
            // The stand-in answers within a backstop only a stall reaches, not the server's 3 s.
            let patience = test_time::backstop(CLI_TIMEOUT);
            assert_eq!(whois_by(find_tailscale(), ADDRESS, patience), expected);
            return;
        }

        for (case, stdout, stderr, status) in [
            (
                "stdout",
                STDOUT.as_bytes(),
                b"peer not found\n".as_slice(),
                0,
            ),
            ("lossy-utf8", b"hi\xff\n".as_slice(), b"".as_slice(), 0),
            (
                "unknown",
                PEER.as_bytes(),
                b"2026/10/03 peer not found\n".as_slice(),
                1,
            ),
            (
                "daemon-error",
                PEER.as_bytes(),
                b"  tailscaled is down\n".as_slice(),
                1,
            ),
            (
                "peer-error",
                b"".as_slice(),
                b"peer unreachable\n".as_slice(),
                1,
            ),
            ("empty-error", b"".as_slice(), b"".as_slice(), 1),
        ] {
            let fixture = Fixture::new(stdout, stderr, status);
            fixture.run(
                "gate::tests::native_whois_returns_stdout_or_the_lookup_result",
                case,
            );
            fixture.assert_args(&["whois", "--json", ADDRESS]);
        }
    }

    #[test]
    fn native_status_returns_stdout_or_an_error() {
        use super::access_fixture::{Fixture, child_case};

        if let Some(case) = child_case() {
            let expected = match case.as_str() {
                "stdout" => Ok(format!("{STATUS}\n")),
                "lossy-utf8" => Ok("{\u{fffd}}".to_string()),
                "daemon-error" => Err("tailscale status failed: tailscaled is down".to_string()),
                other => panic!("unknown access fixture: {other}"),
            };
            assert_eq!(tailscale_status(), expected);
            return;
        }

        for (case, stdout, stderr, status) in [
            (
                "stdout",
                format!("{STATUS}\n").into_bytes(),
                b"".as_slice(),
                0,
            ),
            ("lossy-utf8", b"{\xff}".to_vec(), b"".as_slice(), 0),
            (
                "daemon-error",
                STATUS.as_bytes().to_vec(),
                b"  tailscaled is down\n".as_slice(),
                1,
            ),
        ] {
            let fixture = Fixture::new(&stdout, stderr, status);
            fixture.run(
                "gate::tests::native_status_returns_stdout_or_an_error",
                case,
            );
            fixture.assert_args(&["status", "--json"]);
        }
    }
}
