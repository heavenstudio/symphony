//! Who a request is from, as the gate reads Tailscale's answers ([`Gate::visitor`]), and which
//! boards have added them ([`Gate::member`]): a whois answer's node and login, this machine's own
//! login from its status, and what each kind of peer is, with fakes of both calls in place of a
//! tailnet.

use super::*;
use std::collections::VecDeque;
use symphony_plugin::Sender;
use symphony_plugin::execution::{Principal, PrincipalKind};

/// The user this machine is under in [`STATUS`], with sixteen digits, as Tailscale's ids have: a
/// double would round it.
const OWN: u64 = 6_123_456_789_012_345;
/// Another user of the tailnet.
const OTHER: u64 = 6_123_456_789_012_346;

const STATUS: &str = r#"{"Self": {"DNSName": "studio.tail.ts.net.", "UserID": 6123456789012345, "TailscaleIPs": ["100.83.24.15"]}, "User": {"6123456789012345": {"ID": 6123456789012345, "LoginName": "owner@example.com"}}}"#;
/// [`STATUS`] of a machine that is itself tagged: under no login.
const TAGGED_STATUS: &str = r#"{"Self": {"DNSName": "studio.tail.ts.net.", "UserID": 6123456789012345, "Tags": ["tag:server"], "TailscaleIPs": ["100.83.24.15"]}}"#;
/// [`STATUS`] that names no user.
const USERLESS_STATUS: &str =
    r#"{"Self": {"DNSName": "studio.tail.ts.net.", "TailscaleIPs": ["100.83.24.15"]}}"#;

/// The owner's phone, under this machine's login.
const PHONE: &str = r#"{"Node": {"Name": "phone.tail.ts.net.", "User": 6123456789012345}, "UserProfile": {"ID": 6123456789012345, "LoginName": "owner@example.com", "DisplayName": "Owner"}}"#;
/// A colleague's laptop, under another login.
const LAPTOP: &str = r#"{"Node": {"Name": "laptop.tail.ts.net.", "User": 6123456789012346}, "UserProfile": {"ID": 6123456789012346, "LoginName": "ana@example.com", "DisplayName": "Ana Lima"}}"#;
/// A tagged node: Tailscale names the pseudo-user of tagged devices.
const SERVER: &str = r#"{"Node": {"Name": "ci.tail.ts.net.", "User": 99, "Tags": ["tag:ci"]}, "UserProfile": {"ID": 99, "LoginName": "tagged-devices", "DisplayName": "Tagged Devices"}}"#;
/// A node whose answer names no user.
const NAMELESS: &str = r#"{"Node": {"Name": "old.tail.ts.net."}}"#;

const PHONE_IP: &str = "100.101.102.103";
const LAPTOP_IP: &str = "100.101.102.104";
const SERVER_IP: &str = "100.101.102.105";
const NAMELESS_IP: &str = "100.101.102.106";
/// This machine's own tailnet address, in [`STATUS`].
const OWN_IP: &str = "100.83.24.15";
const TIMED_OUT: &str = "no answer within 3 s";

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

fn login(id: u64, name: &str, display: &str) -> Login {
    Login {
        id,
        name: name.into(),
        display: display.into(),
    }
}

/// A gate whose whois knows the four nodes above by address, whose status gives `statuses` in
/// turn, one per call, failing the test when asked once more; with the clock `now`.
struct Judge {
    gate: Gate,
    statuses: Arc<Mutex<VecDeque<Result<String, String>>>>,
    now: Arc<Mutex<Instant>>,
}

impl Judge {
    fn new(statuses: &[Result<&str, &str>]) -> Judge {
        let statuses = Arc::new(Mutex::new(
            statuses
                .iter()
                .map(|answer| answer.map(str::to_string).map_err(str::to_string))
                .collect::<VecDeque<_>>(),
        ));
        let now = Arc::new(Mutex::new(Instant::now()));
        let (queue, clock) = (statuses.clone(), now.clone());
        let gate = Gate::new(
            Arc::new(|address: &str| {
                Ok(match address {
                    PHONE_IP => Some(PHONE.into()),
                    LAPTOP_IP => Some(LAPTOP.into()),
                    SERVER_IP => Some(SERVER.into()),
                    NAMELESS_IP => Some(NAMELESS.into()),
                    // Asked of its own address, Tailscale names this machine's own node.
                    OWN_IP => Some(SERVER.into()),
                    _ => None,
                })
            }),
            Arc::new(move || *clock.lock().unwrap()),
        )
        .with_status(Arc::new(move || {
            queue
                .lock()
                .unwrap()
                .pop_front()
                .expect("status asked more often than the case allows")
        }));
        Judge {
            gate,
            statuses,
            now,
        }
    }

    fn after(&self, wait: Duration) -> &Judge {
        *self.now.lock().unwrap() += wait;
        self
    }

    fn visitor(&self, address: &str) -> Option<Visitor> {
        self.gate.visitor(ip(address))
    }

    fn unasked(&self) -> usize {
        self.statuses.lock().unwrap().len()
    }
}

