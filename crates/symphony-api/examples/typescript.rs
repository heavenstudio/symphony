//! The web client's wire types: the TypeScript declarations of the types it reads and the
//! constants it names, written from these Rust types so they are never written by hand twice
//! (apps/symphony-web/src/wire.ts); and, with `cases`, what the functions of this crate and of
//! symphony-blocks the client carries in TypeScript answer for a set of inputs, with a sample of
//! each read the client checks at run time (apps/symphony-web/src/wire.cases.json), which the
//! client's unit tests hold its TypeScript to.
//!
//!     cargo run -p symphony-api --features typescript --example typescript          # wire.ts
//!     cargo run -p symphony-api --features typescript --example typescript cases    # the cases
//!
//! `just client-wire` writes both files from it (apps/symphony-web/wire.sh), and `just client-lint`
//! fails when a committed file is not what it prints. Every type a declared type names is declared
//! too, or this refuses. The cases hold the source of each function the client carries
//! ([`CARRIED`]), so a change to one fails that check even where no case's input shows it.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::Write as _;

use serde_json::{Value, json};
use symphony_api as api;
use ts_rs::{Config, TS};

/// One type's declaration, the names of the types it names, and its own name.
struct Declared {
    name: String,
    decl: String,
    names: Vec<String>,
}

fn declared<T: TS + 'static>(cfg: &Config) -> Declared {
    Declared {
        name: T::ident(cfg),
        decl: T::decl(cfg),
        names: T::dependencies(cfg)
            .into_iter()
            .map(|dependency| dependency.ts_name)
            .collect(),
    }
}

fn main() {
    let printed = match std::env::args().nth(1).as_deref() {
        None => write!(std::io::stdout().lock(), "{}", types()),
        Some("cases") => match cases() {
            Ok(cases) => writeln!(std::io::stdout().lock(), "{cases}"),
            Err(why) => {
                let _ = writeln!(std::io::stderr(), "typescript: {why}");
                std::process::exit(1);
            }
        },
        Some(other) => {
            let _ = writeln!(
                std::io::stderr(),
                "typescript: no `{other}`; it prints wire.ts, or with `cases` the cases"
            );
            std::process::exit(2);
        }
    };
    if let Err(error) = printed {
        let _ = writeln!(std::io::stderr(), "typescript: {error}");
        std::process::exit(1);
    }
}

/// apps/symphony-web/src/wire.ts, before Biome formats it.
fn types() -> String {
    let types = declarations();
    let names: BTreeSet<&str> = types.iter().map(|one| one.name.as_str()).collect();
    for one in &types {
        for named in &one.names {
            assert!(
                names.contains(named.as_str()),
                "{} names {named}, which the file does not declare: add it to the list",
                one.name
            );
        }
    }
    let mut out = String::from(
        "// The Symphony's wire types as the web client reads them, written from crates/symphony-api\n\
         // by its `typescript` example (`just client-wire`). Never edited by hand: `just client-lint`\n\
         // fails when this file and the Rust types disagree.\n\n",
    );
    out.push_str(&constants());
    for one in &types {
        let _ = write!(out, "\nexport {}\n", one.decl);
    }
    out
}

/// The constants the client names, as wire.ts declares them.
fn constants() -> String {
    let mut out = String::new();
    let text = |text: &str| Value::from(text).to_string();
    let constants = [
        ("PROJECT_ELEMENT", text(api::PROJECT_ELEMENT)),
        ("BOARD_PATH", text(api::BOARD_PATH)),
        ("FEED_PATH", text(api::FEED_PATH)),
        ("FEED_EVENT", text(api::FEED_EVENT)),
        ("PING_EVENT", text(api::PING_EVENT)),
        ("SERVER_EVENT", text(api::SERVER_EVENT)),
        ("CACHE_FORMAT", api::CACHE_FORMAT.to_string()),
        ("BOARD_CARDS", api::BOARD_CARDS.to_string()),
        ("ASK_LATE_SECONDS", api::ASK_LATE_SECONDS.to_string()),
        ("FILE_QUERY", text(api::FILE_QUERY)),
        ("REPORTS_PATH", text(api::REPORTS_PATH)),
        ("REPORTS_LANDED_PATH", text(api::REPORTS_LANDED_PATH)),
        ("SETTINGS_PATH", text(api::SETTINGS_PATH)),
        ("SETTING_WRITE_PATH", text(api::SETTING_WRITE_PATH)),
        ("PEOPLE_PATH", text(api::PEOPLE_PATH)),
        ("PEOPLE_CHANGE_PATH", text(api::PEOPLE_CHANGE_PATH)),
        ("PROFILE_PATH", text(api::PROFILE_PATH)),
        ("CARD_OWNERS_PATH", text(api::CARD_OWNERS_PATH)),
        ("TASK_HAND_ROUTE", text(api::TASK_HAND_ROUTE)),
        ("CARD_HAND_ROUTE", text(api::CARD_HAND_ROUTE)),
        ("RUNS_PATH", text(api::RUNS_PATH)),
        ("RUN_PATH", text(api::RUN_PATH)),
        ("RUN_LOG_PATH", text(api::RUN_LOG_PATH)),
        ("RUN_LOG_TAIL", api::RUN_LOG_TAIL.to_string()),
        ("RUN_LOG_MOST", api::RUN_LOG_MOST.to_string()),
        ("RUNS_DAYS", api::RUNS_DAYS.to_string()),
        ("SCHEMA_UPDATES_PATH", text(api::SCHEMA_UPDATES_PATH)),
        ("VERSION_HEADER", text(api::VERSION_HEADER)),
        ("GETTING_STARTED", text(api::GETTING_STARTED)),
        ("VERDICT_TASKS", text(api::VERDICT_TASKS)),
        ("UPLOAD_PATH", text(api::UPLOAD_PATH)),
        ("UPLOAD_MAX_BYTES", api::UPLOAD_MAX_BYTES.to_string()),
        ("UPLOAD_ACCEPT", text(api::UPLOAD_ACCEPT)),
        (
            "QUICK_TASK_TITLE_MAX",
            api::QUICK_TASK_TITLE_MAX.to_string(),
        ),
        ("TASK_ACTION_ROUTE", text(api::TASK_ACTION_ROUTE)),
        ("TASK_PASS_ROUTE", text(api::TASK_PASS_ROUTE)),
        ("TASK_TOKENS_PATH", text(api::TASK_TOKENS_PATH)),
        ("TASK_COSTS_PATH", text(api::TASK_COSTS_PATH)),
        ("TASK_COSTS_MOST", api::TASK_COSTS_MOST.to_string()),
        ("TASK_GATES_PATH", text(api::TASK_GATES_PATH)),
        ("TASK_TIMELINE_PATH", text(api::TASK_TIMELINE_PATH)),
        ("TASK_DIFF_PATH", text(api::TASK_DIFF_PATH)),
        ("TASK_DIFF_FILE_PATH", text(api::TASK_DIFF_FILE_PATH)),
        ("TIMELINE_EVERY", api::timeline::TIMELINE_EVERY.to_string()),
        (
            "TIMELINE_HISTORY_LIMIT",
            api::timeline::TIMELINE_HISTORY_LIMIT.to_string(),
        ),
        (
            "TIMELINE_HISTORY_SOURCES",
            api::timeline::TIMELINE_HISTORY_SOURCES.to_string(),
        ),
        ("TASK_ATTEMPTS_PATH", text(api::TASK_ATTEMPTS_PATH)),
        ("ATTEMPT_CONTROL_ROUTE", text(api::ATTEMPT_CONTROL_ROUTE)),
        ("ATTEMPT_VIEW_ROUTE", text(api::ATTEMPT_VIEW_ROUTE)),
        ("AGENT_FOCUS_PATH", text(api::AGENT_FOCUS_PATH)),
        ("AGENT_SCREEN_PATH", text(api::AGENT_SCREEN_PATH)),
        ("WORKFLOW_PATH", text(api::workflow::WORKFLOW_PATH)),
        (
            "WORKFLOW_ACTIONS_PATH",
            text(api::workflow::WORKFLOW_ACTIONS_PATH),
        ),
        (
            "WORKFLOW_APPLY_ROUTE",
            text(api::workflow::WORKFLOW_APPLY_ROUTE),
        ),
        (
            "WORKFLOW_GUARDS_PATH",
            text(api::workflow::WORKFLOW_GUARDS_PATH),
        ),
    ];
    for (name, value) in constants {
        let _ = writeln!(out, "export const {name} = {value};");
    }
    let _ = writeln!(
        out,
        "export const SYMPHONY_ROUTES = {} as const;",
        json!(api::SYMPHONY_ROUTES)
    );
    let pages = json!({
        "HOME": api::page_names::HOME,
        "QUEUE": api::page_names::QUEUE,
        "DOCS": api::page_names::DOCS,
        "BOARD": api::page_names::BOARD,
        "REPORTS": api::page_names::REPORTS,
        "SETTINGS": api::page_names::SETTINGS,
        "PEOPLE": api::page_names::PEOPLE,
        "RUNS": api::page_names::RUNS,
        "AGENTS": api::page_names::AGENTS,
        "PROJECTS": api::page_names::PROJECTS,
    });
    let _ = writeln!(out, "export const PAGE_NAMES = {pages} as const;");
    out
}

