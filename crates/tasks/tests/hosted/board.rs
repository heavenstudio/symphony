//! The board's store on a hosted database: made and moved by its deployment's migration alone,
//! never in a database that holds another's objects; the same work answered as the local board
//! answers it, on PostgreSQL 16 and 18; a writer behind held to; two migrations at once; and one
//! project's login kept out of another's database.

use crate::cluster::{self, MAJORS, Project, environment, settings};
use crate::store::{Fixture, HOST, NOW, Seed};
use symphony_board::pg::hosted::Client;
use symphony_board::{Error, NewTask, Opening, Program, Schema, Store};

/// The store `how` opens on `project`'s board, its clock at `NOW` and no process alive.
fn opened(project: &Project, how: &Opening) -> Result<Store, Error> {
    let class = if how.migrating { "maintenance" } else { "web" };
    Store::opening_hosted(
        project.client(class),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
        how,
    )
}

fn refused(opened: Result<Store, Error>) -> String {
    match opened {
        Err(Error::Refused(why)) => why,
        Err(other) => panic!("refused, not {other:?}"),
        Ok(_) => panic!("refused, not opened"),
    }
}

/// How many tables and functions the project's database holds, by the operator's count.
fn objects(project: &Project) -> String {
    project.cluster.value(
        &project.name,
        "SELECT (SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = 'public') + (SELECT count(*) FROM pg_proc p JOIN pg_namespace n \
         ON n.oid = p.pronamespace WHERE n.nspname = 'public')",
    )
}

fn version(project: &Project) -> String {
    project.cluster.value(
        &project.name,
        "SELECT version FROM meta WHERE id = '\"schema\"'",
    )
}

#[test]
fn a_hosted_board_is_made_by_its_migration_alone_and_then_opens_to_every_writer() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        for how in [Opening::write(), Opening::read()] {
            let why = refused(opened(&project, &how));
            assert!(
                why.contains("its deployment's migration"),
                "PostgreSQL {major}: {why}"
            );
            assert_eq!(objects(&project), "0", "nothing was made: {why}");
        }
        let store = opened(&project, &Opening::write().migrating()).unwrap();
        assert_eq!(
            version(&project),
            Schema::this_build().version().to_string()
        );
        store
            .create(&NewTask::new("sim/jump", "Jump departure", "owner"))
            .unwrap();
        drop(store);
        let writer = opened(&project, &Opening::write()).unwrap();
        assert_eq!(writer.get("sim/jump").unwrap().title, "Jump departure");
        let reader = opened(&project, &Opening::read()).unwrap();
        assert_eq!(reader.get("sim/jump").unwrap().title, "Jump departure");
        // A migration run again finds nothing to do.
        opened(&project, &Opening::write().migrating()).unwrap();
        assert_eq!(writer.get("sim/jump").unwrap().title, "Jump departure");
    }
}

/// A hosted store has no database of its own to give out ([`Store::database`] panics on one):
/// what names it in a scope, an operation or a binding is its label, and what it sent is counted
/// in the board's one log, the one each of its clients counts in.
#[test]
fn a_hosted_store_is_named_by_its_label_and_counts_its_requests_without_a_local_database() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        let store = opened(&project, &Opening::write().migrating()).unwrap();
        let client = project.client("web");
        assert_eq!(store.board_id(), client.label(), "PostgreSQL {major}");
        for said in [&project.name, &project.password] {
            assert!(!store.board_id().contains(said.as_str()), "{said} said");
        }
        let scope = symphony_board::execution::board_scope("symphony", &store);
        assert_eq!(
            (scope.project.as_str(), scope.board.as_str()),
            ("symphony", client.label())
        );

        let log = store.query_log();
        assert!(std::sync::Arc::ptr_eq(&log, &client.queries()));
        let sent = || log.report(symphony_board::sweep::trigger::now()).0.count;
        let before = sent();
        store.board().unwrap();
        let after = sent();
        assert!(after > before, "PostgreSQL {major}: {before} -> {after}");
    }
}

#[test]
fn a_database_that_holds_anothers_objects_is_never_taken_for_an_empty_board() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        project
            .client("web")
            .simple("CREATE TABLE theirs (kept text); INSERT INTO theirs VALUES ('kept');")
            .unwrap();
        let why = refused(opened(&project, &Opening::write().migrating()));
        assert!(
            why.contains("holds tables or functions"),
            "PostgreSQL {major}: {why}"
        );
        assert_eq!(objects(&project), "1");
        assert_eq!(
            project
                .cluster
                .value(&project.name, "SELECT kept FROM theirs"),
            "kept"
        );
    }
}

