//! The People page's routes: the page as each reader is shown it, no sign-in's subject in any
//! answer, and its writes through the real router: a change by the person at the machine and by
//! a person holding administer, each a line of the board's log, a reader without it refused with
//! nothing written, and a profile written by whom `symphony_board::people::change` lets. And what
//! the power to direct agents changes on a task's page: a message held, and passed on.

use super::*;
use crate::testing::{Board, LAPTOP, Tree, laptop_with, repository_holding};
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, header};
use std::net::SocketAddr;
use symphony_api::{PEOPLE_PATH, PROFILE_PATH, TaskEntry};
use symphony_board::people::{Did, Group, Member, Profile};
use tower::ServiceExt;

const HOST: &str = "127.0.0.1:8123";
/// The person at the machine, from loopback.
const HERE: &str = "127.0.0.1:5000";

fn signin(text: &str) -> Signin {
    Signin::parse(text).unwrap()
}

/// Ana's profile: two Tailscale sign-ins, one GitHub, a git handle and an email.
fn anas() -> Profile {
    Profile {
        name: "ana".into(),
        display: "Ana Lima".into(),
        signins: vec![
            signin("tailscale:ana@example.com"),
            signin("github:ana-lima"),
            signin("tailscale:ana@work.example"),
        ],
        git: vec!["ana-lima".into()],
        emails: vec!["ana@mail.example".into()],
        ..Profile::default()
    }
}

/// A board holding Ana (speak, and decide by the group `staff`) and Bo, with no profile, holding
/// administer.
fn people() -> People {
    People {
        members: vec![
            Member {
                name: "ana".into(),
                groups: vec!["staff".into()],
                powers: vec![Power::Speak],
                revision: 3,
            },
            Member {
                name: "bo".into(),
                groups: vec![],
                powers: vec![Power::Administer],
                revision: 4,
            },
        ],
        groups: vec![Group {
            name: "staff".into(),
            powers: vec![Power::Decide],
            revision: 1,
        }],
        revision: 4,
    }
}

fn asker(name: &str, machine: bool) -> Asker {
    Asker {
        name: name.into(),
        machine,
    }
}

fn line(n: i64, did: Did, name: &str, powers: &[Power]) -> Change {
    Change {
        n,
        at: 1_700_000_000.0 + n as f64,
        by: "owner".into(),
        did,
        name: name.into(),
        group: None,
        powers: powers.to_vec(),
    }
}

#[test]
fn a_page_lists_each_person_by_their_profile_and_names_no_sign_ins_subject() {
    let profiles = Profiles {
        people: vec![anas()],
    };
    let log = vec![
        line(1, Did::GroupAdd, "staff", &[Power::Decide]),
        line(2, Did::Add, "ana", &[Power::Speak]),
    ];
    let page = page(Ok((people(), log)), Ok(profiles), &asker("owner", true));
    let ana = &page.people[0];
    assert_eq!(
        (ana.display.as_str(), ana.signins.as_slice()),
        (
            "Ana Lima",
            ["tailscale".to_string(), "github".to_string()].as_slice()
        ),
        "each service once"
    );
    assert_eq!(ana.powers, ["read", "speak", "decide"]);
    assert_eq!(
        (ana.own.as_slice(), ana.profile),
        (["speak".to_string()].as_slice(), true)
    );
    let bo = &page.people[1];
    assert_eq!(
        (
            bo.display.as_str(),
            bo.profile,
            bo.signins.len(),
            bo.git.len()
        ),
        ("bo", false, 0, 0)
    );
    assert_eq!(
        page.groups,
        [GroupRow {
            name: "staff".into(),
            powers: vec!["decide".into()]
        }]
    );
    assert_eq!(
        (
            page.log[1].did.as_str(),
            page.log[1].powers.as_slice(),
            page.revision
        ),
        ("add", ["speak".to_string()].as_slice(), 4)
    );
    let said = serde_json::to_string(&page).unwrap();
    for private in ["ana@example.com", "ana@work.example", "ana-lima\""] {
        let shown = said.matches(private).count();
        let allowed = usize::from(private == "ana-lima\"");
        assert_eq!(shown, allowed, "{private} in {said}");
    }
}