/// The types the client reads, in wire.ts's order.
fn declarations() -> Vec<Declared> {
    // JSON numbers are read by the browser as doubles whatever their Rust width.
    let cfg = Config::new().with_large_int("number");
    [
        board_declarations(&cfg),
        page_declarations(&cfg),
        document_declarations(&cfg),
        task_declarations(&cfg),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The types the frame, the feed, the queue, Reports and Settings read.
fn board_declarations(cfg: &Config) -> Vec<Declared> {
    vec![
        declared::<api::ProjectView>(cfg),
        declared::<api::PersonName>(cfg),
        declared::<api::NavItem>(cfg),
        declared::<api::Board>(cfg),
        declared::<api::Card>(cfg),
        declared::<api::Picture>(cfg),
        declared::<api::Counts>(cfg),
        declared::<api::TaskCounts>(cfg),
        declared::<api::Progress>(cfg),
        declared::<api::Region>(cfg),
        declared::<api::ProgressBar>(cfg),
        declared::<api::Delta>(cfg),
        declared::<api::Change>(cfg),
        declared::<api::Queue>(cfg),
        declared::<api::QueueItem>(cfg),
        declared::<api::QueueAsk>(cfg),
        declared::<api::Activity>(cfg),
        declared::<api::ActivityItem>(cfg),
        declared::<api::Pending>(cfg),
        declared::<api::PendingItem>(cfg),
        // Reports'.
        declared::<api::Grain>(cfg),
        declared::<api::report_page::ReportMark>(cfg),
        declared::<api::report_page::Spend>(cfg),
        declared::<api::report_page::LandedPage>(cfg),
        declared::<api::report_page::LandedRow>(cfg),
        declared::<api::report_page::LandedFigures>(cfg),
        declared::<api::report_page::KindRow>(cfg),
        declared::<api::report_page::ReviewRow>(cfg),
        // The Settings page's.
        declared::<api::SettingsPage>(cfg),
        declared::<api::SettingsFile>(cfg),
        declared::<api::SettingsSection>(cfg),
        declared::<api::SettingsEntry>(cfg),
        declared::<api::EntryState>(cfg),
        declared::<api::EntryNeed>(cfg),
        declared::<api::Setting>(cfg),
        declared::<api::SettingSource>(cfg),
        declared::<api::SettingScope>(cfg),
        declared::<api::SettingClass>(cfg),
        declared::<api::SettingKind>(cfg),
        declared::<api::StoredSetting>(cfg),
        declared::<api::StoredChange>(cfg),
        declared::<api::SettingAction>(cfg),
        declared::<api::SettingValue>(cfg),
        declared::<api::SettingWrite>(cfg),
        declared::<api::SettingWritten>(cfg),
    ]
}

/// The types the People page, the Runs page, the task board and the agents page read, after the
/// board's in wire.ts.
fn page_declarations(cfg: &Config) -> Vec<Declared> {
    vec![
        // The People page's.
        declared::<api::PeoplePage>(cfg),
        declared::<api::PersonRow>(cfg),
        declared::<api::Addresses>(cfg),
        declared::<api::NoticeWay>(cfg),
        declared::<api::GroupRow>(cfg),
        declared::<api::PeopleLogLine>(cfg),
        declared::<api::PeopleAction>(cfg),
        declared::<api::PeopleChange>(cfg),
        declared::<api::ProfileWrite>(cfg),
        declared::<api::MachineProfile>(cfg),
        // Whose a card or a task is.
        declared::<api::Owner>(cfg),
        declared::<api::CardOwners>(cfg),
        // The Runs page's, its schema updates too.
        declared::<api::RunsPage>(cfg),
        declared::<api::RunPage>(cfg),
        declared::<api::RunLine>(cfg),
        declared::<api::End>(cfg),
        declared::<api::Suite>(cfg),
        declared::<api::FailedTest>(cfg),
        declared::<api::RunLog>(cfg),
        declared::<api::SchemaUpdates>(cfg),
        declared::<api::SchemaUpdate>(cfg),
        declared::<api::UpdateState>(cfg),
        declared::<api::UpdateTime>(cfg),
        // The task board's, its tasks and the machines' work.
        declared::<api::TaskListing>(cfg),
        declared::<api::WaitingBoard>(cfg),
        declared::<api::WaitingColumn>(cfg),
        declared::<api::Task>(cfg),
        declared::<api::WaitsOn>(cfg),
        declared::<api::Orchestrator>(cfg),
        declared::<api::Role>(cfg),
        declared::<api::FieldValue>(cfg),
        declared::<api::TaskChange>(cfg),
        declared::<api::TaskFields>(cfg),
        declared::<api::workflow::TaskPlace>(cfg),
        declared::<api::workflow::ManualRecovery>(cfg),
        declared::<api::workflow::Pair>(cfg),
        declared::<api::MachineWork>(cfg),
        declared::<api::MachineObservation>(cfg),
        declared::<api::WorkOperation>(cfg),
        declared::<api::WorkCounts>(cfg),
        declared::<api::WorkGuidance>(cfg),
        declared::<api::WorkFreshness>(cfg),
        declared::<api::WorkKind>(cfg),
        declared::<api::WorkState>(cfg),
        // The agents page's: its agents, the tasks they hold and where Go to session goes.
        declared::<api::AgentPage>(cfg),
        declared::<api::Agents>(cfg),
        declared::<api::Agent>(cfg),
        declared::<api::HeldTask>(cfg),
        declared::<api::AgentRun>(cfg),
        declared::<api::AgentSaid>(cfg),
        declared::<api::AgentSlots>(cfg),
        declared::<api::AgentTokens>(cfg),
        declared::<api::AgentCost>(cfg),
        declared::<api::AgentUnpriced>(cfg),
        declared::<api::TaskTokens>(cfg),
        declared::<api::RoleTokens>(cfg),
        declared::<api::RoundTokens>(cfg),
        declared::<api::TokenRow>(cfg),
        declared::<api::TaskCosts>(cfg),
        declared::<api::TaskCost>(cfg),
        declared::<api::SessionGo>(cfg),
        declared::<api::AgentFocus>(cfg),
        declared::<api::AgentScreen>(cfg),
    ]
}

/// The blocks a plugin renders to, and the documents', the asks' and the uploads' types.
fn document_declarations(cfg: &Config) -> Vec<Declared> {
    vec![
        // The blocks a plugin renders to, and what they hold.
        declared::<api::Block>(cfg),
        declared::<api::Span>(cfg),
        declared::<api::LinkTarget>(cfg),
        declared::<api::Presentation>(cfg),
        declared::<api::Image>(cfg),
        declared::<api::Cell>(cfg),
        declared::<api::Verdict>(cfg),
        declared::<api::ThreadEntry>(cfg),
        declared::<api::CardState>(cfg),
        declared::<api::CardGroup>(cfg),
        declared::<api::ThreadAuthor>(cfg),
        declared::<api::CalloutKind>(cfg),
        declared::<api::FigureSize>(cfg),
        declared::<api::ContainerKind>(cfg),
        declared::<api::InlineKind>(cfg),
        declared::<api::VectorShape>(cfg),
        declared::<api::VectorKind>(cfg),
        declared::<api::Media>(cfg),
        declared::<api::GalleryItem>(cfg),
        declared::<api::Viewer>(cfg),
        declared::<api::View>(cfg),
        // A document's and a folder's pages: their reads, the feed's changes and the asks.
        declared::<api::Document>(cfg),
        declared::<api::DocumentComparison>(cfg),
        declared::<api::DocumentOutline>(cfg),
        declared::<api::DocumentSections>(cfg),
        declared::<api::Crumb>(cfg),
        declared::<api::Heading>(cfg),
        declared::<api::Fragment>(cfg),
        declared::<api::Section>(cfg),
        declared::<api::Difference>(cfg),
        declared::<api::SectionComparison>(cfg),
        declared::<api::DocumentCard>(cfg),
        declared::<api::DocumentTask>(cfg),
        declared::<api::Folder>(cfg),
        declared::<api::FolderEntry>(cfg),
        declared::<api::DocumentChange>(cfg),
        declared::<api::SectionPosition>(cfg),
        declared::<api::ItemChange<api::DocumentCard>>(cfg),
        declared::<api::Version>(cfg),
        declared::<api::DocumentDiff>(cfg),
        declared::<api::Ask>(cfg),
        declared::<api::AskAnswer>(cfg),
        declared::<api::AskMessage>(cfg),
        // A file attached to a post.
        declared::<api::Upload>(cfg),
    ]
}

/// The types a task's page reads, after the others in wire.ts.
fn task_declarations(cfg: &Config) -> Vec<Declared> {
    vec![
        // A custom workflow: the definition the board and a task's page draw by, the owner's next
        // actions on a task, and the operation the page posts with its answer.
        declared::<api::workflow::BoardWorkflow>(cfg),
        declared::<api::workflow::WorkflowRead>(cfg),
        declared::<api::workflow::Definition>(cfg),
        declared::<api::workflow::Phase>(cfg),
        declared::<api::workflow::State>(cfg),
        declared::<api::workflow::Position>(cfg),
        declared::<api::workflow::Recovery>(cfg),
        declared::<api::workflow::RecoveryAction>(cfg),
        declared::<api::workflow::Guard>(cfg),
        declared::<api::workflow::Authority>(cfg),
        declared::<api::workflow::Effect>(cfg),
        declared::<api::workflow::RetryBound>(cfg),
        declared::<api::workflow::Edge>(cfg),
        declared::<api::workflow::Revision>(cfg),
        declared::<api::workflow::Source>(cfg),
        declared::<api::workflow::PositionSnapshot>(cfg),
        declared::<api::workflow::InputValue>(cfg),
        declared::<api::workflow::OperationRequest>(cfg),
        declared::<api::workflow::NextAction>(cfg),
        declared::<api::workflow::NextActions>(cfg),
        declared::<api::workflow::ActionsRead>(cfg),
        declared::<api::workflow::TakenOutput>(cfg),
        declared::<api::workflow::OperationResult>(cfg),
        declared::<api::workflow::RefusalCode>(cfg),
        declared::<api::workflow::Role>(cfg),
        declared::<api::workflow::Slot>(cfg),
        declared::<api::workflow::Dependency>(cfg),
        declared::<api::workflow::UnsuccessfulDependency>(cfg),
        declared::<api::workflow::Outcome>(cfg),
        declared::<api::workflow::Completion>(cfg),
        declared::<api::workflow::Operation>(cfg),
        declared::<api::workflow::LegacyStatus>(cfg),
        // A task's guards as the board's records have them, and the guard that refused a take.
        declared::<api::workflow::TaskGuards>(cfg),
        declared::<api::workflow::GuardAuthority>(cfg),
        declared::<api::workflow::GuardNeed>(cfg),
        declared::<api::workflow::GuardedEdge>(cfg),
        declared::<api::workflow::GuardEntry>(cfg),
        declared::<api::workflow::GuardMet>(cfg),
        declared::<api::workflow::GuardRecord>(cfg),
        declared::<api::workflow::GuardRefusal>(cfg),
        declared::<api::workflow::GuardWhy>(cfg),
        declared::<api::workflow::GuardStanding>(cfg),
        declared::<api::workflow::Attested>(cfg),
        declared::<api::workflow::Attestee>(cfg),
        // A task's page: its brief, thread, landing offer and gates (its holders' sessions are the
        // agents page's types).
        declared::<api::TaskPage>(cfg),
        declared::<api::TaskEntry>(cfg),
        declared::<api::HeldWords>(cfg),
        declared::<api::Message>(cfg),
        declared::<api::TaskHistory>(cfg),
        declared::<api::TaskBrief>(cfg),
        declared::<api::TaskOffer>(cfg),
        declared::<api::TaskGates>(cfg),
        declared::<api::Gate>(cfg),
        declared::<api::GatePast>(cfg),
        declared::<api::GateState>(cfg),
        declared::<api::GatesSeen>(cfg),
        // Its execution attempts.
        declared::<api::TaskAttempts>(cfg),
        declared::<api::AttemptView>(cfg),
        declared::<api::AttemptControls>(cfg),
        declared::<api::AttemptControl>(cfg),
        declared::<api::AttemptViewAsk>(cfg),
        declared::<api::AttemptLink>(cfg),
        // Its timeline.
        declared::<api::TaskTimeline>(cfg),
        declared::<api::timeline::Offset>(cfg),
        declared::<api::timeline::Stage>(cfg),
        declared::<api::timeline::Claim>(cfg),
        declared::<api::timeline::Round>(cfg),
        declared::<api::timeline::Verdict>(cfg),
        declared::<api::timeline::Run>(cfg),
        declared::<api::timeline::RunResult>(cfg),
        declared::<api::timeline::GateLane>(cfg),
        declared::<api::timeline::GateSpan>(cfg),
        declared::<api::timeline::PlaceSpan>(cfg),
        declared::<api::timeline::History>(cfg),
        declared::<api::timeline::HistorySource>(cfg),
        declared::<api::timeline::HistoryState>(cfg),
        declared::<api::timeline::RoleSpan>(cfg),
        declared::<api::timeline::TimelineMessage>(cfg),
        declared::<api::timeline::MessageEndpoint>(cfg),
        declared::<api::timeline::EndpointState>(cfg),
        declared::<api::timeline::RoleEndpoint>(cfg),
        // Its changed files and a patch opened from that comparison.
        declared::<api::TaskDiffSource>(cfg),
        declared::<api::TaskDiff>(cfg),
        declared::<api::TaskDiffFile>(cfg),
        declared::<api::TaskDiffPatch>(cfg),
    ]
}

/// The project views the cases read routes under: the plain project, one under the one server's
/// base, one with two entrypoints, one at the root, and one with plugin pages.
fn views() -> Vec<api::ProjectView> {
    let nav = |label: &str, href: &str, document: &str| api::NavItem {
        label: label.into(),
        title: label.into(),
        href: href.into(),
        document: document.into(),
    };
    vec![
        api::ProjectView::default(),
        api::ProjectView {
            base: "/alpha".into(),
            ..api::ProjectView::default()
        },
        api::ProjectView {
            entrypoints: vec!["docs".into(), "design".into()],
            ..api::ProjectView::default()
        },
        api::ProjectView {
            entrypoints: vec![String::new()],
            ..api::ProjectView::default()
        },
        api::ProjectView {
            nav: vec![
                nav("▤ shelf", "/shelf", "docs/SHELF.md"),
                nav("◇ maps", "/maps/", "docs/MAPS.md"),
            ],
            ..api::ProjectView::default()
        },
    ]
}

/// apps/symphony-web/src/wire.cases.json, before Biome formats it; why not, when a function
/// [`CARRIED`] names is not found whole.
fn cases() -> Result<Value, String> {
    let categories: Vec<Value> = [
        "docs/ROOT.md",
        "docs/lamps/a.md",
        "docs/a/b/c.md",
        "@proposal~topic/docs/pictures/CARDS.md",
        "@scope",
        "other/a.md",
        "docs/",
        "",
    ]
    .iter()
    .map(|path| json!([path, api::queue_category(path)]))
    .collect();
    let html: Vec<Value> = [
        "a.html",
        "A.HTML",
        "a.Htm",
        "docs/a.htm",
        "a.md",
        "html",
        ".html",
        "docs/.htm",
        "a.html/",
        "a.html.md",
        "a.xhtml",
        "a.html5",
    ]
    .iter()
    .map(|name| json!([name, api::is_html(name)]))
    .collect();
    // Each word with the grain it names, if any, and that grain's default and longest windows.
    let grains: Vec<Value> = ["hour", "day", "week", "month", "", "Hour", "days"]
        .iter()
        .map(|word| {
            let grain = api::Grain::parse(word);
            json!([
                word,
                grain,
                grain.map(api::Grain::default_days),
                grain.map(api::Grain::max_days)
            ])
        })
        .collect();
    let addresses: Vec<Value> = [
        "/docs/a.html",
        "/docs/a.html#top",
        "/docs/a.html?state=b#x",
        "/docs/a.html?raw",
        "/docs/a.html?x=1&raw=1#y",
        "/docs/a.html?rawness=1",
        "/docs/a.html?#x",
        "",
    ]
    .iter()
    .map(|src| json!([src, api::file_address(src)]))
    .collect();
    let verdicts: Vec<Value> = [
        "verdicts/2026-11-11-1244",
        "verdicts/",
        "verdicts",
        "app/verdicts/a",
        "Verdicts/a",
        "",
    ]
    .iter()
    .map(|slug| {
        let task = api::DocumentTask {
            slug: (*slug).into(),
            title: String::new(),
            status: "ready".into(),
        };
        json!([slug, task.answers_verdicts()])
    })
    .collect();
    let mut cases = bases();
    cases["views"] = routes();
    cases["queueCategory"] = categories.into();
    cases["answersVerdicts"] = verdicts.into();
    cases["isHtml"] = html.into();
    cases["slugOrder"] = slug_orders();
    cases["stages"] = stages();
    cases["grains"] = grains.into();
    cases["fileAddress"] = addresses.into();
    cases["webAddress"] = web_addresses();
    cases["sessionControl"] = session_controls();
    cases["elementHtml"] = elements();
    cases["encoded"] = encodings();
    cases["decode"] = decodings();
    cases["samples"] = samples();
    cases["carried"] = carried()?;
    Ok(cases)
}

/// Whether [`api::web_address`] lets a page link each of a set of addresses: http and https in
/// any case with a host, and nothing with a space, a control character or another scheme.
fn web_addresses() -> Value {
    [
        "https://example.com/s/1",
        "http://example.com",
        "HTTPS://Example.com?x#y",
        "HtTp://h/",
        "https://",
        "https:///path",
        "https://?q",
        "https://#f",
        "https://h ost/",
        "https://h/\tx",
        "https://h/\u{7f}",
        "https://h/\u{a0}",
        "https://h/\u{2028}",
        "https://h/\u{85}",
        "https://h/\u{3000}",
        "https://h/\u{feff}",
        "https://h/\u{200b}",
        "https://h/\u{1c}",
        "https://h/\u{9f}",
        "https://h\u{202f}/",
        "https://h:8080/x",
        "javascript:alert(1)",
        "ftp://h/",
        "//h/",
        "http:/h",
        "https:h",
        "",
        "\u{e9}://h",
        "http\u{15b}://h",
        "http\u{17f}://h",
    ]
    .iter()
    .map(|url| json!([url, api::web_address(url)]))
    .collect()
}

/// The Go to session control [`api::SessionGo::control`] gives a page on the server's machine
/// (`local`) and one elsewhere, for a set of ways in, and whether each goes through another
/// session than `amber`'s ([`api::SessionGo::via_parent`]): the control as `{kind: target}`, or
/// none.
fn session_controls() -> Value {
    let go = |handle: &str, url: Option<&str>, pane: Option<&str>| api::SessionGo {
        handle: handle.into(),
        url: url.map(Into::into),
        pane: pane.map(Into::into),
    };
    let ways = [
        go("amber", Some("https://example.com/s"), Some("w1:p2")),
        go("amber", Some("https://example.com/s"), None),
        go("amber", None, Some("w1:p2")),
        go("amber", None, None),
        go("amber", Some("javascript:alert(1)"), Some("w1:p2")),
        go("amber", Some("javascript:alert(1)"), None),
        go("amber", Some(""), Some("")),
        go("birch", Some("http://h"), None),
    ];
    ways.iter()
        .flat_map(|way| {
            [true, false].map(|local| {
                let control = way.control(local).map(|control| match control {
                    api::SessionControl::Focus(pane) => json!({"focus": pane}),
                    api::SessionControl::Link(url) => json!({"link": url}),
                    api::SessionControl::Screen(pane) => json!({"screen": pane}),
                });
                json!([way, local, control, way.via_parent("amber")])
            })
        })
        .collect()
}

/// What [`api::Viewer::element_html`] writes for a file under a set of viewers: their element, the
/// file, its caption and poster, and the markup (none for an element that is no custom element's).
fn elements() -> Value {
    let viewer = |element: &str, attributes: &[(&str, &str)]| api::Viewer {
        element: element.into(),
        script: "/static/orbit.js".into(),
        attributes: attributes
            .iter()
            .map(|(name, value)| ((*name).into(), (*value).into()))
            .collect(),
        ..api::Viewer::default()
    };
    let plain = viewer("orbit-view", &[("controls", ""), ("angle", "-45deg")]);
    let odd = viewer(
        "orbit-view",
        &[
            ("onload x", "1"),
            ("Upper", "1"),
            ("9th", "1"),
            ("data-x", "\"><script>'&"),
        ],
    );
    let mut asked: Vec<(api::Viewer, &str, &str, Option<&str>)> = vec![
        (plain.clone(), "/a.item", "A", Some("/a.svg")),
        (plain.clone(), "/a.item", "A", None),
        (odd, "/a\".item", "<b>", Some("/p&q.png")),
    ];
    for element in [
        "",
        "script",
        "Orbit-view",
        "-view",
        "orbit view",
        "orbit-view>",
        "x-1",
    ] {
        asked.push((viewer(element, &[]), "/a.item", "A", None));
    }
    asked
        .into_iter()
        .map(|(viewer, src, label, poster)| {
            let html = viewer.element_html(src, label, poster);
            json!([viewer, src, label, poster, html])
        })
        .collect()
}

/// What [`api::View::encoded`] makes of a set of views' attributes.
fn encodings() -> Value {
    let pairs: [&[(&str, &str)]; 4] = [
        &[("angle", "0deg 25deg auto"), ("zoom", "a=b")],
        &[("bad name", "x"), ("wrapped", "one\ntwo"), ("cr", "a\rb")],
        &[("Upper", "1"), ("ok", ""), ("x-2", "y")],
        &[],
    ];
    pairs
        .iter()
        .map(|set| {
            let view = api::View {
                label: "Above".into(),
                set: set
                    .iter()
                    .map(|(name, value)| ((*name).into(), (*value).into()))
                    .collect(),
                ..api::View::default()
            };
            json!([view, view.encoded()])
        })
        .collect()
}

/// What [`api::View::decode`] reads out of a view's attribute value, line by line: the pairs.
fn decodings() -> Value {
    [
        "angle=0deg 25deg auto\nzoom=a=b",
        "no pair\n=empty\nBad=x\nok=",
        "a=1\r\nb=2\r\n",
        "a=1\r",
        "x=a\rb\n\n\ny-2=",
        "\n",
        "",
        "a-=1\n9=2\nb c=3",
    ]
    .iter()
    .map(|value| json!([value, api::View::decode(value)]))
    .collect()
}

/// The Rust the web client carries in TypeScript, each function by its file, its name and the
/// TypeScript that carries it. Their source goes into the cases whole ([`carried`]), so a change to
/// one, which the cases' inputs may not show, fails `just client-lint` until the cases are written
/// again: the change is then ported, with cases that show it.
const CARRIED: &[(&str, &str, &str)] = &[
    (API_PROJECT, "here", "here, src/project.ts"),
    (API_PROJECT, "strip", "strip, src/project.ts"),
    (API_PROJECT, "entrypoints", "entrypoints, src/project.ts"),
    (
        API_PROJECT,
        "is_document_route",
        "isDocumentRoute, src/project.ts",
    ),
    (
        API_PROJECT,
        "is_folder_route",
        "isFolderRoute, src/project.ts",
    ),
    (
        API_PROJECT,
        "documents_href",
        "documentsHref, src/project.ts",
    ),
    (API_PROJECT, "readme_route", "readmeRoute, src/project.ts"),
    (API_PROJECT, "own_pages", "ownPages, src/project.ts"),
    (API_PROJECT, "is_page", "isPage, src/project.ts"),
    (API_PROJECT, "is_html", "isHtml, src/project.ts"),
    (API_PROJECT, "extension", "extension, src/project.ts"),
    (
        API_PROJECT,
        "html_extension",
        "htmlExtension, src/project.ts",
    ),
    (API_PROJECT, "within", "within, src/project.ts"),
    (API_PROJECT, "asks_for_file", "asksForFile, src/project.ts"),
    (API_PROJECT, "file_address", "fileAddress, src/project.ts"),
    (API_LIB, "queue_category", "queueCategory, src/queue.tsx"),
    (
        API_LIB,
        "answers_verdicts",
        "answersVerdicts, src/document.ts",
    ),
    (API_TASK_QUERIES, "slug_order", "slugOrder, src/task.ts"),
    (API_TASK_QUERIES, "kind", "isTask, src/task.ts"),
    (API_TASK_QUERIES, "is_task", "isTask, src/task.ts"),
    (API_TASK_QUERIES, "has_status", "taskStage, src/work.ts"),
    (API_MACHINE_WORK, "is_for", "isFor, src/work.ts"),
    (API_TASK_STAGE, "task_stage", "taskStage, src/work.ts"),
    (API_TASK_STAGE, "places_alike", "placesAlike, src/work.ts"),
    (API_TASK_STAGE, "can_place", "canPlace, src/work.ts"),
    (API_TASK_STAGE, "conflicts", "conflicts, src/work.ts"),
    (BLOCKS_MEDIA, "element_html", "elementHtml, src/media.tsx"),
    (BLOCKS_MEDIA, "encoded", "encoded, src/media.tsx"),
    (BLOCKS_MEDIA, "decode", "decode, src/media-picture.tsx"),
    (BLOCKS_MEDIA, "is_name", "isName, src/media-picture.tsx"),
    (BLOCKS_MEDIA, "is_element", "isElement, src/media.tsx"),
    (BLOCKS_MEDIA, "escape", "escapeText, src/media.tsx"),
    (API_REPORT_PAGE, "parse", "parseGrain, src/reports.tsx"),
    (
        API_REPORT_PAGE,
        "default_days",
        "defaultDays, src/reports.tsx",
    ),
    (API_AGENTS, "web_address", "webAddress, src/links.ts"),
    (
        API_AGENTS,
        "control",
        "sessionControl, src/session-control.ts",
    ),
    (
        API_AGENTS,
        "via_parent",
        "viaParent, src/session-control.ts",
    ),
    (API_LINKS, "app_path", "appPath, src/links.ts"),
    (API_LINKS, "linkable", "linkable, src/links.ts"),
    (API_GATES, "count", "countOf, src/gates-model.ts"),
    (API_GATES, "waiting", "waitingGates, src/gates-model.ts"),
    (API_GATES, "seen", "seenOf, src/gates-model.ts"),
    (
        BLOCKS_GATES,
        "unsaid_commit",
        "unsaidCommit, src/gates-model.ts",
    ),
    (BLOCKS_GATES, "signed", "signed, src/gates-model.ts"),
    (BLOCKS_GATES, "waits", "waits, src/gates-model.ts"),
    (
        API_TIMELINE,
        "offset_at",
        "offsetAt, src/timeline-layout.ts",
    ),
    (
        API_TIMELINE,
        "every_seconds",
        "everySeconds, src/timeline-layout.ts",
    ),
    (API_TIMELINE, "end", "timelineEnd, src/timeline-layout.ts"),
    (
        API_TIMELINE,
        "machine_seconds",
        "machineSeconds, src/timeline-layout.ts",
    ),
    (
        API_TIMELINE,
        "queued_seconds",
        "queuedSeconds, src/timeline-layout.ts",
    ),
    (
        API_TIMELINE,
        "seconds",
        "stageSeconds, src/timeline-layout.ts",
    ),
    (API_TIMELINE, "waited", "waited, src/timeline-layout.ts"),
    (API_TIMELINE, "ran", "ran, src/timeline-layout.ts"),
];

/// The files [`CARRIED`] names, by their path in the repository.
const API_PROJECT: &str = "crates/symphony-api/src/project.rs";
const API_LIB: &str = "crates/symphony-api/src/lib.rs";
const API_TASK_QUERIES: &str = "crates/symphony-api/src/task_queries.rs";
const API_MACHINE_WORK: &str = "crates/symphony-api/src/machine_work.rs";
const API_TASK_STAGE: &str = "crates/symphony-api/src/task_stage.rs";
const API_REPORT_PAGE: &str = "crates/symphony-api/src/report_page.rs";
const BLOCKS_MEDIA: &str = "crates/symphony-blocks/src/media.rs";
const API_AGENTS: &str = "crates/symphony-api/src/agents.rs";
const API_LINKS: &str = "crates/symphony-api/src/links.rs";
const API_GATES: &str = "crates/symphony-api/src/gates.rs";
const BLOCKS_GATES: &str = "crates/symphony-blocks/src/gates.rs";
const API_TIMELINE: &str = "crates/symphony-api/src/timeline.rs";

/// Each of [`CARRIED`]: its name, the TypeScript that carries it, and its source, line by line.
fn carried() -> Result<Value, String> {
    CARRIED
        .iter()
        .map(|(file, name, ts)| {
            let text = match *file {
                API_PROJECT => include_str!("../src/project.rs"),
                API_TASK_QUERIES => include_str!("../src/task_queries.rs"),
                API_MACHINE_WORK => include_str!("../src/machine_work.rs"),
                API_TASK_STAGE => include_str!("../src/task_stage.rs"),
                API_REPORT_PAGE => include_str!("../src/report_page.rs"),
                API_AGENTS => include_str!("../src/agents.rs"),
                BLOCKS_MEDIA => include_str!("../../symphony-blocks/src/media.rs"),
                API_LINKS => include_str!("../src/links.rs"),
                API_GATES => include_str!("../src/gates.rs"),
                BLOCKS_GATES => include_str!("../../symphony-blocks/src/gates.rs"),
                API_TIMELINE => include_str!("../src/timeline.rs"),
                _ => include_str!("../src/lib.rs"),
            };
            Ok(json!({
                "rust": format!("{file} {name}"),
                "typescript": ts,
                "source": source(text, name).map_err(|why| format!("{file}: {why}"))?,
            }))
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Value::from)
}

/// The function `name` in `text`, line by line: from its `fn` line to the first line after it that
/// is that line's indentation and `}`, rustfmt's form, which `cargo fmt --check` in `just lint-rs`
/// holds; so no brace inside it, in a string, a character or a comment, ends it early. A function
/// declared on no line or on two, or with no such closing line, stops the example. Its tests run
/// with `just test-rs -p symphony-api --features typescript --example typescript`, which `just
/// client-lint` runs (apps/symphony-web/check.sh).
fn source<'a>(text: &'a str, name: &str) -> Result<Vec<&'a str>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let declared: Vec<usize> = (0..lines.len())
        .filter(|&at| lines.get(at).is_some_and(|line| declares(line, name)))
        .collect();
    let [start] = declared[..] else {
        return Err(format!(
            "its file declares `fn {name}` on {} lines: CARRIED names one",
            declared.len()
        ));
    };
    let first = lines.get(start).copied().unwrap_or_default();
    // An empty body, which rustfmt leaves on the `fn` line.
    if first.trim_end().ends_with("{}") {
        return Ok(vec![first]);
    }
    let indent = first.strip_suffix(first.trim_start()).unwrap_or_default();
    let close = format!("{indent}}}");
    let after = lines.iter().skip(start + 1).position(|line| *line == close);
    let end = after.ok_or_else(|| format!("`fn {name}` has no line `{close}` after it"))?;
    Ok(lines
        .get(start..=start + 1 + end)
        .unwrap_or_default()
        .to_vec())
}

/// Whether `line` declares the function `name`: code, not a comment, naming `fn name(` or
/// `fn name<`.
fn declares(line: &str, name: &str) -> bool {
    !line.trim_start().starts_with("//")
        && ["(", "<"]
            .iter()
            .any(|after| line.contains(&format!("fn {name}{after}")))
}

/// How `slug_order` orders each pair of a set of slugs: -1, 0 or 1, as `a` comes before, with or
/// after `b`. Runs of digits by their number, leading zeros apart, text and bytes past ASCII.
fn slug_orders() -> Value {
    let slugs = [
        "",
        "0",
        "00",
        "01",
        "1",
        "9",
        "10",
        "ready-3",
        "ready-30",
        "ready-03",
        "ready-3a",
        "ready-3-1",
        "a2b9",
        "a2b10",
        "a10b1",
        "ready",
        "ready-",
        "ready-#",
        "ready-a",
        "ready-é",
        "é01",
        "a/2",
        "a/10",
        "A1",
        "a 1",
        "\u{ffff}",
        "\u{10000}",
    ];
    let pairs: Vec<Value> = slugs
        .iter()
        .flat_map(|a| {
            slugs.iter().map(move |b| {
                let order = match api::slug_order(a, b) {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                };
                json!([a, b, order])
            })
        })
        .collect();
    pairs.into()
}

/// An operation of a machine's work: `kind` (`build`, `test`, `command`) and `state` (`queued`,
/// `running`) of the task `task`, as the machine `machine` reports it by the id `id`.
fn operation(machine: &str, id: &str, task: &str, kind: &str, state: &str) -> api::WorkOperation {
    let kind = match kind {
        "build" => api::WorkKind::Build,
        "test" => api::WorkKind::Test,
        _ => api::WorkKind::Command,
    };
    let state = if state == "running" {
        api::WorkState::Running
    } else {
        api::WorkState::Queued
    };
    api::WorkOperation {
        machine_id: (!machine.is_empty()).then(|| machine.into()),
        operation_id: Some(id.into()),
        parent_id: None,
        handle: Some("amber".into()),
        task: Some(task.into()),
        checkout: "/opaque".into(),
        recipe: "fixture".into(),
        kind,
        state,
        resource: "run_slot".into(),
        reason: String::new(),
        phase_since: None,
        under: Vec::new(),
    }
}

/// A machine `id` reporting `operations`, as `freshness` and `complete` say it.
fn machine(
    id: &str,
    freshness: api::WorkFreshness,
    complete: bool,
    operations: Vec<api::WorkOperation>,
) -> api::MachineObservation {
    api::MachineObservation {
        machine_id: id.into(),
        label: id.into(),
        producer_id: "fixture".into(),
        sequence: 1,
        age_ms: Some(0),
        freshness,
        complete,
        reason: String::new(),
        counts: None,
        operations,
    }
}

/// The work of one machine `alpha`, fresh and complete, reporting `operations`.
fn work(operations: Vec<api::WorkOperation>) -> api::MachineWork {
    api::MachineWork {
        freshness: api::WorkFreshness::Fresh,
        machines: vec![machine(
            "alpha",
            api::WorkFreshness::Fresh,
            true,
            operations,
        )],
        ..api::MachineWork::default()
    }
}

/// A task `slug` in `status`, under `parent` when one is given.
fn task(slug: &str, status: &str, parent: Option<&str>) -> api::Task {
    api::Task {
        slug: slug.into(),
        title: "A task".into(),
        body: String::new(),
        status: status.into(),
        priority: 2,
        parent: parent.map(Into::into),
        created_by: "owner".into(),
        created: 1.0,
        updated: 2.0,
        links: vec!["docs/A.md".into()],
        depends: Vec::new(),
        roles: vec![api::Role {
            handle: "amber".into(),
            role: "executor".into(),
            since: 1.0,
            expires: None,
            health: "live".into(),
        }],
        instruction: false,
        fields: std::collections::BTreeMap::new(),
        waits_on: None,
        review_rounds: None,
        from: None,
        workflow: None,
    }
}

/// The works the stage cases place tasks by: each what it shows, by its name.
fn works() -> Vec<(&'static str, api::MachineWork)> {
    let op = operation;
    let mut under = op("alpha", "u", "w/a/1", "build", "running");
    under.under = vec!["w/a".into(), "w".into()];
    let mut no_id = op("alpha", "n", "w/a", "test", "running");
    no_id.operation_id = None;
    let mut empty_id = op("alpha", "e", "w/a", "test", "running");
    empty_id.operation_id = Some(String::new());
    let mut contradicted = work(vec![op("alpha", "c", "w/a", "build", "running")]);
    contradicted.machines.push(machine(
        "beta",
        api::WorkFreshness::Fresh,
        true,
        vec![op("alpha", "c", "w/a", "build", "queued")],
    ));
    let mut stale = work(vec![op("alpha", "s", "w/a", "test", "running")]);
    stale.machines[0].freshness = api::WorkFreshness::Stale;
    let mut partial = work(vec![op("alpha", "p", "w/a", "test", "queued")]);
    partial.machines[0].freshness = api::WorkFreshness::Incomplete;
    partial.machines[0].complete = false;
    let unassigned = api::MachineWork {
        unassigned: vec![op("alpha", "x", "w/a", "build", "running")],
        ..api::MachineWork::default()
    };
    vec![
        ("none", api::MachineWork::default()),
        (
            "queued build",
            work(vec![op("alpha", "b", "w/a", "build", "queued")]),
        ),
        (
            "running build",
            work(vec![op("alpha", "b", "w/a", "build", "running")]),
        ),
        (
            "queued test",
            work(vec![op("alpha", "t", "w/a", "test", "queued")]),
        ),
        (
            "running build, queued test",
            work(vec![
                op("alpha", "b", "w/a", "build", "running"),
                op("alpha", "t", "w/a", "test", "queued"),
            ]),
        ),
        (
            "queued test, running test",
            work(vec![
                op("alpha", "t", "w/a", "test", "queued"),
                op("alpha", "r", "w/a", "test", "running"),
            ]),
        ),
        (
            "another task's",
            work(vec![op("alpha", "o", "w/b", "test", "running")]),
        ),
        (
            "a command",
            work(vec![op("alpha", "c", "w/a", "command", "running")]),
        ),
        (
            "another machine's id",
            work(vec![op("beta", "m", "w/a", "build", "running")]),
        ),
        (
            "no machine id",
            work(vec![op("", "m", "w/a", "build", "running")]),
        ),
        ("no operation id", work(vec![no_id])),
        ("an empty operation id", work(vec![empty_id])),
        ("a subtask's", work(vec![under])),
        ("contradicted", contradicted),
        ("stale", stale),
        ("partial", partial),
        ("unassigned", unassigned),
    ]
}

/// The stage each work puts each task in, connected and not, and whether each two works place
/// alike (`places_alike`), the same works told apart by a heartbeat and by the order of their
/// operations.
fn stages() -> Value {
    let works = works();
    let tasks = [
        task("w/a", "in_progress", None),
        task("w/a", "review", None),
        task("w", "in_progress", None),
        task("w/a/1", "in_progress", Some("w/a")),
        task("w/b", "in_progress", None),
    ];
    let placed: Vec<Value> = works
        .iter()
        .flat_map(|(name, work)| {
            tasks.iter().enumerate().flat_map(move |(at, task)| {
                [true, false].map(|connected| {
                    json!([
                        name,
                        at,
                        connected,
                        work.task_stage(task, connected).as_str()
                    ])
                })
            })
        })
        .collect();
    // Two operations, a running build and a queued test of `w/a`, told apart from this work by
    // what places nothing (a heartbeat, a command) and by their order.
    let both = || {
        work(vec![
            operation("alpha", "b", "w/a", "build", "running"),
            operation("alpha", "t", "w/a", "test", "queued"),
        ])
    };
    let mut beat = both();
    for machine in &mut beat.machines {
        machine.sequence = 9;
        machine.age_ms = Some(900);
    }
    let mut swapped = both();
    for machine in &mut swapped.machines {
        machine.operations.reverse();
    }
    let mut others = both();
    for machine in &mut others.machines {
        machine
            .operations
            .push(operation("alpha", "x", "w/c", "command", "running"));
    }
    let mut compared: Vec<(&str, api::MachineWork)> = works.clone();
    compared.extend([
        ("a heartbeat later", beat),
        ("swapped", swapped),
        ("with a command", others),
    ]);
    let alike: Vec<Value> = compared
        .iter()
        .flat_map(|(a, one)| {
            compared
                .iter()
                .map(move |(b, other)| json!([a, b, one.places_alike(other)]))
        })
        .collect();
    let works: serde_json::Map<String, Value> = compared
        .iter()
        .map(|(name, work)| ((*name).to_string(), json!(work)))
        .collect();
    let tasks: Vec<Value> = tasks.iter().map(|task| json!(task)).collect();
    json!({ "works": works, "tasks": tasks, "placed": placed, "alike": alike })
}

/// `here` and `strip` under no base and one, each for a set of paths.
fn bases() -> Value {
    let bases = ["", "/alpha"];
    let here: Vec<Value> = bases
        .iter()
        .flat_map(|base| {
            let view = api::ProjectView {
                base: (*base).into(),
                ..api::ProjectView::default()
            };
            [
                "/tasks",
                "/",
                "tasks",
                "//cdn.example/x",
                "#top",
                "https://a.example/b",
                "",
            ]
            .map(|path| json!([base, path, view.here(path)]))
        })
        .collect();
    let strip: Vec<Value> = bases
        .iter()
        .flat_map(|base| {
            let view = api::ProjectView {
                base: (*base).into(),
                ..api::ProjectView::default()
            };
            [
                "/alpha",
                "/alpha/",
                "/alpha/queue",
                "/alphabet",
                "/queue",
                "/",
            ]
            .map(|path| json!([base, path, view.strip(path)]))
        })
        .collect();
    json!({ "here": here, "strip": strip })
}

/// Each of [`views`] with what it answers for a set of routes, its own pages and its README.
fn routes() -> Value {
    let paths = [
        "",
        "queue",
        "tasks",
        "task/a/b",
        "docs",
        "docs/",
        "docs/A.md",
        "docs/a/",
        "docs/a",
        "docs/x.png",
        "docs/page.html",
        "docs/A.HTM",
        "docs/a/b.Htm",
        "docs/.html",
        "docs/page.html/",
        "docs/a.html.md",
        "docs/a.xhtml",
        "docs/html",
        "design/c.html",
        "readout.html",
        "tasks/a.html",
        "docs/guide.md/",
        "design/",
        "design/B.md",
        "README.md",
        "notes",
        "shelf",
        "maps/",
        "docs/SHELF.md",
        "taskforce/notes.md",
        "api/x",
        "assets/a.js",
        "artifacts/uploads/a",
    ];
    views()
        .iter()
        .map(|view| {
            let answers: Vec<Value> = paths
                .iter()
                .map(|path| {
                    json!([
                        path,
                        view.is_document_route(path),
                        view.is_folder_route(path)
                    ])
                })
                .collect();
            json!({
                "view": view,
                "routes": answers,
                "ownPages": view.own_pages(),
                "readme": view.readme_route(),
            })
        })
        .collect()
}

/// A value of each read the client checks at run time, as the server writes it: every field the
/// type has, then only those it always sends.
fn samples() -> Value {
    let card = api::Card {
        postponed: true,
        slug: "lamp-colour".into(),
        doc: "@proposal~lamps/docs/lamps/LAMPS.md".into(),
        doc_title: "Lamps".into(),
        title: "Which colour do the lamps take?".into(),
        summary: "Warm or cold.".into(),
        why: "design · gates 2 items".into(),
        posted: Some("2026-11-14 08:32".into()),
        pictures: vec![api::Picture {
            src: Some("/thumb/a.png".into()),
            caption: "A".into(),
            waiting: true,
        }],
        task: Some("app/lamps".into()),
    };
    let bare = api::Card {
        postponed: false,
        slug: "s".into(),
        doc: "docs/DECISIONS.md".into(),
        doc_title: "Decisions".into(),
        title: "T".into(),
        summary: String::new(),
        why: "w".into(),
        posted: None,
        pictures: vec![],
        task: None,
    };
    let item = |branch: Option<&str>| api::QueueItem {
        href: "/docs/A.md#s".into(),
        path: "docs/A.md".into(),
        slug: "s".into(),
        title: "A card".into(),
        kind: "comment".into(),
        text: "Answered.".into(),
        branch: branch.map(Into::into),
        ask: None,
    };
    let asked = |holder: Option<&str>| api::QueueItem {
        ask: Some(api::QueueAsk {
            id: 3,
            asked: 1_000,
            holder: holder.map(Into::into),
            held: holder.map(|_| 1_120),
        }),
        ..item(None)
    };
    let queue = api::Queue {
        waiting: vec![card.clone(), bare.clone()],
        spot_check: vec![item(None)],
        agent: vec![
            item(Some("app/lamps")),
            asked(None),
            asked(Some("umber-stoat")),
        ],
        branches: vec![],
    };
    let board = api::Board {
        version: 7,
        build: "/assets/symphony-shell-0123456789abcdef.js".into(),
        ping_ms: 15_000,
        next: vec![card, bare],
        counts: api::Counts::default(),
        progress: api::Progress::default(),
    };
    let mut samples = serde_json::Map::new();
    samples.insert("queue".into(), json!(queue));
    samples.insert("emptyQueue".into(), json!(api::Queue::default()));
    samples.insert("board".into(), json!(board));
    samples.extend(task_board_samples());
    samples.extend(workflow_samples());
    samples.extend(attempt_samples());
    samples.extend(agents_samples());
    samples.insert("reports".into(), json!(reports()));
    samples.insert("emptyReports".into(), json!(api::ReportsPage::default()));
    samples.insert("landed".into(), json!(landed_page()));
    samples.insert(
        "emptyLanded".into(),
        json!(api::report_page::LandedPage::default()),
    );
    samples.insert("settings".into(), json!(settings()));
    samples.insert("emptySettings".into(), json!(api::SettingsPage::default()));
    samples.extend(people_samples());
    samples.extend(tokens_samples());
    samples.insert("blocks".into(), json!(blocks()));
    samples.extend(
        documents()
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value)),
    );
    Value::Object(samples)
}