/// What a board answers to the same work, in text: the tasks, a thread, and each search.
fn worked(store: &Store) -> String {
    let titles = [
        (
            "sim/jump",
            "Jump departure ÉCOLE naïve",
            "The ship leaves ﬁnally; Straße and İstanbul",
        ),
        (
            "sim/dock",
            "Dock arrival Ångström",
            "ǅungla ΌΣΟΣ ὈΔΥΣΣΕΎΣ résumé",
        ),
        (
            "ui/map",
            "Map 東京 кириллица",
            "Zoë's coöperative map, ½ done, ⅷ parts",
        ),
    ];
    for (slug, title, body) in titles {
        let mut new = NewTask::new(slug, title, "owner");
        new.body = body.to_string();
        store.create(&new).unwrap();
    }
    store.claim("sim/jump", "amber-heron").unwrap();
    store
        .say(
            "sim/jump",
            "amber-heron",
            "Départ confirmé for the Ecole",
            None,
        )
        .unwrap();
    store
        .say("sim/dock", "brisk-finch", "odysseus and osos arrive", None)
        .unwrap();
    let mut said = String::new();
    for query in [
        "ecole",
        "naive",
        "strasse",
        "istanbul",
        "angstrom",
        "dzungla",
        "οσος",
        "οδυσσευσ",
        "resume",
        "東京",
        "кириллица",
        "zoe cooperative",
        "finally",
        "depart",
        "nothing-matches",
    ] {
        let hits: Vec<String> = store
            .search(query)
            .unwrap()
            .into_iter()
            .map(|hit| format!("{}@{:.6}", hit.slug, hit.score))
            .collect();
        said.push_str(&format!("{query}: {hits:?}\n"));
    }
    for slug in ["sim/jump", "sim/dock", "ui/map"] {
        said.push_str(&format!("{:?}\n", store.get(slug).unwrap()));
        let thread: Vec<(String, String)> = store
            .thread(slug)
            .unwrap()
            .into_iter()
            .map(|m| (m.sender, m.text))
            .collect();
        said.push_str(&format!("{thread:?}\n"));
    }
    // The whole board, read as a statement for each of its lists in one transaction
    // (`Pg::Listed`): every task and session in the board's order.
    let board = store.board().unwrap();
    let tasks: Vec<(&str, &str)> = (board.tasks.iter())
        .map(|task| (task.slug.as_str(), task.status.as_str()))
        .collect();
    let sessions: Vec<&str> = (board.sessions.iter())
        .map(|session| session.row.handle.as_str())
        .collect();
    said.push_str(&format!(
        "{tasks:?} {sessions:?} {}\n",
        board.latest_message
    ));
    let inbox: Vec<String> = (store.inbox("amber-heron").unwrap().into_iter())
        .map(|message| message.text)
        .collect();
    said.push_str(&format!("{inbox:?}\n"));
    said
}

#[test]
fn the_same_work_is_answered_alike_on_hosted_16_hosted_18_and_the_local_board() {
    let seed = || {
        Seed::default()
            .session("amber-heron")
            .session("brisk-finch")
    };
    let local = worked(&Fixture::new(seed()).store);
    assert!(local.contains("ecole: [\"sim/jump@"), "{local}");
    assert!(
        local.contains("(\"sim/jump\", \"in_progress\")") && local.contains("\"amber-heron\""),
        "{local}"
    );
    let mut generations = Vec::new();
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        let store = opened(&project, &Opening::write().migrating()).unwrap();
        store.copy(&seed().dump).unwrap();
        assert_eq!(worked(&store), local, "PostgreSQL {major}");
        // The copy gave the board a recovery generation of its own, as on any board.
        let generation = store.recovery_generation().unwrap();
        assert!(generation.is_some(), "PostgreSQL {major}");
        assert!(!generations.contains(&generation), "PostgreSQL {major}");
        generations.push(generation);
    }
}