/// Ana reached by mail and Slack, her cards by Slack; Bo, the board's admin, by WhatsApp, his
/// verdicts that way.
fn reached() -> Profiles {
    let map = |value: serde_json::Value| value.as_object().unwrap().clone();
    Profiles {
        people: vec![
            Profile {
                chat: map(serde_json::json!({"slack": "U024BE7LH"})),
                preferences: map(serde_json::json!({"card": "slack"})),
                ..anas()
            },
            Profile {
                name: "bo".into(),
                chat: map(serde_json::json!({"whatsapp": "+15551234567"})),
                preferences: map(serde_json::json!({"verdict": "whatsapp"})),
                ..Profile::default()
            },
        ],
    }
}

#[test]
fn addresses_go_to_the_person_the_machine_and_admins_and_ways_to_the_person_and_the_machine() {
    let anas = Addresses {
        email: Some("ana@mail.example".into()),
        slack: Some("U024BE7LH".into()),
        whatsapp: None,
    };
    let bos = Addresses {
        whatsapp: Some("+15551234567".into()),
        ..Addresses::default()
    };
    let ways = |card: Option<&str>, verdict: Option<&str>| {
        let kind = |kind: &str, way: Option<&str>| NoticeWay {
            kind: kind.into(),
            way: way.map(Into::into),
        };
        Some(vec![
            kind("card", card),
            kind("question", None),
            kind("verdict", verdict),
        ])
    };
    let shown = |asker: Asker| {
        let page = page(Ok((people(), vec![])), Ok(reached()), &asker);
        let said = serde_json::to_string(&page).unwrap();
        let rows = (page.people.into_iter())
            .map(|row| (row.addresses, row.notices))
            .collect::<Vec<_>>();
        (rows, said)
    };
    let (machine, _) = shown(asker("owner", true));
    assert_eq!(
        machine,
        [
            (Some(anas.clone()), ways(Some("slack"), None)),
            (Some(bos.clone()), ways(None, Some("whatsapp")))
        ]
    );
    let (own, said) = shown(asker("ana", false));
    assert_eq!(
        own,
        [
            (Some(anas.clone()), ways(Some("slack"), None)),
            (None, None)
        ]
    );
    assert!(!said.contains("+15551234567"), "{said}");
    // Bo administers: he sees Ana's addresses and not her ways, and his own whole.
    let (admin, said) = shown(asker("bo", false));
    assert_eq!(
        admin,
        [
            (Some(anas), None),
            (Some(bos), ways(None, Some("whatsapp")))
        ]
    );
    assert_eq!(said.matches("\"notices\"").count(), 1, "{said}");
    // Anyone else sees neither, and the answer carries neither field.
    let (other, said) = shown(asker("cy", false));
    assert_eq!(other, [(None, None), (None, None)]);
    for private in [
        "addresses",
        "notices",
        "ana@mail.example",
        "U024BE7LH",
        "slack",
    ] {
        assert!(!said.contains(private), "{private} in {said}");
    }
    // A person the machine holds no profile of is shown with none set, to whom may see them.
    let (_, said) = shown(asker("owner", true));
    assert!(said.contains("\"addresses\""), "{said}");
    let page = page(
        Ok((people(), vec![])),
        Ok(Profiles::default()),
        &asker("bo", false),
    );
    assert_eq!(
        (
            page.people[1].addresses.clone(),
            page.people[1].notices.clone()
        ),
        (Some(Addresses::default()), ways(None, None))
    );
}

#[test]
fn the_page_says_who_reads_it_and_whether_they_administer() {
    let shown = |asker: Asker| {
        let page = page(Ok((people(), vec![])), Ok(Profiles::default()), &asker);
        (page.administers, page.machine, page.you)
    };
    assert_eq!(shown(asker("owner", true)), (true, true, None));
    assert_eq!(shown(asker("bo", false)), (true, false, Some("bo".into())));
    assert_eq!(
        shown(asker("ana", false)),
        (false, false, Some("ana".into()))
    );
}

#[test]
fn a_board_or_profiles_not_read_are_said_and_the_page_lists_what_was() {
    let page = page(
        Err("down".into()),
        Err(NO_MACHINE.into()),
        &asker("owner", true),
    );
    assert_eq!(
        page.problem.as_deref(),
        Some(
            "the board's people could not be read: down; the machine's profiles could not be \
             read: this server names no machine folder, so no profile is read or written"
        )
    );
    assert!(page.people.is_empty() && page.log.is_empty());
    // A reader who is not the person at the machine is told what was not read, never why, as a
    // cause can name a local path.
    let read = super::page(
        Err("the database at /private/board/.s.PGSQL is down".into()),
        Err("/Users/someone/machine/people.json: unreadable".into()),
        &asker("ana", false),
    );
    assert_eq!(
        read.problem.as_deref(),
        Some("the board's people could not be read; the machine's profiles could not be read")
    );
    let page = page_of_profiles_unread();
    assert_eq!(
        page.people[0].display, "ana",
        "a person listed without a profile"
    );
}