/// The People page's reads, as [`samples`] holds them: the person at the machine's, an empty
/// board's, and theirs once they gave themselves a name.
fn people_samples() -> serde_json::Map<String, Value> {
    let mut samples = serde_json::Map::new();
    samples.insert("people".into(), json!(people()));
    samples.insert(
        "emptyPeople".into(),
        json!(api::PeoplePage {
            administers: true,
            machine: true,
            mine: Some(api::MachineProfile::default()),
            ..api::PeoplePage::default()
        }),
    );
    // The person at the machine once they gave themselves a name, on a board with nobody named.
    samples.insert(
        "namedPeople".into(),
        json!(api::PeoplePage {
            administers: true,
            machine: true,
            mine: Some(api::MachineProfile {
                name: Some("stefano".into()),
                display: "Stefano".into(),
                git: vec!["stefano-b".into()],
            }),
            signins: vec!["tailscale".into()],
            ..api::PeoplePage::default()
        }),
    );
    samples
}

/// A task's tokens and the cards' costs, as [`samples`] holds them.
fn tokens_samples() -> serde_json::Map<String, Value> {
    let costs = api::TaskCosts {
        prices: "2026-11-16".into(),
        costs: vec![
            api::TaskCost {
                slug: "w/a".into(),
                dollars: 12.345,
                unpriced: false,
            },
            api::TaskCost {
                slug: "w/b".into(),
                dollars: 0.0,
                unpriced: true,
            },
        ],
    };
    serde_json::Map::from_iter([
        ("taskTokens".into(), json!(task_tokens())),
        ("emptyTaskTokens".into(), json!(api::TaskTokens::default())),
        ("taskCosts".into(), json!(costs)),
    ])
}