#[test]
fn a_writer_behind_holds_the_migration_and_a_board_moved_on_refuses_it() {
    let newer = || {
        Schema::this_build()
            .then_each("CREATE TABLE IF NOT EXISTS fixture_next (id jsonb PRIMARY KEY, doc jsonb);")
    };
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        let old = Program::named("tasks", "/opt/old/tasks");
        opened(&project, &Opening::write().migrating().by(old.clone())).unwrap();
        let stale = opened(&project, &Opening::write().by(old.clone())).unwrap();
        // The newer build's migration is held while the old writer was seen within the week.
        let held = refused(opened(&project, &Opening::write().migrating().at(newer())));
        assert!(
            held.contains("/opt/old/tasks"),
            "PostgreSQL {major}: {held}"
        );
        assert_eq!(
            version(&project),
            Schema::this_build().version().to_string()
        );
        // Told to leave it behind, it moves the board, and the old writer's next write is refused.
        opened(
            &project,
            &Opening::write().migrating().leave_behind().at(newer()),
        )
        .unwrap();
        assert_eq!(version(&project), newer().version().to_string());
        let late = stale.create(&NewTask::new("late/one", "Written behind", "owner"));
        assert!(
            matches!(late, Err(Error::Refused(_))),
            "PostgreSQL {major}: {late:?}"
        );
        assert_eq!(
            project
                .cluster
                .value(&project.name, "SELECT count(*) FROM task"),
            "0"
        );
        refused(opened(&project, &Opening::write().by(old)));
    }
}

#[test]
fn two_migrations_at_once_make_the_board_once() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        let both: Vec<Result<Store, Error>> = std::thread::scope(|scope| {
            let running: Vec<_> = (0..2)
                .map(|_| scope.spawn(|| opened(&project, &Opening::write().migrating())))
                .collect();
            running.into_iter().map(|one| one.join().unwrap()).collect()
        });
        for one in &both {
            assert!(one.is_ok(), "PostgreSQL {major}: {:?}", one.as_ref().err());
        }
        assert_eq!(
            project.cluster.value(
                &project.name,
                "SELECT count(*) FROM meta WHERE id = '\"schema\"'"
            ),
            "1"
        );
        both[0]
            .as_ref()
            .unwrap()
            .create(&NewTask::new("sim/jump", "Jump departure", "owner"))
            .unwrap();
        assert_eq!(
            both[1].as_ref().unwrap().get("sim/jump").unwrap().title,
            "Jump departure"
        );
    }
}

#[test]
fn two_processes_migrations_at_once_make_the_board_once_and_neither_is_refused() {
    // Two pools, as two processes of the deployment have, so neither waits for the other's
    // connection: the one that read no board and then finds the other's tables decides again.
    for major in MAJORS {
        for _ in 0..2 {
            let project = cluster::cluster(major).project();
            let start = std::sync::Barrier::new(2);
            let both: Vec<Result<Store, Error>> = std::thread::scope(|scope| {
                let running: Vec<_> = ["localhost", "127.0.0.1"]
                    .map(|host| {
                        let (project, start) = (&project, &start);
                        scope.spawn(move || {
                            let client = Client::connect(
                                &cluster::budgeted("", 2),
                                project.environment_at(host, "maintenance"),
                            )
                            .unwrap();
                            start.wait();
                            Store::opening_hosted(
                                client,
                                Box::new(|| NOW),
                                Box::new(|_| false),
                                HOST,
                                &Opening::write().migrating(),
                            )
                        })
                    })
                    .into_iter()
                    .collect();
                running.into_iter().map(|one| one.join().unwrap()).collect()
            });
            for one in &both {
                assert!(one.is_ok(), "PostgreSQL {major}: {:?}", one.as_ref().err());
            }
            assert_eq!(
                project.cluster.value(
                    &project.name,
                    "SELECT count(*) FROM meta WHERE id = '\"schema\"'"
                ),
                "1"
            );
        }
    }
}