fn page_of_profiles_unread() -> PeoplePage {
    page(
        Ok((people(), vec![])),
        Err("people.json: unreadable".into()),
        &asker("ana", false),
    )
}

#[test]
fn the_people_are_read_by_get_and_changed_by_two_posts_the_one_needing_administer() {
    assert!(crate::api::READS.contains(&PEOPLE_PATH));
    for path in [PEOPLE_PATH, "/people"] {
        assert_eq!(crate::write::methods(path), None, "{path}");
    }
    for path in [PEOPLE_CHANGE_PATH, PROFILE_PATH] {
        assert_eq!(crate::write::methods(path), Some("POST"), "{path}");
        assert!(!crate::api::READS.contains(&path));
    }
    use crate::powers::{Needs, needs};
    assert_eq!(
        needs(PEOPLE_CHANGE_PATH),
        Needs::Powers(&[Power::Administer])
    );
    assert_eq!(needs(PROFILE_PATH), Needs::Powers(&[]));
}

/// A project with a board, its People page served behind `gate` over the machine folder
/// `machine`: the server, its root, and the tree and board kept alive.
struct Served {
    shared: Arc<Shared>,
    root: PathBuf,
    machine: PathBuf,
    _board: Board,
    _tree: Tree,
}

impl Served {
    /// The board holding `people`, Ana's profile signing her in as the laptop's login.
    fn new(people: &[(&str, &[Power])]) -> Served {
        let tree = Tree::new();
        let root = repository_holding(&tree, "main", "docs/README.md", "# Docs\n");
        let machine = tree.root.join("machine");
        (Profiles {
            people: vec![anas()],
        })
        .write(&machine)
        .unwrap();
        let board = Board::holding(&root, people);
        let gate = laptop_with(Arc::new(|_, _| None)).with_machine(machine.clone());
        let shared = Shared::new(1, 8123, &root, None, gate);
        let hub = crate::live::Hub::new(&root, &root.join("build/client"));
        assert!(shared.api.set(Arc::new(hub)).is_ok());
        Served {
            shared,
            root,
            machine,
            _board: board,
            _tree: tree,
        }
    }

    /// [`Served::new`], whose gate signs Ana in from the laptop by her profile.
    fn signing_in(people: &[(&str, &[Power])]) -> Served {
        let served = Served::new(people);
        let gate = laptop_with(crate::roster::of(served.machine.clone()))
            .with_machine(served.machine.clone());
        let shared = Shared::new(1, 8123, &served.root, None, gate);
        assert!(shared.api.set(served.shared.hub().ok().unwrap()).is_ok());
        Served { shared, ..served }
    }

    /// What the router answers `client`'s `method` of `path` with `body`: status and body.
    async fn asked(
        &self,
        client: &str,
        method: &str,
        path: &str,
        body: &str,
    ) -> (StatusCode, String) {
        let app = crate::front::router(self.shared.clone())
            .layer(MockConnectInfo(client.parse::<SocketAddr>().unwrap()));
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST);
        if method == "POST" {
            request = request
                .header(header::ORIGIN, format!("http://{HOST}"))
                .header(header::CONTENT_TYPE, "application/json");
        }
        let response = app
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    /// `client`'s change `change`, its page as it then reads.
    async fn changed(&self, client: &str, change: serde_json::Value) -> PeoplePage {
        let (status, body) = self
            .asked(client, "POST", PEOPLE_CHANGE_PATH, &change.to_string())
            .await;
        assert_eq!(status, StatusCode::OK, "{change}: {body}");
        serde_json::from_str(&body).unwrap()
    }

    async fn page(&self, client: &str) -> PeoplePage {
        let (status, body) = self.asked(client, "GET", PEOPLE_PATH, "").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        serde_json::from_str(&body).unwrap()
    }

    fn profiles(&self) -> Profiles {
        Profiles::read(&self.machine).unwrap()
    }