#[test]
fn a_whois_answer_names_its_node_and_the_login_it_is_under() {
    assert_eq!(
        peer_in(PHONE),
        Some(Peer {
            node: "phone.tail.ts.net".into(),
            login: Some(login(OWN, "owner@example.com", "Owner")),
        })
    );
    assert_eq!(
        peer_in(LAPTOP).and_then(|peer| peer.login),
        Some(login(OTHER, "ana@example.com", "Ana Lima"))
    );
    let undisplayed = r#"{"Node": {}, "UserProfile": {"ID": 7, "LoginName": "bo@example.com", "DisplayName": ""}}"#;
    assert_eq!(
        peer_in(undisplayed),
        Some(Peer {
            node: String::new(),
            login: Some(login(7, "bo@example.com", "bo@example.com")),
        }),
        "a login with no display name is shown by its name"
    );
}

#[test]
fn a_tagged_node_or_one_whose_answer_names_no_user_is_under_no_login() {
    for answer in [
        SERVER,
        NAMELESS,
        r#"{"Node": {"Name": "a"}, "UserProfile": {}}"#,
        r#"{"Node": {"Name": "a"}, "UserProfile": {"ID": 0, "LoginName": "zero@example.com"}}"#,
        r#"{"Node": {"Name": "a"}, "UserProfile": {"ID": 7}}"#,
        r#"{"Node": {"Name": "a"}, "UserProfile": {"ID": 7, "LoginName": ""}}"#,
        r#"{"Node": {"Name": "a"}, "UserProfile": {"ID": "7", "LoginName": "s@example.com"}}"#,
        r#"{"Node": {"Name": "a"}, "UserProfile": {"ID": -7, "LoginName": "n@example.com"}}"#,
    ] {
        let peer = peer_in(answer).unwrap_or_else(|| panic!("a node: {answer}"));
        assert_eq!(peer.login, None, "{answer}");
    }
    // An empty tag list tags nothing.
    let untagged =
        r#"{"Node": {"Tags": []}, "UserProfile": {"ID": 7, "LoginName": "t@example.com"}}"#;
    assert!(peer_in(untagged).unwrap().login.is_some());
    for garbage in [
        "",
        "not json {",
        "[]",
        "{}",
        r#"{"Node": null}"#,
        r#"{"Node": 5}"#,
    ] {
        assert_eq!(peer_in(garbage), None, "{garbage:?}");
    }
}

#[test]
fn a_status_answer_names_the_user_this_machine_is_under_unless_it_is_tagged() {
    let own = own_in(STATUS).unwrap();
    assert_eq!(own.user, Some(OWN));
    assert!(own.names.contains("studio.tail.ts.net"));
    assert_eq!(
        own_in(TAGGED_STATUS).unwrap().user,
        None,
        "a tagged machine"
    );
    assert_eq!(own_in(USERLESS_STATUS).unwrap().user, None);
    let zero = r#"{"Self": {"DNSName": "studio.tail.ts.net.", "UserID": 0}}"#;
    assert_eq!(own_in(zero).unwrap().user, None);
    for unanswered in [
        "",
        "not json",
        "{}",
        r#"{"Self": {"UserID": 6123456789012345}}"#,
    ] {
        assert_eq!(
            own_in(unanswered),
            None,
            "no names, no answer: {unanswered:?}"
        );
    }
}

#[test]
fn each_kind_of_peer_is_the_visitor_its_login_makes_it() {
    let judge = Judge::new(&[Ok(STATUS)]);
    assert_eq!(
        judge.visitor(PHONE_IP),
        Some(Visitor::Machine),
        "the owner's phone"
    );
    assert_eq!(
        judge.visitor(LAPTOP_IP),
        Some(Visitor::Login(login(OTHER, "ana@example.com", "Ana Lima")))
    );
    assert_eq!(
        judge.visitor(SERVER_IP),
        Some(Visitor::Node("ci.tail.ts.net".into()))
    );
    assert_eq!(
        judge.visitor(NAMELESS_IP),
        Some(Visitor::Node("old.tail.ts.net".into()))
    );
    assert_eq!(
        judge.visitor(OWN_IP),
        Some(Visitor::Machine),
        "this machine's own address, though its node answers tagged"
    );
    assert_eq!(
        judge.visitor(&format!("::ffff:{LAPTOP_IP}")),
        judge.visitor(LAPTOP_IP)
    );
    for refused in ["100.64.0.9", "192.168.1.20", "8.8.8.8"] {
        assert_eq!(judge.visitor(refused), None, "{refused}");
    }
    assert_eq!(judge.unasked(), 0, "one status answer serves every look");
}

#[test]
fn loopback_is_the_machine_without_asking_tailscale() {
    let gate = Gate::new(
        Arc::new(|address: &str| panic!("whois asked of {address}")),
        Arc::new(Instant::now),
    )
    .with_status(Arc::new(|| panic!("status asked")));
    for address in ["127.0.0.1", "::1", "::ffff:127.0.0.1"] {
        assert_eq!(
            gate.visitor(ip(address)),
            Some(Visitor::Machine),
            "{address}"
        );
    }
    assert_eq!(gate.visitor(ip("192.168.1.20")), None, "asks nothing");
}