/// The task board's reads, as [`samples`] holds them: its listings, the machines' work and the
/// feed's changes of a task.
fn task_board_samples() -> Vec<(String, Value)> {
    let mut idle = task("w/b", "ready", None);
    idle.links.clear();
    idle.roles.clear();
    // A block that names nothing: sent in the stage it stopped in, with what it waits on.
    idle.from = Some("ready".into());
    idle.waits_on = Some(api::WaitsOn {
        kind: "nobody".into(),
        who: String::new(),
        why: "no reason recorded".into(),
        since: 2.0,
    });
    let mut fielded = task("w/a/1", "in_progress", Some("w/a"));
    fielded.fields.insert(
        "game".into(),
        [
            ("effort".to_string(), api::FieldValue::Number(3.0)),
            ("risk".to_string(), api::FieldValue::Text("high".into())),
        ]
        .into(),
    );
    let mut guided = work(vec![operation("alpha", "b", "w/a", "build", "running")]);
    guided.guidance.insert(
        "alpha".into(),
        api::WorkGuidance {
            build: Some(2),
            test: None,
        },
    );
    guided.unassigned = vec![operation("", "x", "w/b", "test", "queued")];
    let listing = api::TaskListing {
        machine_work: guided.clone(),
        tasks: vec![task("w/a", "in_progress", None), fielded, idle],
        statuses: vec!["ready".into(), "in_progress".into()],
        signoff: vec!["w/a".into()],
        counts: vec![("ready".into(), 1), ("in_progress".into(), 2)],
        lifecycle_counts: vec![("ready".into(), 1), ("in_progress".into(), 2)],
        next: Some("w/b".into()),
        orchestrators: vec![api::Orchestrator {
            handle: "sable-finch".into(),
            health: "stale".into(),
        }],
        unstarted: 3,
        waiting: None,
    };
    let empty = api::TaskListing {
        machine_work: api::MachineWork::default(),
        tasks: Vec::new(),
        statuses: Vec::new(),
        signoff: Vec::new(),
        counts: Vec::new(),
        lifecycle_counts: Vec::new(),
        next: None,
        orchestrators: Vec::new(),
        unstarted: 0,
        waiting: None,
    };
    let change = api::TaskChange {
        key: "w/a".into(),
        fields: api::TaskFields {
            status: Some("review".into()),
            priority: Some(1),
            parent: Some(String::new()),
            roles: Some(Vec::new()),
            // Of no kind and empty: each cleared.
            waits_on: Some(api::WaitsOn::default()),
            from: Some(String::new()),
            ..api::TaskFields::default()
        },
    };
    let added = api::ItemChange {
        key: "w/a".into(),
        value: Some(task("w/a", "ready", None)),
        removed: false,
    };
    let removed = api::ItemChange::<api::Task> {
        key: "w/a".into(),
        value: None,
        removed: true,
    };
    let bare = api::TaskChange {
        key: "w/b".into(),
        fields: api::TaskFields::default(),
    };
    [
        ("listing", json!(listing)),
        ("emptyListing", json!(empty)),
        ("work", json!(guided)),
        ("emptyWork", json!(api::MachineWork::default())),
        ("taskChange", json!(change)),
        ("bareTaskChange", json!(bare)),
        ("taskAdded", json!(added)),
        ("taskRemoved", json!(removed)),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect()
}

/// A place of a workflow: `phase`'s `state`.
fn pair(phase: &str, state: &str) -> Value {
    json!({"phase": phase, "state": state})
}

/// A board's custom definition: built, then reviewed twice, a review sending work back to be built
/// again, and a phase off the board.
fn workflow_definition() -> Value {
    let position = |phase: &str, state: &str, staffing: Value, legacy: &str| {
        json!({"at": pair(phase, state), "staffing": staffing, "slot": "release",
            "dependencies": "none", "unsuccessful_dependency": "block", "accepts_work": true,
            "terminal": false, "legacy": legacy, "outcome": null,
            "satisfies_dependencies": false, "completion": "none", "recovery": null})
    };
    let edge = |id: &str, label: &str, from: Value, to: Value, operation: &str, by: Value| {
        json!({"id": id, "label": label, "from": from, "to": to, "operation": operation,
            "alias": null, "authority": [by], "inputs": [], "dependencies": "none",
            "guards": [], "effects": [], "automatic": false, "retry": null, "human_wait": false})
    };
    let executor = json!({"kind": "task_role", "role": "executor"});
    let reviewer = json!({"kind": "task_role", "role": "reviewer"});
    let owner = json!({"kind": "project_capability", "capability": "owner"});
    let phase = |id: &str, label: &str, order: u32, phone: u32, column: bool| json!({"id": id, "label": label, "order": order, "phone_order": phone, "column": column});
    let mut rework = edge(
        "rework",
        "Send back",
        pair("review", "awaiting"),
        pair("build", "working"),
        "changes",
        reviewer.clone(),
    );
    rework["inputs"] = json!(["reason"]);
    // The owner accepts a verified task on the accepted output of each task it depends on.
    let mut accept = edge(
        "accept",
        "Accept",
        pair("verify", "awaiting"),
        pair("verify", "accepted"),
        "close_covered",
        owner.clone(),
    );
    accept["dependencies"] = json!("accepted_output");
    json!({
        "format": 1,
        "initial": pair("build", "available"),
        "task_kinds": ["task"],
        "phases": [
            phase("build", "Build", 0, 2, true),
            phase("review", "Review", 1, 1, true),
            phase("verify", "Verify", 2, 0, true),
            phase("parked", "Parked", 3, 3, false),
        ],
        "states": [
            {"id": "available", "label": "Available"}, {"id": "working", "label": "Working"},
            {"id": "blocked", "label": "Blocked"}, {"id": "awaiting", "label": "Awaiting review"},
            {"id": "accepted", "label": "Accepted"},
        ],
        "positions": [
            position("build", "available", Value::Null, "ready"),
            position("build", "working", json!("executor"), "in_progress"),
            position("build", "blocked", Value::Null, "blocked"),
            position("review", "awaiting", Value::Null, "review"),
            position("verify", "awaiting", Value::Null, "review"),
            position("verify", "accepted", Value::Null, "done"),
            position("parked", "available", Value::Null, "backlog"),
        ],
        "edges": [
            edge("claim", "Claim", pair("build", "available"), pair("build", "working"), "claim", executor.clone()),
            edge("submit", "Submit", pair("build", "working"), pair("review", "awaiting"), "request_review", executor),
            rework,
            edge("pass", "Pass review", pair("review", "awaiting"), pair("verify", "awaiting"), "approve", reviewer),
            accept,
            edge("block", "Block", pair("build", "available"), pair("build", "blocked"), "block", owner),
        ],
        "capabilities": ["owner"],
        "adapters": [],
        "guards": [],
    })
}

/// A custom workflow's reads, as [`samples`] holds them: the board's definition
/// ([`workflow_definition`]) and the read of a board with none, a task's next actions and an
/// edge's result, and a listing that holds its tasks in their phases ([`held_listing`]).
fn workflow_samples() -> Vec<(String, Value)> {
    let definition = workflow_definition();
    let revision = json!({"format": 1, "digest": "d".repeat(64),
        "source": {"commit": "1".repeat(40), "project_blob": "2".repeat(40)}});
    let read: api::workflow::BoardWorkflow = serde_json::from_value(json!({
        "project": "symphony",
        "workflow": {"definition": definition, "revision": revision, "activation_generation": 2},
    }))
    .expect("a board's workflow read");
    let none = api::workflow::BoardWorkflow {
        project: "symphony".into(),
        workflow: None,
    };
    let snapshot = json!({"workflow": revision, "activation_generation": 2, "task_revision": 7,
        "at": pair("review", "awaiting")});
    let next: api::workflow::NextActions = serde_json::from_value(json!({
        "snapshot": snapshot,
        "actions": [
            {"edge": "pass", "operation": "approve", "destination": pair("verify", "awaiting"),
             "label": "Pass review", "responsible": [{"kind": "task_role", "role": "reviewer"}],
             "required_inputs": [], "unavailable": ["unauthorized"]},
            {"edge": "rework", "operation": "changes",
             "destination": pair("build", "working"), "label": "Send back",
             "responsible": [{"kind": "project_capability", "capability": "owner"}],
             "required_inputs": ["reason"], "unavailable": []},
        ],
    }))
    .expect("a task's next actions");
    let mut moved = next.clone();
    moved.snapshot.task_revision = 8;
    moved.snapshot.at = api::workflow::Pair {
        phase: "build".into(),
        state: "working".into(),
    };
    moved.actions.clear();
    let applied = api::workflow::OperationResult {
        transition: "w/a@8".into(),
        next: moved,
        effects: vec![api::workflow::Effect::ClearBlock],
    };
    // What the page reads of a task's actions: the preview, and the outputs an edge takes, none
    // at Review; at Verify, where Accept takes the accepted output of what the task depends on.
    let actions = api::workflow::ActionsRead {
        snapshot: next.snapshot.clone(),
        actions: next.actions.clone(),
        takes: Vec::new(),
    };
    let taking: api::workflow::ActionsRead = serde_json::from_value(json!({
        "snapshot": {"workflow": revision, "activation_generation": 2, "task_revision": 12,
            "at": pair("verify", "awaiting")},
        "actions": [
            {"edge": "accept", "operation": "close_covered",
             "destination": pair("verify", "accepted"), "label": "Accept",
             "responsible": [{"kind": "project_capability", "capability": "owner"}],
             "required_inputs": [], "unavailable": []},
        ],
        "takes": [{"task": "w/a", "output": "w/a@3"}],
    }))
    .expect("a task's actions taking accepted outputs");
    let held = held_listing();
    let (guards, refused) = guard_samples();
    [
        ("workflow", json!(read)),
        ("noWorkflow", json!(none)),
        ("nextActions", json!(actions)),
        ("takingActions", json!(taking)),
        ("applied", json!(applied)),
        ("heldListing", json!(held)),
        ("taskGuards", json!(guards)),
        ("guardRefusal", json!(refused)),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect()
}

/// A task's guards and a guard's refusal, as [`workflow_samples`] holds them: at review/awaiting,
/// which the task entered by an edge `design signoff` guards, met at a generation its authority
/// has moved on from, with Pass review held by `release signoff`, whose authority this host asks
/// and whose ledger has a page; and the ledger's refusal of that guard, with its two reasons.
fn guard_samples() -> (api::workflow::TaskGuards, api::workflow::GuardRefusal) {
    use api::workflow::{
        Attested, Attestee, GuardAuthority, GuardEntry, GuardMet, GuardNeed, GuardRecord,
        GuardRefusal, GuardStanding, GuardWhy, GuardedEdge, Pair, TaskGuards,
    };
    let record = |result, facts: &[&str], reasons: &[&str], generation| GuardRecord {
        result,
        facts: facts.iter().map(|fact| (*fact).to_owned()).collect(),
        reasons: reasons.iter().map(|reason| (*reason).to_owned()).collect(),
        epoch: "2026".into(),
        generation,
        issued_at: 1_795_000_000.0,
        expires_at: 1_795_000_600.0,
        task_revision: 7,
        at: Pair {
            phase: "review".into(),
            state: "awaiting".into(),
        },
        input_revisions: [("design".to_owned(), "r4".to_owned())].into(),
        principal: Attestee {
            kind: "human".into(),
            issuer: "local".into(),
            subject: "owner".into(),
        },
    };
    let authority = |adapter: &str, generation, asked_here| GuardAuthority {
        adapter: adapter.into(),
        issuer: "ledger".into(),
        epoch: Some("2026".into()),
        generation: Some(generation),
        conflicted: false,
        admitted: 3,
        asked_here,
    };
    let guards = TaskGuards {
        authorities: vec![
            authority("design", 12, false),
            authority("release", 41, true),
        ],
        guards: vec![
            GuardNeed {
                guard: "design_signoff".into(),
                adapter: "design".into(),
                page: None,
            },
            GuardNeed {
                guard: "release_signoff".into(),
                adapter: "release".into(),
                page: Some("/docs/LEDGER.md#w/a:release_signoff".into()),
            },
        ],
        edges: vec![GuardedEdge {
            edge: "pass".into(),
            guards: vec!["release_signoff".into()],
        }],
        entry: Some(GuardEntry {
            edge: "submit".into(),
            label: "Submit".into(),
            revision: 7,
            principal: Attestee {
                kind: "agent".into(),
                issuer: "board".into(),
                subject: "amber-wren".into(),
            },
            at: 1_794_990_000.0,
            guards: vec![GuardMet {
                guard: "design_signoff".into(),
                adapter: "design".into(),
                issuer: "ledger".into(),
                epoch: "2026".into(),
                generation: 11,
                standing: GuardStanding::DecidedAgain,
                record: Some(record(
                    Attested::Pass,
                    &["the design was approved /docs/DECISIONS.md#design-signoff"],
                    &[],
                    11,
                )),
            }],
        }),
    };
    let refused = GuardRefusal {
        guard: "release_signoff".into(),
        adapter: "release".into(),
        why: GuardWhy::Refused,
        record: Some(record(
            Attested::Refuse,
            &[],
            &[
                "a blocker is still open /task/w/a",
                "the release check failed https://ci.example/runs/7",
            ],
            41,
        )),
    };
    (guards, refused)
}

/// A listing that holds its tasks in their phases: one built, one awaiting review after its
/// reviewer was lost, and one blocked, resuming where it was available.
fn held_listing() -> api::TaskListing {
    let place = |phase: &str, state: &str, revision: u64| api::workflow::TaskPlace {
        phase: phase.into(),
        state: state.into(),
        resume: None,
        revision,
        manual: None,
    };
    let mut building = task("w/a", "in_progress", None);
    building.workflow = Some(place("build", "working", 4));
    let mut reviewed = task("w/b", "review", None);
    reviewed.workflow = Some(api::workflow::TaskPlace {
        manual: Some(api::workflow::ManualRecovery {
            trigger: "lost_holder".into(),
            holder: "sable-finch".into(),
            role: "reviewer".into(),
        }),
        ..place("review", "awaiting", 9)
    });
    let mut stopped = task("w/c", "blocked", None);
    stopped.workflow = Some(api::workflow::TaskPlace {
        resume: Some(api::workflow::Pair {
            phase: "build".into(),
            state: "available".into(),
        }),
        ..place("build", "blocked", 2)
    });
    let column = |status: &str, ordinary: usize, blocked: usize| api::WaitingColumn {
        status: status.into(),
        ordinary,
        blocked,
        oldest: Some(1.0),
    };
    api::TaskListing {
        machine_work: api::MachineWork::default(),
        tasks: vec![building, reviewed, stopped],
        statuses: vec!["in_progress".into(), "review".into(), "blocked".into()],
        signoff: Vec::new(),
        counts: vec![("phase:build".into(), 2), ("phase:review".into(), 1)],
        lifecycle_counts: vec![
            ("blocked".into(), 1),
            ("in_progress".into(), 1),
            ("review".into(), 1),
        ],
        next: None,
        orchestrators: Vec::new(),
        unstarted: 0,
        waiting: Some(api::WaitingBoard {
            kinds: vec![("nobody".into(), 1), ("working".into(), 2)],
            columns: vec![column("phase:build", 1, 1), column("phase:review", 1, 0)],
            states: vec![
                ("awaiting".into(), 1),
                ("blocked".into(), 1),
                ("working".into(), 1),
            ],
            workflow: Some(2),
        }),
    }
}

/// The agents page's reads, as [`samples`] holds them: its page with every field and with none it
/// can leave empty, an agent the feed changes, one with nothing known of it, the probes, a focus
/// and a screen.
fn agents_samples() -> Vec<(String, Value)> {
    let agent = sampled_agent();
    let bare = bare_agent();
    let probes = api::AgentProbes {
        panes: "ok".into(),
        slots: "unavailable".into(),
    };
    let page = api::AgentPage {
        view: api::Agents {
            machine_work: work(vec![operation("alpha", "b", "w/a", "build", "running")]),
            generated: 3.0,
            probes: probes.clone(),
            agents: vec![agent.clone(), bare.clone()],
        },
        tasks: vec![task("w/a", "in_progress", None)],
        total: 2,
        next: Some("birch".into()),
        local: true,
    };
    let empty = api::AgentPage {
        view: api::Agents {
            machine_work: api::MachineWork::default(),
            generated: 0.0,
            probes: probes.clone(),
            agents: Vec::new(),
        },
        tasks: Vec::new(),
        total: 0,
        next: None,
        local: false,
    };
    [
        ("agentPage", json!(page)),
        ("emptyAgentPage", json!(empty)),
        (
            "agentChange",
            json!(api::ItemChange {
                key: "amber".into(),
                value: Some(agent),
                removed: false,
            }),
        ),
        ("bareAgent", json!(bare)),
        ("agentProbes", json!(probes)),
        (
            "agentFocus",
            json!(api::AgentFocus {
                said: "Shown in the Herdr app".into(),
            }),
        ),
        (
            "agentScreen",
            json!(api::AgentScreen {
                handle: "amber".into(),
                pane: "w1:p2".into(),
                text: "$ just test-rs".into(),
                read_at: 4.0,
            }),
        ),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect()
}

/// An agent with every field the agents page reads set: a subagent of `birch` holding `w/a`,
/// running a test, gone to through its parent's session.
fn sampled_agent() -> api::Agent {
    api::Agent {
        handle: "amber".into(),
        kind: "worker".into(),
        model: "a model".into(),
        parent: Some("birch".into()),
        task: Some("w/a".into()),
        role: Some("executor".into()),
        tasks: vec![api::HeldTask {
            slug: "w/a".into(),
            role: "executor".into(),
            status: "in_progress".into(),
        }],
        following: 2,
        worktree: "/tmp/w/a".into(),
        branch: "w/a".into(),
        agent: Some("claude".into()),
        session_id: Some("s1".into()),
        pane: Some("w1:p2".into()),
        callback: Some("unix:/tmp/amber.sock".into()),
        chat: true,
        state: "working".into(),
        state_detail: Some(api::AgentRun {
            recipe: "test-rs".into(),
            phase: "running".into(),
            since: Some(1.0),
        }),
        health: "live".into(),
        last_tool_call: Some(2.0),
        last_message: Some(api::AgentSaid {
            slug: "w/a".into(),
            ts: 2.0,
            text: "Built.".into(),
        }),
        slots: Some(api::AgentSlots { open: 3, cap: 8 }),
        session_url: Some("https://example.com/s".into()),
        in_process: true,
        go: Some(api::SessionGo {
            handle: "birch".into(),
            url: Some("https://example.com/s".into()),
            pane: Some("w1:p2".into()),
        }),
        tokens: Some(api::AgentTokens {
            input: 12,
            cache_write_5m: 3_400,
            cache_write_1h: 560,
            cache_read: 78_900,
            output: 1_200,
            calls: 7,
        }),
        cost: Some(api::AgentCost {
            dollars: 0.061_308,
            input: 0.000_048,
            cache_write_5m: 0.017,
            cache_write_1h: 0.004_48,
            cache_read: 0.015_78,
            output: 0.024,
            unpriced: vec![api::AgentUnpriced {
                model: "gpt-x".into(),
                speed: "standard".into(),
                tokens: api::AgentTokens {
                    input: 5,
                    cache_write_5m: 0,
                    cache_write_1h: 0,
                    cache_read: 0,
                    output: 1,
                    calls: 1,
                },
            }],
            prices: "2026-11-16".into(),
        }),
    }
}

/// An agent with nothing known of it but its handle and its health.
fn bare_agent() -> api::Agent {
    api::Agent {
        handle: "birch".into(),
        kind: String::new(),
        model: String::new(),
        parent: None,
        task: None,
        role: None,
        tasks: Vec::new(),
        following: 0,
        worktree: String::new(),
        branch: String::new(),
        agent: None,
        session_id: None,
        pane: None,
        callback: None,
        chat: false,
        state: String::new(),
        state_detail: None,
        health: "lost".into(),
        last_tool_call: None,
        last_message: None,
        slots: None,
        session_url: None,
        in_process: false,
        go: None,
        tokens: None,
        cost: None,
    }
}

/// A value of each read a document's or a folder's page checks, as the server writes it: every
/// field, then only those always sent.
fn documents() -> Vec<(&'static str, Value)> {
    use document_sample as part;
    let change = part::change();
    vec![
        ("outline", json!(part::outline())),
        ("bareOutline", json!(part::bare_outline())),
        (
            "sections",
            json!(api::DocumentSections {
                version: 13,
                change: change.clone(),
            }),
        ),
        ("change", json!(change)),
        (
            "bareChange",
            json!(api::DocumentChange {
                path: "docs/A.md".into(),
                ..api::DocumentChange::default()
            }),
        ),
        ("folder", json!(part::folder(Some(part::document())))),
        ("bareFolder", json!(part::folder(None))),
        (
            "versions",
            json!({"versions": [api::Version {
                sha: "0123abc".into(),
                date: "2026-11-14".into(),
                subject: "Lamps".into(),
            }]}),
        ),
        (
            "diff",
            json!(api::DocumentDiff {
                sha: "0123abc".into(),
                subject: "Lamps".into(),
                diff: "-a\n+b".into(),
            }),
        ),
        (
            "asks",
            json!({"asks": [part::ask()], "listening": true, "recipient": "amber-heron"}),
        ),
        (
            "noAsks",
            json!({"asks": [], "listening": false, "recipient": null}),
        ),
    ]
}

/// The parts of the documents' samples.
mod document_sample {
    use std::collections::BTreeMap;

    use symphony_api::{self as api, Block, Span};

    fn paragraph(words: &str) -> Block {
        Block::Paragraph {
            spans: vec![Span::Text { text: words.into() }],
        }
    }

    fn fragment(words: &str) -> api::Fragment {
        api::Fragment {
            blocks: vec![paragraph(words)],
            html: String::new(),
            media: Vec::new(),
        }
    }

    fn section(
        key: &str,
        part: Option<usize>,
        comparison: Option<api::SectionComparison>,
    ) -> api::Section {
        api::Section {
            start: 3,
            end: 9,
            key: key.into(),
            hash: "h1".into(),
            part,
            blocks: vec![paragraph("Body")],
            html: String::new(),
            media: Vec::new(),
            comparison,
            ends: Some(["paragraph".into(), "paragraph".into()]),
        }
    }

    fn changed() -> api::SectionComparison {
        api::SectionComparison {
            kind: api::Difference::Changed,
            before: vec![paragraph("Before")],
        }
    }

    fn card() -> api::DocumentCard {
        api::DocumentCard {
            slug: "lamp-colour".into(),
            state: "pending".into(),
            text: "Which colour?".into(),
            group: "decide".into(),
            open: true,
        }
    }

    fn crumb(href: Option<&str>) -> api::Crumb {
        api::Crumb {
            name: "docs".into(),
            href: href.map(Into::into),
        }
    }

    fn task() -> api::DocumentTask {
        api::DocumentTask {
            slug: "app/lamps".into(),
            title: "Lamps".into(),
            status: "in_progress".into(),
        }
    }

    /// A document with every field: a heading, a changed section, a card's section and its
    /// thread, a task and a comparison.
    pub(super) fn document() -> api::Document {
        api::Document {
            version: 12,
            path: "docs/lamps/LAMPS.md".into(),
            title: "Lamps".into(),
            waiting: vec!["lamp-colour".into()],
            crumbs: vec![crumb(Some("/docs/")), crumb(None)],
            headings: vec![api::Heading {
                level: 2,
                anchor: "colour".into(),
                text: "Colour".into(),
            }],
            sections: vec![
                section("colour", None, Some(changed())),
                section("card:lamp-colour", Some(1), None),
            ],
            cards: vec![card()],
            threads: BTreeMap::from([("lamp-colour".to_string(), fragment("Agreed."))]),
            tasks: vec![task()],
            comparison: Some(api::DocumentComparison {
                task: "design/lamps".into(),
            }),
        }
    }

    pub(super) fn outline() -> api::DocumentOutline {
        api::DocumentOutline {
            document: document(),
            anchors: BTreeMap::from([("colour".to_string(), vec!["colour".to_string()])]),
            thread_hashes: BTreeMap::from([("lamp-colour".to_string(), "t1".to_string())]),
        }
    }

    /// An outline of a document holding nothing but what is always sent.
    pub(super) fn bare_outline() -> api::DocumentOutline {
        api::DocumentOutline {
            document: api::Document {
                version: 0,
                path: "docs/A.md".into(),
                title: "A".into(),
                waiting: vec![],
                crumbs: vec![],
                headings: vec![],
                sections: vec![],
                cards: vec![],
                threads: BTreeMap::new(),
                tasks: vec![],
                comparison: None,
            },
            anchors: BTreeMap::new(),
            thread_hashes: BTreeMap::new(),
        }
    }

    /// A change with every field: a section and a thread written and removed, a card changed
    /// and one removed, and every part of the document's own written.
    pub(super) fn change() -> api::DocumentChange {
        api::DocumentChange {
            path: "docs/lamps/LAMPS.md".into(),
            removed: true,
            sections: vec![section("colour", Some(0), Some(changed()))],
            deleted: vec!["old".into()],
            positions: vec![api::SectionPosition {
                key: "colour".into(),
                start: 4,
                end: 10,
                part: None,
            }],
            order: Some(vec!["colour".into(), "card:lamp-colour".into()]),
            threads: BTreeMap::from([
                ("lamp-colour".to_string(), Some(fragment("Agreed."))),
                ("lamp-size".to_string(), None),
            ]),
            title: Some("Lamps".into()),
            waiting: Some(vec![]),
            crumbs: Some(vec![crumb(None)]),
            headings: Some(vec![]),
            cards: vec![
                api::ItemChange {
                    key: "lamp-colour".into(),
                    value: Some(card()),
                    removed: false,
                },
                api::ItemChange {
                    key: "lamp-size".into(),
                    value: None,
                    removed: true,
                },
            ],
            tasks: Some(vec![task()]),
            comparison: Some(vec![]),
        }
    }

    fn entry(kind: &str) -> api::FolderEntry {
        api::FolderEntry {
            kind: kind.into(),
            path: "docs/lamps".into(),
            title: "Lamps".into(),
            docs: 3,
            waiting: 1,
            summary: "The lamps.".into(),
        }
    }

    pub(super) fn folder(intro: Option<api::Document>) -> api::Folder {
        api::Folder {
            path: "docs".into(),
            title: "Docs".into(),
            crumbs: vec![crumb(None)],
            docs: 4,
            intro,
            entries: vec![entry("folder"), entry("doc")],
        }
    }

    /// An ask answered, with a message holding blocks and one holding none.
    pub(super) fn ask() -> api::Ask {
        let message = api::AskMessage {
            seq: 1,
            kind: "question".into(),
            by: "owner".into(),
            text: "Why?".into(),
            at: 1.5,
            blocks: vec![paragraph("Why?")],
            html: String::new(),
            media: Vec::new(),
        };
        api::Ask {
            id: 7,
            path: "docs/lamps/LAMPS.md".into(),
            anchor: "colour".into(),
            quote: "warm".into(),
            question: "Why?".into(),
            asked: 1.0,
            updated: 2.5,
            answers: vec![api::AskAnswer {
                text: "Because.".into(),
                by: "agent".into(),
                at: 2.0,
            }],
            messages: vec![
                message.clone(),
                api::AskMessage {
                    blocks: vec![],
                    ..message
                },
            ],
            status: "open".into(),
        }
    }
}

/// A source of the Settings sample: where `said` says a value stands.
fn settings_source(
    scope: Option<api::SettingScope>,
    said: &str,
    file: Option<(&str, u32)>,
) -> api::SettingSource {
    api::SettingSource {
        scope,
        said: said.into(),
        file: file.map(|(name, _)| name.to_string()),
        line: file.map(|(_, line)| line),
        over: None,
    }
}

/// A setting of the Settings sample: a preference of the project's, for its caller to write over.
fn sample_setting(
    key: &str,
    name: &str,
    kind: api::SettingKind,
    value: &str,
    source: api::SettingSource,
) -> api::Setting {
    api::Setting {
        key: key.into(),
        name: name.into(),
        kind,
        value: value.into(),
        choices: Vec::new(),
        source,
        scopes: vec![api::SettingScope::Project],
        class: api::SettingClass::Preference,
    }
}

fn settings_entry(id: &str, name: &str, about: &str) -> api::SettingsEntry {
    api::SettingsEntry {
        id: id.into(),
        name: name.into(),
        about: about.into(),
        state: None,
        needs: Vec::new(),
        settings: Vec::new(),
        source: None,
    }
}

fn settings_section(
    id: &str,
    title: &str,
    entries: Vec<api::SettingsEntry>,
) -> api::SettingsSection {
    api::SettingsSection {
        id: id.into(),
        title: title.into(),
        entries,
    }
}

/// An entry with a state, a need met and one not, and a choice the project file writes.
fn settings_herdr() -> api::SettingsEntry {
    let need = |what: &str, met, said: &str| api::EntryNeed {
        what: what.into(),
        met,
        said: said.into(),
    };
    api::SettingsEntry {
        state: Some(api::EntryState {
            word: "On".into(),
            on: Some(true),
            why: "Agents' terminals open in Herdr.".into(),
        }),
        needs: vec![
            need("the hc program", true, "Found on this machine: /bin/hc."),
            need(
                "a token",
                false,
                "Not found on this machine: <nothing> opens.",
            ),
        ],
        settings: vec![api::Setting {
            choices: vec!["hc".into(), "none".into()],
            ..sample_setting(
                "[agents] panes",
                "What agents' terminals run in",
                api::SettingKind::Choice,
                "hc",
                settings_source(
                    Some(api::SettingScope::Project),
                    "symphony.toml [agents] panes, line 10",
                    Some(("symphony.toml", 10)),
                ),
            )
        }],
        ..settings_entry("herdr", "Herdr panes", "Agents' terminals in Herdr.")
    }
}

/// An entry with a setting a local file writes over the committed one, and one at its default.
fn settings_models() -> api::SettingsEntry {
    use api::SettingScope::{Board, Machine, Project};
    api::SettingsEntry {
        settings: vec![
            api::Setting {
                scopes: vec![Project, Board, Machine],
                ..sample_setting(
                    "[agents.models] executor",
                    "The executor's model",
                    api::SettingKind::Text,
                    "fable",
                    api::SettingSource {
                        over: Some("\"opus\" (the board's store [agents.models] executor)".into()),
                        ..settings_source(
                            Some(Machine),
                            "symphony.local.toml [agents.models] executor, line 2",
                            Some(("symphony.local.toml", 2)),
                        )
                    },
                )
            },
            api::Setting {
                scopes: vec![Project, Board, Machine],
                ..sample_setting(
                    "[limits] machine_agents",
                    "Agents this machine runs",
                    api::SettingKind::Number,
                    "no limit",
                    settings_source(None, "default: no limit", None),
                )
            },
            api::Setting {
                scopes: vec![Project, Board, Machine],
                ..sample_setting(
                    "[limits] low_volume",
                    "Open slots below which an orchestrator hears low volume",
                    api::SettingKind::Number,
                    "3",
                    api::SettingSource {
                        over: Some("4 (symphony.toml [limits] low_volume, line 12)".into()),
                        ..settings_source(
                            Some(Board),
                            "the board's store [limits] low_volume",
                            None,
                        )
                    },
                )
            },
        ],
        ..settings_entry(
            "models",
            "The model each role runs on",
            "What a launch passes.",
        )
    }
}

/// A reviewed sentence with its file and line, and an entry that is a sentence with no setting.
fn settings_reviewed() -> Vec<api::SettingsEntry> {
    vec![
        api::SettingsEntry {
            settings: vec![api::Setting {
                class: api::SettingClass::Reviewed,
                ..sample_setting(
                    "[land]",
                    "What a land does",
                    api::SettingKind::Sentence,
                    "No check; pushes main; no generator; merged here.",
                    settings_source(
                        Some(api::SettingScope::Project),
                        "symphony.toml [land], line 36",
                        Some(("symphony.toml", 36)),
                    ),
                )
            }],
            ..settings_entry("landing", "Landing", "What a land runs.")
        },
        api::SettingsEntry {
            settings: vec![api::Setting {
                class: api::SettingClass::Reviewed,
                ..sample_setting(
                    "[workflow]",
                    "The workflow tasks move by",
                    api::SettingKind::Sentence,
                    "The board's own, built-in v1.",
                    settings_source(None, "default: no [workflow] written", None),
                )
            }],
            ..settings_entry(
                "workflow",
                "Workflow",
                "The stages a task moves through and what each move requires.",
            )
        },
    ]
}

/// An entry a plugin describes (the describers seam) as a sentence alone, with where it is said
/// and no setting: the one shape that carries a source of its own.
fn settings_described() -> api::SettingsEntry {
    api::SettingsEntry {
        state: Some(api::EntryState {
            word: "Read".into(),
            on: None,
            why: "Twelve lamps listed.".into(),
        }),
        source: Some(settings_source(
            Some(api::SettingScope::Project),
            "catalogue.toml, line 1",
            Some(("catalogue.toml", 1)),
        )),
        ..settings_entry(
            "plugin-shop-catalogue",
            "Catalogue",
            "Described by the plugin shop.",
        )
    }
}

/// The device's entry: a value no file writes.
fn settings_appearance() -> api::SettingsEntry {
    api::SettingsEntry {
        settings: vec![api::Setting {
            scopes: vec![api::SettingScope::Device],
            ..sample_setting(
                "appearance",
                "The theme of this project's pages here",
                api::SettingKind::Text,
                "kept in this browser",
                settings_source(Some(api::SettingScope::Device), "this browser", None),
            )
        }],
        ..settings_entry("appearance", "Appearance", "A theme of this device's own.")
    }
}

/// What the board's store holds for the sample's settings: an entry in force with its record and
/// its commit waiting for delivery, none for another, and one the local file stands over.
fn settings_stored() -> Vec<api::StoredSetting> {
    let change = |before: Option<&str>, after: Option<&str>, at: &str| api::StoredChange {
        before: before.map(str::to_string),
        after: after.map(str::to_string),
        at: at.into(),
        by: "owner".into(),
    };
    vec![
        api::StoredSetting {
            setting: "[limits] low_volume".into(),
            key: "limits.low_volume".into(),
            value: Some("3".into()),
            held: true,
            refused: None,
            changes: vec![
                change(None, Some("2"), "2026-11-22 20:10"),
                change(Some("2"), Some("3"), "2026-11-22 20:15"),
            ],
            waiting: Some("docs/owner-save-4: waiting for its pull request".into()),
        },
        api::StoredSetting {
            setting: "[limits] machine_agents".into(),
            key: "limits.machine_agents".into(),
            ..api::StoredSetting::default()
        },
        api::StoredSetting {
            setting: "[agents.models] executor".into(),
            key: "agents.models.executor".into(),
            value: Some("opus".into()),
            held: false,
            refused: None,
            changes: vec![change(None, Some("opus"), "2026-11-22 20:12")],
            waiting: None,
        },
    ]
}

/// The People page as the person at the machine reads it: Ana, given speak and deciding by the
/// group staff, signing in by two services, reached by mail and Slack, her cards by Slack and her
/// verdicts by mail; Bo, who has no profile, given administer; Cy, a reader who set nothing for
/// notices; the group staff; and the log of every change that made them.
fn people() -> api::PeoplePage {
    let words = |powers: &[&str]| powers.iter().map(|power| (*power).to_string()).collect();
    let person = |name: &str, display: &str, powers: &[&str], own: &[&str], groups: &[&str]| {
        api::PersonRow {
            name: name.into(),
            display: display.into(),
            powers: words(powers),
            own: words(own),
            groups: words(groups),
            ..api::PersonRow::default()
        }
    };
    let line =
        |n: i32, did: &str, name: &str, group: Option<&str>, powers: &[&str]| api::PeopleLogLine {
            n: n.into(),
            at: 1_763_000_000.0 + 3600.0 * f64::from(n),
            by: "owner".into(),
            did: did.into(),
            name: name.into(),
            group: group.map(Into::into),
            powers: words(powers),
        };
    let notices = |ways: [Option<&str>; 3]| {
        let kinds = ["card", "question", "verdict"];
        let ways = kinds
            .into_iter()
            .zip(ways)
            .map(|(kind, way)| api::NoticeWay {
                kind: kind.into(),
                way: way.map(Into::into),
            });
        Some(ways.collect())
    };
    api::PeoplePage {
        people: vec![
            api::PersonRow {
                signins: words(&["tailscale", "github"]),
                git: words(&["ana-lima"]),
                profile: true,
                addresses: Some(api::Addresses {
                    email: Some("ana@example.com".into()),
                    slack: Some("U024BE7LH".into()),
                    whatsapp: None,
                }),
                notices: notices([Some("slack"), None, Some("email")]),
                ..person(
                    "ana",
                    "Ana Lima",
                    &["read", "speak", "decide"],
                    &["speak"],
                    &["staff"],
                )
            },
            person("bo", "bo", &["read", "administer"], &["administer"], &[]),
            api::PersonRow {
                signins: words(&["tailscale"]),
                profile: true,
                addresses: Some(api::Addresses::default()),
                notices: notices([None, None, None]),
                ..person("cy", "Cy Okafor", &["read"], &["read"], &[])
            },
        ],
        groups: vec![api::GroupRow {
            name: "staff".into(),
            powers: words(&["decide"]),
        }],
        log: vec![
            line(1, "group-add", "staff", None, &["decide"]),
            line(2, "add", "ana", None, &["speak"]),
            line(3, "join", "ana", Some("staff"), &[]),
            line(4, "add", "bo", None, &["administer"]),
            line(5, "add", "cy", None, &["read"]),
        ],
        revision: 5,
        administers: true,
        machine: true,
        you: None,
        problem: None,
        mine: Some(api::MachineProfile::default()),
        signins: words(&["tailscale"]),
    }
}

/// A Settings page of every shape an entry takes: one with a state, a need met and one not, a
/// plugin's that is a sentence with where it is said and no setting, a setting a local file writes
/// over the committed one and over the store, one the store holds, reviewed sentences, the
/// device's, and a problem said; with what the store holds for the settings the page writes.
fn settings() -> api::SettingsPage {
    let file = |name: &str, scope, found| api::SettingsFile {
        name: name.into(),
        scope,
        path: format!("/repo/{name}"),
        found,
    };
    api::SettingsPage {
        files: vec![
            file("symphony.toml", api::SettingScope::Project, true),
            file("symphony.local.toml", api::SettingScope::Machine, false),
        ],
        problems: vec!["symphony.local.toml: [land] check (line 5) is not read".into()],
        sections: vec![
            settings_section(
                "integrations",
                "Integrations and tools",
                vec![settings_herdr(), settings_described()],
            ),
            settings_section("models", "Models and limits", vec![settings_models()]),
            settings_section("reviewed", "Reviewed settings", settings_reviewed()),
            settings_section("device", "This device", vec![settings_appearance()]),
        ],
        stored: settings_stored(),
    }
}

/// A window of one period every source recorded whole, with every status, wait, role and gauge it
/// can have and none at zero, a settings change and a file not read: each member the Reports page
/// reads a period by is written in it.
fn reports() -> api::ReportsPage {
    const MONDAY: f64 = 1_794_182_400.0;
    api::ReportsPage {
        grain: api::Grain::Day,
        days: 1,
        from: MONDAY,
        to: MONDAY + 86_400.0,
        buckets: vec![report_bucket(MONDAY)],
        first: Some(MONDAY),
        marks: vec![api::report_page::ReportMark {
            at: MONDAY + 3_600.0,
            commit: "4f1c0a9b2d3e".into(),
            subject: "settings: land without the check".into(),
            changed: vec!["settings".into()],
        }],
        unread: vec!["/repo/.git/symphony/reports/2026-11-08.json: not read".into()],
        prices: "2026-11-16".into(),
    }
}

/// A task's tokens as its page reads them: two roles, two rounds and a model with no price.
fn task_tokens() -> api::TaskTokens {
    let (executor, reviewer) = (spend(2.0, true), spend(1.0, false));
    let mut total = executor.tokens;
    let tokens = reviewer.tokens;
    total.input += tokens.input;
    total.cache_write_5m += tokens.cache_write_5m;
    total.cache_write_1h += tokens.cache_write_1h;
    total.cache_read += tokens.cache_read;
    total.output += tokens.output;
    total.calls += tokens.calls;
    let cost = api::AgentCost {
        dollars: 3.0,
        input: 0.02,
        cache_write_5m: 0.5,
        cache_write_1h: 0.1,
        cache_read: 0.9,
        output: 1.48,
        ..executor.cost.clone()
    };
    api::TaskTokens {
        total,
        cost,
        by_role: vec![
            api::RoleTokens {
                role: "executor".into(),
                tokens: executor.tokens,
                cost: executor.cost.clone(),
            },
            api::RoleTokens {
                role: "reviewer".into(),
                tokens: reviewer.tokens,
                cost: reviewer.cost.clone(),
            },
        ],
        by_round: vec![
            api::RoundTokens {
                round: 1,
                tokens: executor.tokens,
                cost: executor.cost.clone(),
            },
            api::RoundTokens {
                round: 2,
                tokens: reviewer.tokens,
                cost: reviewer.cost.clone(),
            },
        ],
        rows: vec![
            token_row("executor", 1, executor.clone()),
            token_row("reviewer", 2, reviewer),
        ],
    }
}

/// A row of a task's tokens: `spend` in `role` and `round`.
fn token_row(role: &str, round: u32, spend: api::report_page::Spend) -> api::TokenRow {
    api::TokenRow {
        handle: format!("{role}-1"),
        slug: "w/a".into(),
        role: role.into(),
        model: "claude-opus-5-5".into(),
        speed: "standard".into(),
        round,
        tokens: spend.tokens,
        cost: spend.cost,
        first: 1000.0 * f64::from(round),
        last: 1000.0 * f64::from(round) + 600.0,
    }
}

/// Tokens worth `dollars` at API list prices, with tokens of a model the table has no price for
/// where `unpriced`.
fn spend(dollars: f64, unpriced: bool) -> api::report_page::Spend {
    let tokens = api::AgentTokens {
        input: 10,
        cache_write_5m: 2_000,
        cache_write_1h: 300,
        cache_read: 90_000,
        output: 1_500,
        calls: 4,
    };
    api::report_page::Spend {
        tokens,
        cost: api::AgentCost {
            dollars,
            input: 0.01,
            cache_write_5m: 0.25,
            cache_write_1h: 0.05,
            cache_read: 0.45,
            output: dollars - 0.76,
            unpriced: if unpriced {
                vec![api::AgentUnpriced {
                    model: "gpt-x".into(),
                    speed: "standard".into(),
                    tokens,
                }]
            } else {
                Vec::new()
            },
            prices: "2026-11-16".into(),
        },
    }
}

/// The one day of [`reports`], from `start`, every source recorded whole.
fn report_bucket(start: f64) -> api::ReportBucket {
    use api::agent_activity::{Gauge, Level, RoleLevel};
    use api::report_page::{BucketAgents, BucketFlow, BucketHealth, BucketVerdicts, BucketWaits};
    use api::reports::{DecisionCounts, GitCounts, Spread};
    let spread = Some(Spread {
        n: 2,
        median: 60.0,
        p90: 90.0,
    });
    let role = |role: &str| RoleLevel {
        role: role.into(),
        peak: 2,
        mean: 1.5,
    };
    api::ReportBucket {
        start,
        end: start + 86_400.0,
        rolled: 24,
        flow: Some(BucketFlow {
            held: every_status(1),
            entered: every_status(2),
            done: Some(3),
            task_hours: every_status(1.5),
            unclaimed_hours: 0.5,
            bounces: 1,
            landed: 2,
            reds_filed: 1,
            reds_closed: 1,
        }),
        decisions: Some(DecisionCounts {
            open: 4,
            answered: 3,
            auto_approved: 2,
            withdrawn: 1,
        }),
        git: Some(GitCounts {
            commits: 12,
            merges: 3,
            added: 1200,
            removed: 300,
            reverts: 1,
        }),
        waits: Some(BucketWaits {
            review_to_reviewer: spread,
            changes_to_resumed: spread,
            changes_to_review: spread,
            approved_to_landed: spread,
        }),
        lead: spread,
        verdicts: Some(BucketVerdicts {
            changes: 1,
            approved: 2,
        }),
        red_fix: spread,
        agents: Some(BucketAgents {
            observed_s: 86_400.0,
            samples: 1440,
            running: vec![role("orchestrator"), role("executor"), role("reviewer")],
            running_agents: Level { peak: 3, mean: 2.5 },
            started: 4,
            went_stale: 1,
            went_lost: 1,
            load_1m: Some(Gauge {
                peak: 6.5,
                mean: 3.25,
            }),
            ..BucketAgents::default()
        }),
        health: Some(BucketHealth {
            covered_s: 86_400.0,
            dispatch: api::HourPasses { ok: 10, failed: 1 },
            sweep: api::HourPasses { ok: 10, failed: 2 },
            requests: 100,
            slow: 3,
            unanswered: 1,
            ..BucketHealth::default()
        }),
        spend: Some(vec![
            role_spend("executor", spend(2.0, false)),
            role_spend("reviewer", spend(1.0, true)),
        ]),
        workflow: Some(api::reports::PlacedHours {
            phases: vec![
                spent("build", "Build", 30.5),
                spent("verify", "Verify", 6.0),
            ],
            states: vec![
                spent("working", "Working", 30.5),
                spent("awaiting", "Awaiting", 6.0),
            ],
        }),
        landed: Some(api::report_page::BucketLanded {
            tasks: 3,
            median: Some(1.5),
            p90: Some(2.8),
            lined_cost: 2.0,
            lines: 400,
            unread: 1,
        }),
    }
}

/// The landed tasks of [`reports`]' window, every member the tab reads written: one of each kind
/// and of none, a task priced in part, one with no lines read and one of no counted line.
fn landed_page() -> api::report_page::LandedPage {
    use api::report_page::{KindRow, LandedFigures, LandedPage, LandedRow, ReviewRow};
    const MONDAY: f64 = 1_794_182_400.0;
    let row =
        |slug: &str, kind: Option<&str>, first: f64, later: f64, lines: Option<u64>| LandedRow {
            slug: slug.into(),
            at: MONDAY + 7_200.0,
            kind: kind.map(str::to_string),
            first,
            later,
            tokens: 120_000,
            unpriced: false,
            reviews: u32::from(later > 0.0),
            lines,
        };
    LandedPage {
        grain: api::Grain::Day,
        days: 1,
        from: MONDAY,
        to: MONDAY + 86_400.0,
        tasks: vec![
            LandedRow {
                unpriced: true,
                reviews: 2,
                ..row("app/charts", Some("feature"), 2.0, 1.5, Some(400))
            },
            row("docs/guide", Some("docs"), 1.0, 0.0, None),
            row("generated/outputs", None, 0.5, 0.0, Some(0)),
        ],
        without: 1,
        figures: LandedFigures {
            tasks: 3,
            median: Some(1.0),
            mean: Some(5.0 / 3.0),
            p90: Some(3.0),
            cost: 5.0,
            later: 1.5,
            back: 1,
            lined: 1,
            lined_cost: 3.5,
            lines: 400,
            unlined: 1,
            unread: 1,
        },
        kinds: vec![
            KindRow {
                kind: Some("feature".into()),
                tasks: 1,
                median: Some(3.5),
                cost: 3.5,
                lined_cost: 3.5,
                lines: 400,
            },
            KindRow {
                kind: Some("docs".into()),
                tasks: 1,
                median: Some(1.0),
                cost: 1.0,
                lined_cost: 0.0,
                lines: 0,
            },
            KindRow {
                kind: None,
                tasks: 1,
                median: Some(0.5),
                cost: 0.5,
                lined_cost: 0.0,
                lines: 0,
            },
        ],
        reviews: vec![
            ReviewRow {
                reviews: 0,
                tasks: 2,
                first: 0.75,
                later: 0.0,
                median: Some(0.75),
            },
            ReviewRow {
                reviews: 2,
                tasks: 1,
                first: 2.0,
                later: 1.5,
                median: Some(3.5),
            },
        ],
        kinds_from: Some(MONDAY + 3_600.0),
        prices: "2026-11-16".into(),
        unread: vec!["/repo/.git/symphony/reports/2026-11-08.json: not read".into()],
    }
}

/// `n` for every status, as [`report_bucket`]'s counts by status are.
fn every_status<T: Copy>(n: T) -> api::reports::ByStatus<T> {
    api::reports::ByStatus {
        backlog: n,
        blocked: n,
        cancelled: n,
        concept: n,
        design: n,
        done: n,
        in_progress: n,
        ready: n,
        review: n,
    }
}

/// The task-hours spent in the phase or state `id`, named `label`.
fn spent(id: &str, label: &str, hours: f64) -> api::reports::Spent {
    api::reports::Spent {
        id: id.into(),
        label: label.into(),
        hours,
    }
}

/// `role`'s tokens in a period and their estimated cost, `spend`'s.
fn role_spend(role: &str, spend: api::report_page::Spend) -> api::report_page::RoleSpend {
    let api::report_page::Spend { tokens, cost } = spend;
    api::report_page::RoleSpend {
        role: role.into(),
        tokens,
        cost,
    }
}

/// Blocks holding every kind of block, span, media, container, inline tag, vector shape and
/// figure size, as a plugin or a document renders them; the client's unit tests read them all and
/// draw each, and hold that no kind its types know is missing here.
fn blocks() -> Vec<api::Block> {
    let mut blocks = sample::prose();
    blocks.extend(sample::cards());
    blocks.extend(sample::media());
    blocks.extend(sample::wrappers());
    blocks
}

/// The parts of the blocks' sample, each holding some of the kinds.
mod sample {
    use std::collections::BTreeMap;

    use serde_json::json;
    use symphony_api::{
        self as api, Block, CalloutKind, CardGroup, CardState, ContainerKind, FigureSize, Image,
        InlineKind, Span, ThreadAuthor, VectorKind,
    };

    fn text(text: &str) -> Span {
        Span::Text { text: text.into() }
    }

    fn words(words: &str) -> Vec<Span> {
        vec![text(words)]
    }

    fn paragraph(words: &str) -> Block {
        Block::Paragraph {
            spans: vec![text(words)],
        }
    }

    fn pairs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).into(), (*value).into()))
            .collect()
    }

    /// The style every presentation of the sample holds, which a client writes as given: a value
    /// marked `!important` and a declaration after a comment holding a `;`, which a client that
    /// sets a style property by property would lose.
    const STYLE: &str = "outline: 1px dashed rgb(176, 0, 32) !important; /* a; b */ \
                         background-color: rgba(0, 96, 192, 0.1)";

    fn presentation(class: &str, attributes: &[(&str, &str)]) -> api::Presentation {
        let mut attributes = pairs(attributes);
        attributes.insert("style".into(), STYLE.into());
        api::Presentation {
            class: class.into(),
            attributes,
            ..api::Presentation::default()
        }
    }

    fn picture(src: &str, alt: &str) -> Image {
        let sizes = [
            ("width", "640"),
            ("height", "480"),
            ("data-full", "/full.png"),
        ];
        Image {
            src: src.into(),
            alt: alt.into(),
            presentation: presentation("", &sizes),
        }
    }

    fn lumen() -> Image {
        picture("/k.png", "Lumen")
    }

    fn figure(
        picture: Option<Image>,
        href: Option<&str>,
        badge: Option<&str>,
        size: FigureSize,
    ) -> Block {
        Block::Figure {
            picture,
            caption: words("NOR-L01 Lumen"),
            href: href.map(Into::into),
            badge: badge.map(Into::into),
            size,
        }
    }

    fn entry(who: ThreadAuthor, verdict: Option<&str>, words: &str) -> api::ThreadEntry {
        api::ThreadEntry {
            who,
            timestamp: Some("2026-11-14 08:32".into()),
            verdict: verdict.map(Into::into),
            blocks: vec![paragraph(words)],
        }
    }

    fn target(anchor: Option<&str>) -> api::LinkTarget {
        api::LinkTarget {
            path: "docs/lamps/north.md".into(),
            anchor: anchor.map(Into::into),
        }
    }

    /// One shape of each kind, the last with an attribute no shape may carry.
    fn shapes() -> Vec<api::VectorShape> {
        let shape = |kind, attributes: &[(&str, &str)]| api::VectorShape {
            kind,
            attributes: pairs(attributes),
        };
        vec![
            shape(VectorKind::Path, &[("d", "M4 12h16")]),
            shape(
                VectorKind::Circle,
                &[("cx", "12"), ("cy", "12"), ("r", "4")],
            ),
            shape(
                VectorKind::Ellipse,
                &[("cx", "12"), ("cy", "12"), ("rx", "6"), ("ry", "3")],
            ),
            shape(
                VectorKind::Rect,
                &[("x", "2"), ("y", "2"), ("width", "8"), ("height", "8")],
            ),
            shape(
                VectorKind::Line,
                &[("x1", "0"), ("y1", "0"), ("x2", "24"), ("y2", "24")],
            ),
            shape(VectorKind::Polyline, &[("points", "0,0 12,6 24,0")]),
            shape(
                VectorKind::Polygon,
                &[("points", "0,24 12,12 24,24"), ("onclick", "x()")],
            ),
        ]
    }

    /// One span of each kind, and a styled span of each inline tag.
    fn spans() -> Vec<Span> {
        let link = Span::Link {
            href: "/docs/lamps/north.md#nor-l01".into(),
            target: Some(target(Some("nor-l01"))),
            title: "North".into(),
            spans: words("NOR-L01"),
        };
        let mut spans = vec![
            text("Text "),
            Span::Emphasis {
                spans: words("emphasis"),
            },
            Span::Strong {
                spans: words("strong"),
            },
            Span::Code {
                text: "code".into(),
            },
            link.clone(),
            Span::WikiLink {
                href: "/docs/lamps/north.md".into(),
                target: Some(target(None)),
                spans: words("North"),
            },
            Span::LineBreak,
            Span::Image {
                image: picture("/k.png", "Lumen"),
            },
            Span::Media {
                media: api::Media::Mockup {
                    src: "/docs/ui/hud.html".into(),
                    label: "HUD".into(),
                },
            },
            Span::Component {
                kind: "lamp".into(),
                blocks: vec![paragraph("NOR-L01")],
            },
            Span::Icon {
                shape: shapes(),
                label: "Lamp".into(),
                named: true,
                tip: "A lamp".into(),
            },
            Span::Styled {
                tag: InlineKind::Span,
                presentation: presentation("tasklink", &[("data-task", "app/lamps")]),
                spans: vec![link],
            },
            Span::Graphic {
                view_box: "0 0 24 24".into(),
                presentation: presentation("chart", &[("role", "img")]),
                shapes: shapes(),
            },
            Span::Blocks {
                blocks: vec![figure(Some(lumen()), None, None, FigureSize::Cell)],
            },
            Span::Unknown {
                tag: "blink".into(),
                text: "old".into(),
            },
        ];
        spans.extend(
            [
                InlineKind::Span,
                InlineKind::Small,
                InlineKind::Sub,
                InlineKind::Sup,
                InlineKind::Time,
                InlineKind::Mark,
                InlineKind::Kbd,
                InlineKind::Samp,
                InlineKind::Delete,
                InlineKind::Insert,
                InlineKind::Underline,
                InlineKind::Abbreviation,
            ]
            .map(|tag| {
                let word = json!(tag).as_str().unwrap_or_default().to_string();
                Span::Styled {
                    tag,
                    presentation: presentation(&word, &[]),
                    spans: words(&word),
                }
            }),
        );
        spans
    }

    /// A heading, a paragraph of every span, lists, a table, code, a quote and a rule.
    pub(super) fn prose() -> Vec<Block> {
        vec![
            Block::Heading {
                level: 2,
                anchor: "lamps".into(),
                spans: words("Lamps"),
            },
            Block::Paragraph { spans: spans() },
            Block::List {
                ordered: true,
                start: 3,
                items: vec![vec![paragraph("third")], vec![paragraph("fourth")]],
            },
            Block::List {
                ordered: false,
                start: 1,
                items: vec![vec![paragraph("one")]],
            },
            Block::Table {
                head: vec![api::Cell {
                    header: true,
                    spans: words("Lamp"),
                    presentation: presentation("", &[("scope", "col")]),
                }],
                rows: vec![vec![api::Cell {
                    header: false,
                    spans: words("NOR-L01"),
                    presentation: api::Presentation::default(),
                }]],
                presentation: presentation("lamps", &[]),
            },
            Block::Code {
                language: "rust".into(),
                text: "fn main() {}".into(),
            },
            Block::Quote {
                blocks: vec![paragraph("Quoted")],
            },
            Block::Rule,
        ]
    }

    /// Callouts open and resolved, and cards pending and decided.
    pub(super) fn cards() -> Vec<Block> {
        vec![
            Block::Callout {
                kind: CalloutKind::Question,
                title: vec![
                    Span::Styled {
                        tag: InlineKind::Span,
                        presentation: presentation("callout-kind", &[]),
                        spans: words("question"),
                    },
                    text("Warm or cold?"),
                ],
                resolved: false,
                blocks: vec![paragraph("Which one?")],
            },
            Block::Callout {
                kind: CalloutKind::Resolved,
                title: words("Settled"),
                resolved: true,
                blocks: vec![paragraph("Warm.")],
            },
            Block::Card {
                slug: "lamp-colour".into(),
                mark: CardState::Pending,
                title: "Which colour do the lamps take?".into(),
                body: vec![paragraph("Warm or cold.")],
                verdict: api::Verdict {
                    group: CardGroup::Decide,
                    open: true,
                    posted: Some("2026-11-14 08:32".into()),
                    decided: None,
                },
                thread: vec![
                    entry(ThreadAuthor::Agent, None, "Asked."),
                    entry(ThreadAuthor::Owner, Some("approved"), "Warm."),
                ],
                line: Some(12),
                line_offset: None,
                comparison: vec![Block::Comparison {
                    figures: vec![figure(
                        Some(picture("/a.png", "A")),
                        None,
                        None,
                        FigureSize::Slot,
                    )],
                }],
            },
            Block::Card {
                slug: "lamp-size".into(),
                mark: CardState::Approved,
                title: "How large?".into(),
                body: Vec::new(),
                verdict: api::Verdict {
                    group: CardGroup::Resolved,
                    open: false,
                    posted: None,
                    decided: Some("2026-11-14".into()),
                },
                thread: Vec::new(),
                line: None,
                line_offset: Some(4),
                comparison: Vec::new(),
            },
            Block::Component {
                kind: "lamp".into(),
                blocks: vec![paragraph("A lamp")],
            },
        ]
    }

    /// Each kind of media, a picture, and a figure of each size.
    pub(super) fn media() -> Vec<Block> {
        let viewer = api::Viewer {
            element: "orbit-view".into(),
            script: "/static/orbit.js".into(),
            attributes: vec![("controls".into(), String::new())],
            views: vec![api::View {
                label: "Above".into(),
                title: Some("From above".into()),
                set: vec![("angle".into(), "0deg 25deg".into())],
                call: Some("settle".into()),
            }],
            download: Some(".item".into()),
            open_label: Some("Look".into()),
            close_label: None,
        };
        let item = |name: &str, src: &str, poster: Option<&str>| api::GalleryItem {
            name: name.into(),
            src: src.into(),
            poster: poster.map(Into::into),
        };
        let media = |media| Block::Media { media };
        vec![
            media(api::Media::Mockup {
                src: "/docs/ui/hud.html?state=b#x".into(),
                label: String::new(),
            }),
            media(api::Media::Viewer {
                src: "/models/a.item".into(),
                label: "Model".into(),
                poster: Some("/models/a.png".into()),
                viewer: viewer.clone(),
            }),
            media(api::Media::Gallery {
                items: vec![
                    item("FIRST", "/first.item", Some("/first.png")),
                    item("SECOND", "/second.item", None),
                ],
                viewer,
            }),
            Block::Image {
                image: picture("/k.png", "Lumen"),
            },
            figure(
                Some(lumen()),
                Some("/docs/lamps/north.md#nor-l01"),
                None,
                FigureSize::Small,
            ),
            figure(None, None, Some("coming"), FigureSize::Row),
            figure(None, None, None, FigureSize::Large),
            figure(Some(lumen()), None, None, FigureSize::Tile),
            figure(
                Some(lumen()),
                Some("/docs/lamps/north.md"),
                None,
                FigureSize::Wide,
            ),
            figure(None, None, Some("coming"), FigureSize::Cell),
        ]
    }

    /// Blocks that hold blocks (two threads beside each other among them), a comment, markup the
    /// server does not know, and a container of each tag.
    pub(super) fn wrappers() -> Vec<Block> {
        let mut blocks = vec![
            Block::Caption {
                blocks: vec![paragraph("A caption")],
            },
            Block::Group {
                blocks: vec![paragraph("Grouped")],
            },
            Block::Details {
                summary: words("More"),
                open: true,
                blocks: vec![paragraph("Detail")],
            },
            Block::SettledCard {
                summary: words("lamp-size · approved"),
                open: false,
                blocks: vec![paragraph("Settled")],
            },
            Block::Thread {
                entries: vec![entry(ThreadAuthor::Unknown, None, "Who?")],
            },
            Block::Thread {
                entries: vec![entry(ThreadAuthor::Owner, None, "Still waiting")],
            },
            Block::Comment {
                text: "a note".into(),
            },
            Block::Unknown {
                tag: "marquee".into(),
                text: "Scrolling".into(),
            },
        ];
        blocks.extend(
            [
                ContainerKind::Div,
                ContainerKind::Section,
                ContainerKind::Figure,
                ContainerKind::Figcaption,
                ContainerKind::Aside,
                ContainerKind::Header,
                ContainerKind::Footer,
                ContainerKind::Address,
                ContainerKind::Center,
            ]
            .map(|tag| Block::Container {
                tag,
                presentation: presentation("box", &[("data-kind", "lamp")]),
                blocks: vec![paragraph("Inside")],
            }),
        );
        blocks
    }
}