    /// The board's log as `(did, name, by)`.
    fn log(&self) -> Vec<(String, String, String)> {
        (Board::store(&self.root).people_log().unwrap().into_iter())
            .map(|line| (line.did.word().to_string(), line.name, line.by))
            .collect()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_person_at_the_machine_adds_a_person_puts_them_in_a_group_and_gives_a_power() {
    let served = Served::new(&[]);
    let empty = served.page(HERE).await;
    assert_eq!(
        (
            empty.people.len(),
            empty.administers,
            empty.machine,
            empty.you
        ),
        (0, true, true, None)
    );
    let page = served
        .changed(
            HERE,
            serde_json::json!({
                "action": "add", "name": "bo", "powers": ["speak"], "display": "Bo Reis",
                "signins": ["tailscale:bo@example.com"], "git": ["bo-reis"],
            }),
        )
        .await;
    assert_eq!(page.people[0].display, "Bo Reis");
    served
        .changed(
            HERE,
            serde_json::json!({"action": "group-add", "name": "staff", "powers": ["decide"]}),
        )
        .await;
    served
        .changed(
            HERE,
            serde_json::json!({"action": "join", "name": "bo", "group": "staff"}),
        )
        .await;
    let page = served
        .changed(
            HERE,
            serde_json::json!({
                "action": "grant", "name": "bo", "powers": ["administer", "direct_agents"]
            }),
        )
        .await;
    let bo = &page.people[0];
    assert_eq!(
        bo.powers,
        ["read", "speak", "decide", "direct_agents", "administer"]
    );
    assert_eq!(
        (bo.groups.as_slice(), bo.signins.as_slice()),
        (
            ["staff".to_string()].as_slice(),
            ["tailscale".to_string()].as_slice()
        )
    );
    assert_eq!(
        served.log(),
        [
            ("add".into(), "bo".into(), "owner".into()),
            ("group-add".into(), "staff".into(), "owner".into()),
            ("join".into(), "bo".into(), "owner".into()),
            ("grant".into(), "bo".into(), "owner".into()),
        ]
    );
    assert_eq!(page.log.len(), 4);
    let bo = served.profiles().get("bo").cloned().unwrap();
    assert_eq!(bo.git, ["bo-reis"]);
    // The rest of a person's and a group's changes, each a line.
    for change in [
        serde_json::json!({"action": "revoke", "name": "bo", "powers": ["administer", "direct_agents"]}),
        serde_json::json!({"action": "grant", "group": "staff", "powers": ["speak"]}),
        serde_json::json!({"action": "revoke", "group": "staff", "powers": ["speak"]}),
        serde_json::json!({"action": "leave", "name": "bo", "group": "staff"}),
        serde_json::json!({"action": "group-remove", "name": "staff"}),
        serde_json::json!({"action": "remove", "name": "bo"}),
    ] {
        served.changed(HERE, change).await;
    }
    let page = served.page(HERE).await;
    assert!(page.people.is_empty() && page.groups.is_empty());
    assert_eq!(page.log.len(), 10);
    assert_eq!(
        served.profiles().get("bo").map(|p| p.name.as_str()),
        Some("bo"),
        "her profile is the machine's"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_holding_administer_changes_the_board_and_one_without_it_is_refused() {
    let served = Served::signing_in(&[("ana", &[Power::Speak, Power::Administer])]);
    let page = served.page(LAPTOP).await;
    assert_eq!(
        (page.administers, page.machine, page.you.as_deref()),
        (true, false, Some("ana"))
    );
    served
        .changed(
            LAPTOP,
            serde_json::json!({"action": "add", "name": "bo", "powers": ["speak"], "display": "Bo"}),
        )
        .await;
    assert_eq!(served.log()[1], ("add".into(), "bo".into(), "ana".into()));
    assert_eq!(
        served.profiles().get("bo").unwrap().display,
        "Bo",
        "a profile made for one with none"
    );
    // A sign-in is the person at the machine's alone to give, to a name the machine holds a
    // profile of or a new one, and her refusal names no one else's sign-in or profile.
    Board::store(&served.root)
        .remove_person("bo", "owner")
        .unwrap();
    for (name, subject) in [("bo", "mallory@example.com"), ("cy", "ana@laptop.example")] {
        let (status, body) = served
            .asked(
                LAPTOP,
                "POST",
                PEOPLE_CHANGE_PATH,
                &serde_json::json!({"action": "add", "name": name, "signins": [format!("tailscale:{subject}")]})
                    .to_string(),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(
            body.contains("a sign-in is the person at the machine's to give")
                && !body.contains("ana's"),
            "{body}"
        );
    }
    assert_eq!(served.profiles().get("bo").unwrap().signins.len(), 0);
    assert!(served.profiles().get("cy").is_none());
    // A failure is told her without its cause, which names the machine folder; the person at the
    // machine is told it whole.
    let lock = served.machine.join(symphony_board::people::LOCK);
    let _ = std::fs::remove_file(&lock);
    std::fs::create_dir(&lock).unwrap();
    let add = r#"{"action":"add","name":"dora"}"#;
    let (status, body) = served.asked(LAPTOP, "POST", PEOPLE_CHANGE_PATH, add).await;
    assert_eq!(
        (status, body.as_str()),
        (
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"error":"the machine's profiles could not be locked or read, so nothing was written"}"#
        )
    );
    let (status, body) = served.asked(HERE, "POST", PEOPLE_CHANGE_PATH, add).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(body.contains(symphony_board::people::LOCK), "{body}");
    std::fs::remove_dir(&lock).unwrap();
    // Administer taken, her next change is refused before its body is read, and nothing is
    // written; she still reads the page, without its controls.
    Board::store(&served.root)
        .revoke(Holder::Person("ana"), &[Power::Administer], "owner")
        .unwrap();
    let before = served.log().len();
    let (status, body) = served
        .asked(
            LAPTOP,
            "POST",
            PEOPLE_CHANGE_PATH,
            r#"{"action":"add","name":"cy"}"#,
        )
        .await;
    assert_eq!(
        (status, body.as_str()),
        (
            StatusCode::FORBIDDEN,
            r#"{"error":"Forbidden: ana may not administer on this board"}"#
        )
    );
    assert_eq!(served.log().len(), before);
    let page = served.page(LAPTOP).await;
    assert_eq!((page.administers, page.people.len()), (false, 1));
    // Her own profile is hers to write, its sign-ins never; another's is not hers at all.
    let (status, body) = served
        .asked(
            LAPTOP,
            "POST",
            PROFILE_PATH,
            r#"{"name":"ana","display":"Ana L.","git":["ana-l"]}"#,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let ana = served.profiles().get("ana").cloned().unwrap();
    assert_eq!((ana.display.as_str(), ana.git.len()), ("Ana L.", 2));
    for (body, refused) in [
        (
            r#"{"name":"ana","signins":["github:ana"]}"#,
            "a sign-in is the person at the machine's to give",
        ),
        (
            r#"{"name":"bo","display":"Mallory"}"#,
            "only the person at the machine, or bo,",
        ),
    ] {
        let (status, said) = served.asked(LAPTOP, "POST", PROFILE_PATH, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{said}");
        assert!(said.contains(refused), "{said}");
    }
    assert_eq!(served.profiles().get("bo").unwrap().display, "Bo");
}

/// What `client` reads of `slug`'s task page: whether they may pass a held message on, and the
/// thread's entries.
async fn task_read(served: &Served, client: &str, slug: &str) -> (bool, Vec<TaskEntry>) {
    let (status, page) =
        (served.asked(client, "GET", &format!("/api/task-page?slug={slug}"), "")).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let page: symphony_api::TaskPage = serde_json::from_str(&page).unwrap();
    let path = format!("/api/task-history?slug={slug}");
    let (status, history) = served.asked(client, "GET", &path, "").await;
    assert_eq!(status, StatusCode::OK, "{history}");
    let history: symphony_api::TaskHistory = serde_json::from_str(&history).unwrap();
    (page.passes, history.entries)
}

/// A task's message from a person who may speak and may not direct agents is held: its board line
/// carries her words to a person's read, nothing of them to the thread's own text, and its pass is
/// refused her with nothing written; given direct agents, she passes it on under both names, and
/// writes a message of her own as the person at the machine does.
#[tokio::test(flavor = "multi_thread")]
async fn a_message_of_one_who_may_not_direct_agents_is_held_and_one_who_may_passes_it_on() {
    let served = Served::signing_in(&[("ana", &[Power::Speak])]);
    let store = Board::store(&served.root);
    store
        .create(&symphony_board::NewTask::new("tools/x", "X", "owner"))
        .unwrap();
    let hub = served.shared.api.get().unwrap();
    hub.refresh_page("", &served.root).unwrap();
    let words = "the phone picture cuts the header off";
    let message = serde_json::json!({"slug": "tools/x", "text": words}).to_string();
    let (status, body) = (served.asked(LAPTOP, "POST", "/api/task-message", &message)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let answer: serde_json::Value = serde_json::from_str(&body).unwrap();
    let id = answer["held"].as_i64().expect("held");
    let (passes, entries) = task_read(&served, LAPTOP, "tools/x").await;
    assert!(!passes);
    let marker = entries.last().unwrap();
    assert_eq!(marker.message.id, id);
    assert!(!marker.message.text.contains(words), "{marker:?}");
    let held = marker.held.as_ref().unwrap();
    assert_eq!(
        (
            held.id,
            held.author.as_str(),
            held.text.as_str(),
            held.passed_by.as_deref()
        ),
        (id, "ana", words, None)
    );
    assert!(
        task_read(&served, HERE, "tools/x").await.0,
        "the machine's person passes"
    );
    let pass = serde_json::json!({"slug": "tools/x", "id": id}).to_string();
    let (status, body) = (served.asked(LAPTOP, "POST", symphony_api::TASK_PASS_ROUTE, &pass)).await;
    assert_eq!(
        (status, body.as_str()),
        (
            StatusCode::FORBIDDEN,
            r#"{"error":"Forbidden: ana may not direct agents on this board"}"#
        )
    );
    assert_eq!(store.thread_rows("tools/x").unwrap().last().unwrap().id, id);
    store
        .grant(Holder::Person("ana"), &[Power::DirectAgents], "owner")
        .unwrap();
    let (passes, _) = task_read(&served, LAPTOP, "tools/x").await;
    assert!(passes);
    let (status, body) = (served.asked(LAPTOP, "POST", symphony_api::TASK_PASS_ROUTE, &pass)).await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, r#"{"ok":true}"#));
    let (_, entries) = task_read(&served, LAPTOP, "tools/x").await;
    let passed = entries.last().unwrap();
    assert_eq!(
        (passed.message.sender.as_str(), passed.message.person),
        ("ana", true)
    );
    assert_eq!(passed.message.text, format!("[passed on by ana] {words}"));
    for entry in [&entries[entries.len() - 2], passed] {
        let held = entry.held.as_ref().unwrap();
        assert_eq!((held.id, held.passed_by.as_deref()), (id, Some("ana")));
    }
    // Holding the power, her message is said as the person at the machine's is.
    let message = serde_json::json!({"slug": "tools/x", "text": "and the footer"}).to_string();
    let (status, body) = (served.asked(LAPTOP, "POST", "/api/task-message", &message)).await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, r#"{"ok":true}"#));
    let line = store.thread_rows("tools/x").unwrap().pop().unwrap();
    assert_eq!(
        (line.sender.as_str(), line.text.as_str()),
        ("ana", "and the footer")
    );
    assert_eq!(store.held_count("tools/x").unwrap(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_person_at_the_machine_writes_any_profile_and_a_refusal_writes_nothing() {
    let served = Served::new(&[("ana", &[Power::Speak])]);
    let (status, body) = served
        .asked(
            HERE,
            "POST",
            PROFILE_PATH,
            r#"{"name":"ana","signins":["github:ana-l"]}"#,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let page: PeoplePage = serde_json::from_str(&body).unwrap();
    assert_eq!(page.people[0].signins, ["tailscale", "github"]);
    for (route, body, refused) in [
        (
            PEOPLE_CHANGE_PATH,
            r#"{"action":"grant","name":"ana","powers":["fly"]}"#,
            "\\\"fly\\\" is no power",
        ),
        (
            PEOPLE_CHANGE_PATH,
            r#"{"action":"join","name":"ana"}"#,
            "a join or a leave names its group",
        ),
        (
            PEOPLE_CHANGE_PATH,
            r#"{"action":"add","name":"bo","signins":["github"]}"#,
            "a sign-in is SERVICE:SUBJECT",
        ),
        (
            PEOPLE_CHANGE_PATH,
            r#"{"action":"add","name":"ana"}"#,
            "ana",
        ),
        (
            PEOPLE_CHANGE_PATH,
            r#"{"action":"fly","name":"ana"}"#,
            "unknown variant",
        ),
        (
            PROFILE_PATH,
            r#"{"name":"bo","signins":["tailscale:ana@example.com"]}"#,
            "is ana's",
        ),
    ] {
        let before = served.log().len();
        let (status, said) = served.asked(HERE, "POST", route, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {said}");
        assert!(said.contains(refused), "{body}: {said}");
        assert_eq!(served.log().len(), before, "{body}");
    }
    assert_eq!(served.profiles().people.len(), 1);
    // A worktree's page changes no one.
    let (status, said) = served
        .asked(
            HERE,
            "POST",
            "/@feature/api/people-change",
            r#"{"action":"remove","name":"ana"}"#,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{said}");
    assert!(said.contains("never a worktree's"), "{said}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_with_no_machine_folder_changes_the_board_and_writes_no_profile() {
    let served = Served::new(&[("ana", &[Power::Speak])]);
    let shared = Shared::new(1, 8123, &served.root, None, crate::testing::open_gate());
    assert!(shared.api.set(served.shared.hub().ok().unwrap()).is_ok());
    let served = Served { shared, ..served };
    let page = served.page(HERE).await;
    assert_eq!(
        page.problem.as_deref(),
        Some(
            "the machine's profiles could not be read: this server names no machine folder, so no \
         profile is read or written"
        )
    );
    for (route, body) in [
        (PEOPLE_CHANGE_PATH, r#"{"action":"add","name":"bo"}"#),
        (PROFILE_PATH, r#"{"name":"ana","display":"Ana"}"#),
    ] {
        let (status, said) = served.asked(HERE, "POST", route, body).await;
        assert_eq!(
            (status, said.contains(NO_MACHINE)),
            (StatusCode::SERVICE_UNAVAILABLE, true),
            "{said}"
        );
    }
    let page = served
        .changed(
            HERE,
            serde_json::json!({"action": "grant", "name": "ana", "powers": ["decide"]}),
        )
        .await;
    assert_eq!(page.people[0].powers, ["read", "speak", "decide"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hosted_visitor_reads_no_one_and_changes_no_one() {
    let served = Served::new(&[("ana", &[Power::Speak])]);
    for (method, path, body) in [
        ("GET", PEOPLE_PATH, ""),
        (
            "POST",
            PEOPLE_CHANGE_PATH,
            r#"{"action":"remove","name":"ana"}"#,
        ),
        (
            "POST",
            PROFILE_PATH,
            r#"{"name":"ana","display":"Mallory"}"#,
        ),
    ] {
        let app = crate::front::router(served.shared.clone())
            .layer(MockConnectInfo(HERE.parse::<SocketAddr>().unwrap()));
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, format!("http://{HOST}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap();
        request
            .extensions_mut()
            .insert(crate::hosted::tests::admitted().await);
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {path}");
        let said = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&said).contains("ana"),
            "{method} {path}"
        );
    }
    assert_eq!(served.log().len(), 1, "ana is still on the board");
    assert_eq!(served.profiles().people, [anas()], "her profile as it was");
}

/// A change the board refuses is a 400 in the board's words, and profiles that cannot be had (a
/// folder where their lock goes) are a 503 to an add and to a profile's write: nothing is
/// written, on the board or in the machine folder.
#[tokio::test(flavor = "multi_thread")]
async fn a_change_the_board_refuses_or_profiles_not_had_are_said_with_nothing_written() {
    let served = Served::new(&[("ana", &[Power::Speak])]);
    let (status, said) = served
        .asked(
            HERE,
            "POST",
            PEOPLE_CHANGE_PATH,
            r#"{"action":"grant","name":"bo","powers":["direct_agents"]}"#,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{said}");
    assert!(said.contains("no person bo is on this board"), "{said}");
    let lock = served.machine.join(symphony_board::people::LOCK);
    let _ = std::fs::remove_file(&lock);
    std::fs::create_dir(&lock).unwrap();
    for (route, body, words) in [
        (
            PEOPLE_CHANGE_PATH,
            r#"{"action":"add","name":"bo"}"#,
            "the machine's profiles could not be locked or read",
        ),
        (
            PROFILE_PATH,
            r#"{"name":"ana","display":"Ana"}"#,
            "ana's profile could not be written",
        ),
    ] {
        let (status, said) = served.asked(HERE, "POST", route, body).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{route}: {said}");
        assert!(said.contains(words), "{route}: {said}");
    }
    assert_eq!(served.log().len(), 1, "ana's add alone");
    assert_eq!(served.profiles().people, [anas()]);
}

/// Profiles that do not parse are said to the person at the machine whole, which is what the
/// server's log keeps of them for anyone else ([`told`]): their file, line and column, and never a
/// value in them, a sign-in's subject among them.
#[tokio::test(flavor = "multi_thread")]
async fn profiles_that_do_not_parse_are_said_by_where_and_never_by_a_value_in_them() {
    let served = Served::new(&[("ana", &[Power::Speak])]);
    std::fs::write(
        served.machine.join(symphony_board::people::FILE),
        r#"{"people": [{"name": "ana", "signins": "ana@example.com"}]}"#,
    )
    .unwrap();
    let page = served.page(HERE).await;
    let problem = page.problem.unwrap_or_default();
    assert!(
        problem.starts_with("the machine's profiles could not be read: ")
            && problem.contains(
                "people.json: line 1, column 56: people[0].signins: a value of a type or form"
            ),
        "{problem}"
    );
    assert!(!problem.contains("ana@example.com"), "{problem}");
    assert_eq!(page.people[0].display, "ana", "listed without the profile");
}

/// A server whose read hub has not started answers the page 503, as every read of the board, and
/// one whose project holds no board it can reach answers a change 503, naming the board.
#[tokio::test(flavor = "multi_thread")]
async fn the_page_waits_on_the_servers_start_and_a_change_on_a_board_it_reaches() {
    let tree = Tree::new();
    let plain = tree.root.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let shared = Shared::new(1, 8123, &plain, None, crate::testing::open_gate());
    let app = |shared: &Arc<Shared>| {
        crate::front::router(shared.clone())
            .layer(MockConnectInfo(HERE.parse::<SocketAddr>().unwrap()))
    };
    let asked = |method: &str, path: &str, body: &str| {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST);
        if method == "POST" {
            request = request
                .header(header::ORIGIN, format!("http://{HOST}"))
                .header(header::CONTENT_TYPE, "application/json");
        }
        request.body(Body::from(body.to_string())).unwrap()
    };
    let said = |response: axum::response::Response| async move {
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&body).into_owned())
    };
    let (status, body) = said(
        app(&shared)
            .oneshot(asked("GET", PEOPLE_PATH, ""))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(body.contains("the Symphony server is starting"), "{body}");
    let (status, body) = said(
        app(&shared)
            .oneshot(asked(
                "POST",
                PEOPLE_CHANGE_PATH,
                r#"{"action":"remove","name":"ana"}"#,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(body.contains("the task board cannot be reached"), "{body}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_writes_their_own_addresses_and_ways_and_no_one_elses() {
    let served = Served::new(&[("ana", &[Power::Speak]), ("bo", &[Power::Speak])]);
    let shared = Shared::new(
        1,
        8123,
        &served.root,
        None,
        laptop_with(crate::roster::of(served.machine.clone())).with_machine(served.machine.clone()),
    );
    assert!(shared.api.set(served.shared.hub().ok().unwrap()).is_ok());
    let served = Served { shared, ..served };
    let body = serde_json::json!({
        "name": "ana",
        "email": "ana@example.com",
        "slack": "U024BE7LH",
        "whatsapp": "+1 555 123 4567",
        "notices": [{"kind": "card", "way": "slack"}, {"kind": "verdict"}],
    });
    let (status, said) = served
        .asked(LAPTOP, "POST", PROFILE_PATH, &body.to_string())
        .await;
    assert_eq!(status, StatusCode::OK, "{said}");
    let page: PeoplePage = serde_json::from_str(&said).unwrap();
    let row = |name: &str| page.people.iter().find(|row| row.name == name).unwrap();
    assert_eq!(
        row("ana").addresses,
        Some(Addresses {
            email: Some("ana@example.com".into()),
            slack: Some("U024BE7LH".into()),
            whatsapp: Some("+15551234567".into()),
        })
    );
    assert_eq!(
        row("ana").notices.as_ref().map(|ways| ways[0].way.clone()),
        Some(Some("slack".into()))
    );
    assert_eq!(
        (row("bo").addresses.clone(), row("bo").notices.clone()),
        (None, None)
    );
    let profiles = served.profiles();
    assert_eq!(
        profiles.get("ana").unwrap().emails,
        ["ana@example.com"],
        "the email set replaces the one she held"
    );
    let reach = profiles.reach("ana", "card").unwrap();
    assert_eq!((reach.way(), reach.address()), ("slack", "U024BE7LH"));
    // An address its sender refuses is refused by its rule, its value unsaid; another's are not
    // hers to write; and either writes nothing.
    for (body, refused, value) in [
        (
            r#"{"name":"ana","slack":"C024BE7LH"}"#,
            "the Slack member id is none",
            "C024BE7LH",
        ),
        (
            r#"{"name":"ana","notices":[{"kind":"card","way":"sms"}]}"#,
            "a notice goes by one of email, slack, whatsapp",
            "sms",
        ),
        (
            r#"{"name":"bo","whatsapp":"+15551234567"}"#,
            "only the person at the machine, or bo,",
            "+15551234567",
        ),
    ] {
        let (status, said) = served.asked(LAPTOP, "POST", PROFILE_PATH, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{said}");
        assert!(said.contains(refused) && !said.contains(value), "{said}");
    }
    assert_eq!(served.profiles(), profiles);
}