#[test]
fn a_migration_runs_as_the_maintenance_class_alone_and_each_class_has_its_own_pool() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        let as_web = Store::opening_hosted(
            project.client("web"),
            Box::new(|| NOW),
            Box::new(|_| false),
            HOST,
            &Opening::write().migrating(),
        );
        let why = refused(as_web);
        assert!(
            why.contains("its migration runs as the class maintenance of its budget, not web")
                && why.ends_with("nothing was written"),
            "PostgreSQL {major}: {why}"
        );
        assert_eq!(objects(&project), "0", "PostgreSQL {major}");
        opened(&project, &Opening::write().migrating()).unwrap();
        // The web client asked after the maintenance one is the web class's pool of three,
        // not the maintenance pool of one it would have been handed by the board's name alone.
        let (web, maintenance) = (project.client("web"), project.client("maintenance"));
        assert_eq!((web.class(), maintenance.class()), ("web", "maintenance"));
        let idle = |client: &Client| client.counts().idle;
        assert_eq!(Project::value(&web, "SELECT 1"), "1");
        assert_eq!(
            (idle(&web), idle(&maintenance)),
            (1, 0),
            "PostgreSQL {major}"
        );
        assert_eq!(Project::value(&maintenance, "SELECT 1"), "1");
        assert_eq!(
            (idle(&web), idle(&maintenance)),
            (1, 1),
            "PostgreSQL {major}"
        );
        assert_eq!(
            project.cluster.sessions(&project.name),
            2,
            "PostgreSQL {major}"
        );
    }
}

#[test]
fn one_projects_login_reaches_no_other_projects_board() {
    for major in MAJORS {
        let cluster = cluster::cluster(major);
        let (ours, theirs) = (cluster.project(), cluster.project());
        opened(&ours, &Opening::write().migrating())
            .unwrap()
            .create(&NewTask::new("ours/one", "Ours", "owner"))
            .unwrap();
        let other = opened(&theirs, &Opening::write().migrating()).unwrap();
        assert!(
            other.get("ours/one").is_err(),
            "PostgreSQL {major}: their board holds none of ours"
        );
        // Our login at their database: the server lets it nowhere, and the refusal names nothing.
        let crossed = format!(
            "postgresql://{}:{}@localhost:{}/{}",
            ours.name,
            cluster::escaped(&ours.password),
            cluster.port,
            theirs.name
        );
        let client = Client::connect(
            &settings(""),
            environment(&[
                ("URL", crossed),
                ("CA", cluster.ca.display().to_string()),
                ("CLASS", "web".into()),
            ]),
        )
        .unwrap();
        let why = crate::cluster::refused(
            client.request(&[symphony_board::pg::Step::bare("SELECT count(*) FROM task")]),
        );
        assert!(
            why.contains("the login may not connect to the database"),
            "PostgreSQL {major}: {why}"
        );
        assert!(
            !why.contains(&ours.password) && !why.contains(&theirs.name),
            "{why}"
        );
    }
}

#[test]
fn a_made_board_is_its_logins_own_with_no_definer_function_extension_or_other_schema() {
    for major in MAJORS {
        let project = cluster::cluster(major).project();
        opened(&project, &Opening::write().migrating()).unwrap();
        let count = |sql: &str| project.cluster.value(&project.name, sql);
        assert_ne!(objects(&project), "0");
        // Every table, index, sequence and function is the login's, and none runs as its definer.
        assert_eq!(
            count(&format!(
                "SELECT (SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid = \
                 c.relnamespace WHERE n.nspname = 'public' AND pg_get_userbyid(c.relowner) <> \
                 '{0}') + (SELECT count(*) FROM pg_proc p JOIN pg_namespace n ON n.oid = \
                 p.pronamespace WHERE n.nspname = 'public' AND (p.prosecdef OR \
                 pg_get_userbyid(p.proowner) <> '{0}'))",
                project.name
            )),
            "0",
            "PostgreSQL {major}"
        );
        // Nothing was installed or made beside the board: the extension every database has,
        // the schemas every database has, the two languages a board is written in.
        assert_eq!(
            count("SELECT string_agg(extname, ',') FROM pg_extension"),
            "plpgsql"
        );
        assert_eq!(
            count(
                "SELECT string_agg(nspname, ',' ORDER BY nspname) FROM pg_namespace WHERE \
                 nspname !~ '^pg_' AND nspname <> 'information_schema'"
            ),
            "public"
        );
        assert_eq!(
            count(
                "SELECT string_agg(DISTINCT l.lanname, ',' ORDER BY l.lanname) FROM pg_proc p \
                 JOIN pg_language l ON l.oid = p.prolang JOIN pg_namespace n ON n.oid = \
                 p.pronamespace WHERE n.nspname = 'public'"
            ),
            "plpgsql,sql"
        );
        // And the server holds no role or database the migration made.
        assert_eq!(
            count(&format!(
                "SELECT count(*) FROM pg_database WHERE datdba = (SELECT oid FROM pg_roles \
                 WHERE rolname = '{}')",
                project.name
            )),
            "1"
        );
    }
}