#[test]
fn until_a_status_names_the_machines_login_every_other_node_is_another_login() {
    let judge = Judge::new(&[Err(TIMED_OUT), Ok(STATUS)]);
    assert_eq!(
        judge.visitor(PHONE_IP),
        Some(Visitor::Login(login(OWN, "owner@example.com", "Owner"))),
        "with no status answer yet, the phone is a login the gate cannot call the machine's"
    );
    assert_eq!(
        judge.visitor(OWN_IP),
        Some(Visitor::Node("ci.tail.ts.net".into()))
    );
    assert_eq!(judge.unasked(), 1, "asked again only after FAILED_TTL");
    assert_eq!(
        judge.after(FAILED_TTL).visitor(PHONE_IP),
        Some(Visitor::Machine),
        "the answer that names it"
    );
}

#[test]
fn a_status_refresh_that_fails_keeps_the_machines_login_it_last_gave() {
    let judge = Judge::new(&[
        Ok(STATUS),
        Err(TIMED_OUT),
        Ok("not json"),
        Ok("{}"),
        Err("tailscaled is down"),
    ]);
    assert_eq!(judge.visitor(PHONE_IP), Some(Visitor::Machine));
    for why in ["a timeout", "garbage", "no names", "a daemon down"] {
        let wait = if why == "a timeout" { TTL } else { FAILED_TTL };
        assert_eq!(
            judge.after(wait).visitor(PHONE_IP),
            Some(Visitor::Machine),
            "{why}"
        );
    }
    assert_eq!(judge.unasked(), 0);
}

#[test]
fn a_status_that_names_no_login_makes_the_phone_another_login() {
    for (answer, why) in [
        (TAGGED_STATUS, "the machine tagged"),
        (USERLESS_STATUS, "no user"),
    ] {
        let judge = Judge::new(&[Ok(STATUS), Ok(answer)]);
        assert_eq!(judge.visitor(PHONE_IP), Some(Visitor::Machine));
        assert_eq!(
            judge.after(TTL).visitor(PHONE_IP),
            Some(Visitor::Login(login(OWN, "owner@example.com", "Owner"))),
            "{why}"
        );
        assert_eq!(
            judge.visitor(OWN_IP),
            Some(Visitor::Machine),
            "its own address stays the machine: {why}"
        );
    }
}

#[test]
fn the_machine_is_on_every_board_and_by_default_nobody_else_is_on_any() {
    let gate = Judge::new(&[Ok(STATUS)]).gate;
    let local = symphony_board::workflow::local_actor();
    for root in [Path::new("/alpha"), Path::new("/beta")] {
        assert_eq!(gate.member(root, &Visitor::Machine), Some(local.clone()));
        for visitor in [
            Visitor::Login(login(OTHER, "ana@example.com", "Ana")),
            Visitor::Node("ci".into()),
        ] {
            assert_eq!(gate.member(root, &visitor), None, "{visitor:?}");
        }
    }
    let ana = Actor::new(
        Sender::new("ana", "Ana Lima", vec![]),
        Principal::new(PrincipalKind::Human, "test-host".into(), "u-ana".into()),
    );
    assert_eq!(
        Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now))
            .admitting(ana.clone())
            .member(Path::new("/alpha"), &Visitor::Machine),
        Some(ana),
        "a test host's person at the machine"
    );
}

#[test]
fn a_roster_says_which_boards_a_visitor_is_on_and_is_never_asked_of_the_machine() {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let ana = Actor::new(
        Sender::new("ana", "Ana Lima", vec![]),
        Principal::new(
            PrincipalKind::Human,
            "tailscale".into(),
            "ana@example.com".into(),
        ),
    );
    let gate = Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)).with_roster({
        let (asked, ana) = (asked.clone(), ana.clone());
        Arc::new(move |root: &Path, visitor: &Visitor| {
            asked
                .lock()
                .unwrap()
                .push((root.to_path_buf(), visitor.clone()));
            let on = root == Path::new("/alpha")
                && matches!(visitor, Visitor::Login(login) if login.name == "ana@example.com");
            on.then(|| ana.clone())
        })
    });
    let laptop = Visitor::Login(login(OTHER, "ana@example.com", "Ana Lima"));
    assert_eq!(gate.member(Path::new("/alpha"), &laptop), Some(ana));
    assert_eq!(gate.member(Path::new("/beta"), &laptop), None);
    let bo = Visitor::Login(login(7, "bo@example.com", "Bo"));
    assert_eq!(gate.member(Path::new("/alpha"), &bo), None);
    assert!(
        gate.member(Path::new("/alpha"), &Visitor::Machine)
            .is_some()
    );
    assert_eq!(
        *asked.lock().unwrap(),
        [
            ("/alpha".into(), laptop.clone()),
            ("/beta".into(), laptop),
            ("/alpha".into(), bo),
        ],
        "asked with the board's root and the visitor; the machine is never asked of"
    );
}