#[cfg(test)]
mod tests {
    use super::source;

    #[test]
    fn timeline_history_declarations_keep_optional_history_and_explicit_unknown_states() {
        let declarations = super::declarations();
        for (name, fields) in [
            ("TaskTimeline", &["history?: History"][..]),
            (
                "History",
                &[
                    "sources: Array<HistorySource>",
                    "sources_complete: boolean",
                    "roles: Array<RoleSpan>",
                    "messages: Array<TimelineMessage>",
                ][..],
            ),
            (
                "HistorySource",
                &[
                    "state: HistoryState",
                    "first_id: number | null",
                    "before: number | null",
                ][..],
            ),
            (
                "TimelineMessage",
                &[
                    "recipient: string | null",
                    "from: MessageEndpoint",
                    "to: MessageEndpoint",
                    "after_close: boolean | null",
                ][..],
            ),
        ] {
            let declared = declarations
                .iter()
                .find(|declared| declared.name == name)
                .unwrap();
            for field in fields {
                assert!(
                    declared.decl.contains(field),
                    "{name} missing {field}: {}",
                    declared.decl
                );
            }
        }
        let types = super::types();
        assert!(types.contains("\"complete\" | \"partial\" | \"unavailable\""));
        assert!(
            types.contains("\"known\" | \"ambiguous\" | \"everyone\" | \"outside\" | \"unknown\"")
        );
    }

    #[test]
    fn task_diff_declarations_keep_source_words_and_nullable_fields() {
        let types = super::types();
        for declaration in [
            r#"export const TASK_DIFF_PATH = "/api/task-diff";"#,
            r#"export const TASK_DIFF_FILE_PATH = "/api/task-diff-file";"#,
            r#"export type TaskDiffSource = "branch" | "landing" | "unavailable";"#,
        ] {
            assert!(
                types.contains(declaration),
                "missing declaration: {declaration}"
            );
        }
        let declarations = super::declarations();
        for (name, fields) in [
            (
                "TaskDiff",
                &[
                    "source: TaskDiffSource,",
                    "base: string | null,",
                    "head: string | null,",
                    "files: Array<TaskDiffFile>,",
                    "truncated: boolean,",
                    "reason: string | null,",
                ][..],
            ),
            (
                "TaskDiffFile",
                &[
                    "path: string,",
                    "added: number | null,",
                    "removed: number | null,",
                ][..],
            ),
            (
                "TaskDiffPatch",
                &[
                    "base: string,",
                    "head: string,",
                    "path: string,",
                    "patch: string,",
                    "truncated: boolean,",
                ][..],
            ),
        ] {
            let declaration = &declarations
                .iter()
                .find(|one| one.name == name)
                .unwrap()
                .decl;
            for field in fields {
                assert!(declaration.contains(field), "{name} is missing {field}");
            }
        }
    }

    /// Functions as rustfmt writes them, with braces no closing line holds: in a character, a
    /// comment, a string and a format argument; and a doc comment that names one of them.
    const TEXT: &str = "\
pub fn empty() {}

/// Says `fn brace(`, which declares nothing.
pub fn brace(value: char) -> bool {
    // A comment's } closes nothing.
    let closing = '}';
    if value == closing {
        return true;
    }
    format!(\"{value}}}\") == \"}\"
}

impl Wire {
    fn method(&self) -> char {
        '}'
    }
}

fn twice() {
    let _ = 1;
}

fn twice() {
    let _ = 2;
}

fn open() {
    let _ = 1;";

    #[test]
    fn a_function_runs_from_its_fn_line_to_the_line_at_its_indentation_that_closes_it() {
        assert_eq!(
            source(TEXT, "brace"),
            Ok(vec![
                "pub fn brace(value: char) -> bool {",
                "    // A comment's } closes nothing.",
                "    let closing = '}';",
                "    if value == closing {",
                "        return true;",
                "    }",
                "    format!(\"{value}}}\") == \"}\"",
                "}",
            ])
        );
        assert_eq!(
            source(TEXT, "method"),
            Ok(vec![
                "    fn method(&self) -> char {",
                "        '}'",
                "    }"
            ])
        );
        assert_eq!(source(TEXT, "empty"), Ok(vec!["pub fn empty() {}"]));
    }

    #[test]
    fn a_function_declared_on_no_line_or_two_or_never_closed_stops_the_example() {
        assert_eq!(
            source(TEXT, "missing"),
            Err("its file declares `fn missing` on 0 lines: CARRIED names one".into())
        );
        assert_eq!(
            source(TEXT, "twice"),
            Err("its file declares `fn twice` on 2 lines: CARRIED names one".into())
        );
        assert_eq!(
            source(TEXT, "open"),
            Err("`fn open` has no line `}` after it".into())
        );
    }
}

/// A task's execution attempts, as [`samples`] holds them: a history of an attempt paused and
/// followed by its resume, which runs and may be stopped, cancelled or viewed, and one of a
/// provider this server does not hold; and a view's link.
fn attempt_samples() -> Vec<(String, Value)> {
    let attempt = |id: &str, revision: i64, dispatch: &str| -> api::AttemptView {
        serde_json::from_value(json!({
            "id": id.repeat(32), "created": 1_763_500_000_000_i64 + revision, "revision": revision,
            "current": false, "lane": "executor/amber", "role": "executor", "handle": "amber",
            "configuration": "cloud", "dispatch": dispatch, "cancellation": "none",
            "collection": "not_started", "cleanup": "not_due", "initiator": "service host/launcher",
        }))
        .expect("an attempt")
    };
    let mut paused = attempt("a", 6, "terminal");
    paused.ending = Some("stopped".into());
    paused.resource = Some("res-1".into());
    paused.generation = Some("g1".into());
    paused.conversation = Some("conversation-1".into());
    paused.successor = Some("b".repeat(32));
    paused.controls.view = true;
    let mut resumed = attempt("b", 4, "running");
    resumed.current = true;
    resumed.predecessor = Some("a".repeat(32));
    resumed.relation = Some("resume".into());
    resumed.resource = Some("res-2".into());
    resumed.generation = Some("g1".into());
    resumed.initiator = "human local/owner".into();
    resumed.controls = api::AttemptControls {
        cancel: true,
        stop: vec!["pause".into(), "terminate".into()],
        resume: Vec::new(),
        view: true,
        unserved: None,
    };
    let mut elsewhere = attempt("c", 2, "paused");
    elsewhere.lane = "reviewer/coral".into();
    elsewhere.role = "reviewer".into();
    elsewhere.handle = None;
    elsewhere.configuration = "lab".into();
    elsewhere.current = true;
    elsewhere.resource = Some("lab-7".into());
    elsewhere.conversation = Some("conversation-7".into());
    elsewhere.controls.cancel = true;
    elsewhere.controls.unserved =
        Some("No resume or view here: its provider lab is not one this server holds".into());
    let history = api::TaskAttempts {
        attempts: vec![paused, resumed, elsewhere],
    };
    let link = api::AttemptLink {
        url: "https://provider.invalid/res-2".into(),
        valid_for_ms: 10_000,
    };
    [("attempts", json!(history)), ("attemptLink", json!(link))]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
}
