//! The tokens a beat hands the board ([`symphony_board::usage`]): added in the beat's own
//! transaction to the handle bound to its key, on the task and role that handle works then and the
//! task's round, and read back by task and by session; and a board of schema 18 gaining the table
//! with its rows kept.

use crate::common::{Fixture, NOW, Seed};
use serde_json::json;
use symphony_board::rows::UsageRow;
use symphony_board::usage::{Cost, Prices, Spent, Tally, Tokens};
use symphony_board::{Beat, Schema};

/// `n` calls' worth on Opus at the standard speed: `n` of each kind, `n * 1000` read from the cache.
fn opus(n: u64) -> Spent {
    at("claude-opus-5-5", "standard", n)
}

fn at(model: &str, speed: &str, n: u64) -> Spent {
    Spent {
        model: model.into(),
        speed: speed.into(),
        tokens: tokens(n),
    }
}

fn tokens(n: u64) -> Tokens {
    Tokens {
        input: n,
        cache_write_5m: n,
        cache_write_1h: n,
        cache_read: n * 1000,
        output: n,
        calls: n,
    }
}

/// A tool call of agent key `agent`, handing `spent`: the handle it beat for.
fn beat(f: &Fixture, agent: &str, spent: &[Spent]) -> Option<String> {
    f.advance(1.0);
    f.store
        .agent_beat_spending(agent, &Beat::default(), None, None, spent)
        .unwrap()
}

/// `sim/jump` in progress: `ochre-owl` orchestrates, `amber-heron` executes it through key `exec`,
/// `brisk-finch` is the reviewer to be, beating through key `rev`.
fn working() -> Fixture {
    let f = Fixture::new(
        Seed::default()
            .session("ochre-owl")
            .session("amber-heron")
            .session("brisk-finch")
            .task("sim/jump", "in_progress")
            .role("sim/jump", "ochre-owl", "orchestrator")
            .role("sim/jump", "amber-heron", "executor"),
    );
    assert!(f.store.bind_agent("exec", "amber-heron", false).unwrap());
    assert!(f.store.bind_agent("rev", "brisk-finch", false).unwrap());
    f
}

fn rounds(tally: &Tally) -> Vec<(i64, Tokens)> {
    (tally.rounds.iter())
        .map(|(round, spend)| (*round, spend.tokens))
        .collect()
}

fn roles(tally: &Tally) -> Vec<(String, Tokens)> {
    (tally.roles.iter())
        .map(|(role, spend)| (role.clone(), spend.tokens))
        .collect()
}

fn sum(parts: &[u64]) -> Tokens {
    let mut all = Tokens::default();
    for n in parts {
        all.add(&tokens(*n));
    }
    all
}

#[test]
fn a_task_worked_over_two_rounds_reads_each_rounds_cost_back() {
    let f = working();
    assert_eq!(beat(&f, "exec", &[opus(1)]).as_deref(), Some("amber-heron"));
    f.store
        .move_to("sim/jump", "review", "amber-heron")
        .unwrap();
    f.store
        .take_role("sim/jump", "brisk-finch", "reviewer")
        .unwrap();
    // The review of the first round's work is the work after the first move to review: round 2.
    beat(&f, "rev", &[opus(2)]);
    f.store
        .verdict("sim/jump", "brisk-finch", "changes", "the jump overshoots")
        .unwrap();
    beat(&f, "exec", &[opus(4)]);
    f.store
        .move_to("sim/jump", "review", "amber-heron")
        .unwrap();
    beat(&f, "rev", &[opus(8)]);
    let rows = f.store.spent_on(&["sim/jump"]).unwrap();
    let tally = Tally::of(&rows);
    assert_eq!(
        rounds(&tally),
        [(1, sum(&[1])), (2, sum(&[2, 4])), (3, sum(&[8]))]
    );
    assert_eq!(
        roles(&tally),
        [
            ("executor".to_string(), sum(&[1, 4])),
            ("reviewer".to_string(), sum(&[2, 8]))
        ]
    );
    assert_eq!(tally.total.tokens, sum(&[1, 2, 4, 8]));
    let first = rows
        .iter()
        .find(|r| (r.handle.as_str(), r.round) == ("amber-heron", 1))
        .unwrap();
    assert_eq!(
        first,
        &UsageRow {
            handle: "amber-heron".into(),
            slug: "sim/jump".into(),
            role: "executor".into(),
            model: "claude-opus-5-5".into(),
            speed: "standard".into(),
            round: 1,
            input: 1,
            cache_write_5m: 1,
            cache_write_1h: 1,
            cache_read: 1000,
            output: 1,
            calls: 1,
            first: NOW + 1.0,
            last: NOW + 1.0,
        }
    );
}

#[test]
fn beats_of_one_round_add_up_on_one_row_per_model_and_speed() {
    let f = working();
    beat(&f, "exec", &[opus(1), at("claude-opus-5-5", "fast", 3)]);
    beat(&f, "exec", &[opus(2)]);
    beat(&f, "exec", &[]);
    let rows = f.store.spent_on(&["sim/jump"]).unwrap();
    let kept: Vec<(&str, Tokens, f64, f64)> = rows
        .iter()
        .map(|r| (r.speed.as_str(), r.tokens(), r.first, r.last))
        .collect();
    assert_eq!(
        kept,
        [
            ("fast", tokens(3), NOW + 1.0, NOW + 1.0),
            ("standard", sum(&[1, 2]), NOW + 1.0, NOW + 2.0)
        ]
    );
}

#[test]
fn a_session_working_no_task_spends_on_none_in_its_kind_and_an_unbound_key_on_nobody() {
    let f = working();
    // Orchestrating is no task's work: its calls go to no task, in the session's kind.
    assert!(f.store.bind_agent("owl", "ochre-owl", false).unwrap());
    assert_eq!(beat(&f, "owl", &[opus(5)]).as_deref(), Some("ochre-owl"));
    let idle = f.session("chat");
    assert!(f.store.bind_agent("idle", &idle, false).unwrap());
    beat(&f, "idle", &[opus(6)]);
    assert_eq!(beat(&f, "nobody", &[opus(7)]), None);
    let none = f.store.spent_on(&[""]).unwrap();
    let mut said: Vec<(&str, &str, i64)> = (none.iter())
        .map(|r| (r.handle.as_str(), r.role.as_str(), r.round))
        .collect();
    said.sort_unstable();
    let mut expected = [("ochre-owl", "worker", 0), (idle.as_str(), "chat", 0)];
    expected.sort_unstable();
    assert_eq!(said, expected);
    assert!(f.store.spent_on(&["sim/jump"]).unwrap().is_empty());
    let by = f
        .store
        .spent_by(&["ochre-owl", "amber-heron", "nobody"])
        .unwrap();
    let by: Vec<(String, Tokens)> = (by.into_iter())
        .map(|(handle, spend)| (handle, spend.tokens))
        .collect();
    assert_eq!(by, [("ochre-owl".to_string(), tokens(5))]);
}

#[test]
fn what_a_session_spent_is_priced_row_by_row_at_each_rows_model_and_speed() {
    let f = working();
    beat(
        &f,
        "exec",
        &[
            opus(1),
            at("claude-opus-5-5", "fast", 3),
            at("claude-unlisted-0", "standard", 2),
        ],
    );
    let prices = Prices::embedded();
    let mut cost = Cost::default();
    for (model, speed, n) in [
        ("claude-opus-5-5", "standard", 1),
        ("claude-opus-5-5", "fast", 3),
        ("claude-unlisted-0", "standard", 2),
    ] {
        cost.add(&prices.cost(model, speed, &tokens(n)));
    }
    // Fast mode is priced apart, above the standard speed.
    assert!(
        prices.cost("claude-opus-5-5", "fast", &tokens(1)).total()
            > prices
                .cost("claude-opus-5-5", "standard", &tokens(1))
                .total()
    );
    let spend = f.store.spent_by(&["amber-heron"]).unwrap()["amber-heron"].clone();
    assert_eq!(spend.tokens, sum(&[1, 3, 2]));
    assert_eq!(spend.cost, cost);
    assert_eq!(
        spend.cost.unpriced.keys().collect::<Vec<_>>(),
        [&("claude-unlisted-0".to_string(), "standard".to_string())]
    );
    // A task's tally prices the same rows the same way.
    let rows = f.store.spent_on(&["sim/jump"]).unwrap();
    assert_eq!(Tally::of(&rows).total.cost, cost);
}

/// Where one beat of `amber-heron`, holding `roles`, spends: the task and role.
fn spends_on(roles: &[(&str, &str, &str)]) -> Vec<(String, String)> {
    let mut seed = Seed::default().session("amber-heron");
    for (slug, status, role) in roles {
        seed = seed.task(slug, status);
        seed = if *role == "reserved" {
            seed.reserved(slug, "amber-heron", Some(NOW + 3600.0))
        } else {
            seed.role(slug, "amber-heron", role)
        };
    }
    let f = Fixture::new(seed);
    assert!(f.store.bind_agent("k", "amber-heron", false).unwrap());
    beat(&f, "k", &[opus(1)]);
    let slugs: Vec<&str> = std::iter::once("")
        .chain(roles.iter().map(|(slug, _, _)| *slug))
        .collect();
    (f.store.spent_on(&slugs).unwrap().into_iter())
        .map(|r| (r.slug, r.role))
        .collect()
}

#[test]
fn an_open_tasks_execution_comes_before_its_review_before_a_reservation_and_a_finished_task_never()
{
    let on = |slug: &str, role: &str| vec![(slug.to_string(), role.to_string())];
    let done = ("sim/done", "done", "executor");
    let review = ("sim/a", "review", "reviewer");
    let reserved = ("sim/c", "ready", "reserved");
    assert_eq!(
        spends_on(&[done, review, ("sim/b", "review", "executor"), reserved]),
        on("sim/b", "executor"),
        "an executor waiting on its review still works its task"
    );
    assert_eq!(
        spends_on(&[done, reserved, review]),
        on("sim/a", "reviewer")
    );
    assert_eq!(spends_on(&[done, reserved]), on("sim/c", "reserved"));
    assert_eq!(spends_on(&[done]), on("", "worker"));
}

#[test]
fn a_board_of_schema_18_gains_the_usage_table_with_every_row_kept_and_a_beat_then_spends() {
    let f = working();
    let original = f.store.dump().unwrap();
    assert!(original.usages.is_empty());
    // The board as schema 18 left it: every table but `usage`, which it never had.
    let client = f.postgresql();
    client.simple("DROP TABLE usage").unwrap();
    (f.store.raw().row("meta", "schema"))
        .set(json!({"version": 18}))
        .unwrap();
    let tables = |_: &symphony_board::Store| -> Vec<String> {
        let listed = client
            .request(&[symphony_board::pg::Step::bare(
                "SELECT coalesce(jsonb_agg(table_name::text ORDER BY table_name), '[]')::text \
                 FROM information_schema.tables WHERE table_schema = current_schema()",
            )])
            .unwrap();
        serde_json::from_str(listed[0].value().unwrap()).unwrap()
    };
    assert!(!tables(&f.store).contains(&"usage".to_string()));
    let upgraded = f.other();
    assert!(tables(&upgraded).contains(&"usage".to_string()));
    let version = (upgraded.raw().row("meta", "schema").field("version")).unwrap();
    assert_eq!(version, json!(Schema::this_build().version()));
    assert_eq!(upgraded.dump().unwrap(), original);
    f.advance(1.0);
    let beat = Beat::default();
    let spent = upgraded.agent_beat_spending("exec", &beat, None, None, &[opus(2)]);
    assert_eq!(spent.unwrap().as_deref(), Some("amber-heron"));
    let rows = upgraded.spent_on(&["sim/jump"]).unwrap();
    let read: Vec<(&str, &str, i64, Tokens)> = (rows.iter())
        .map(|r| (r.handle.as_str(), r.role.as_str(), r.round, r.tokens()))
        .collect();
    assert_eq!(read, [("amber-heron", "executor", 1, tokens(2))]);
}

#[test]
fn a_copy_carries_the_rows_and_the_dump_reads_them_back() {
    let f = working();
    beat(&f, "exec", &[opus(3)]);
    let dump = f.store.dump().unwrap();
    assert_eq!(dump.usages.len(), 1);
    let again = Fixture::new(Seed { dump: dump.clone() });
    assert_eq!(again.store.dump().unwrap().usages, dump.usages);
    assert_eq!(
        again.store.spent_on(&["sim/jump"]).unwrap(),
        f.store.spent_on(&["sim/jump"]).unwrap()
    );
}

/// `t` as the roll-up's read gives a sum of it.
fn token_sum(role: &str, round: u32, model: &str, t: Tokens) -> symphony_api::reports::TokenSum {
    symphony_api::reports::TokenSum {
        role: role.into(),
        round,
        model: model.into(),
        speed: "standard".into(),
        input: t.input,
        cache_write_5m: t.cache_write_5m,
        cache_write_1h: t.cache_write_1h,
        cache_read: t.cache_read,
        output: t.output,
        calls: t.calls,
    }
}

#[test]
fn the_roll_ups_read_sums_the_table_and_names_a_landed_task_with_its_subtasks() {
    let f = working();
    f.task_with(symphony_board::NewTask {
        parent: Some("sim/jump".into()),
        ..symphony_board::NewTask::new("sim/jump/fix", "Fix it", "ochre-owl")
    });
    let wren = f.session("worker");
    f.store.claim("sim/jump/fix", &wren).unwrap();
    assert!(f.store.bind_agent("fix", &wren, false).unwrap());
    assert!(f.store.bind_agent("owl", "ochre-owl", false).unwrap());
    beat(&f, "exec", &[opus(1)]);
    beat(&f, "fix", &[opus(4)]);
    // Orchestrating is no task's: in the table's sums, in no task's.
    beat(&f, "owl", &[at("gpt-x", "standard", 5)]);
    f.store
        .move_to("sim/jump", "review", "amber-heron")
        .unwrap();
    f.store
        .take_role("sim/jump", "brisk-finch", "reviewer")
        .unwrap();
    beat(&f, "rev", &[opus(2)]);
    let open = f.store.report_records(NOW).unwrap();
    let usage = [
        token_sum("executor", 0, "claude-opus-5-5", sum(&[1, 4])),
        token_sum("reviewer", 0, "claude-opus-5-5", sum(&[2])),
        token_sum("worker", 0, "gpt-x", tokens(5)),
    ];
    assert_eq!(
        open.usage, usage,
        "the whole table, by role, model and speed"
    );
    assert!(open.spent.is_empty(), "no task is done yet");
    f.store
        .verdict("sim/jump", "brisk-finch", "approve", "fine")
        .unwrap();
    f.store
        .landed(
            "sim/jump",
            "ochre-owl",
            "landed sim/jump on main at 3e0c661",
            false,
            &["done"],
        )
        .unwrap();
    let log = f.store.database().queries();
    let (before, writes) = log.report(symphony_board::sweep::trigger::now());
    let landed = f.store.report_records(NOW).unwrap();
    let (after, writes_after) = log.report(symphony_board::sweep::trigger::now());
    assert_eq!(after.count - before.count, 1, "still one round trip");
    assert_eq!(writes_after.count, writes.count, "and no write");
    assert_eq!(landed.usage, usage);
    let spent: Vec<(&str, &symphony_api::reports::TokenSum)> = (landed.spent.iter())
        .map(|(slug, sum)| (slug.as_str(), sum))
        .collect();
    assert_eq!(
        spent,
        [
            // Its subtask's round 1 is added to its own.
            (
                "sim/jump",
                &token_sum("executor", 1, "claude-opus-5-5", sum(&[1, 4]))
            ),
            (
                "sim/jump",
                &token_sum("reviewer", 2, "claude-opus-5-5", sum(&[2]))
            ),
        ]
    );
}

#[test]
fn the_cards_read_gives_each_task_its_subtasks_spend_in_one_round_trip() {
    let f = working();
    f.task_with(symphony_board::NewTask {
        parent: Some("sim/jump".into()),
        ..symphony_board::NewTask::new("sim/jump/fix", "Fix it", "ochre-owl")
    });
    f.task_with(symphony_board::NewTask::new(
        "sim/quiet",
        "Quiet",
        "ochre-owl",
    ));
    let wren = f.session("worker");
    f.store.claim("sim/jump/fix", &wren).unwrap();
    assert!(f.store.bind_agent("fix", &wren, false).unwrap());
    beat(&f, "exec", &[opus(1)]);
    beat(&f, "fix", &[opus(4), at("gpt-x", "standard", 2)]);
    let log = f.store.database().queries();
    let (before, writes) = log.report(symphony_board::sweep::trigger::now());
    let read = f
        .store
        .spent_under(&["sim/jump", "sim/jump/fix", "sim/quiet", "sim/none"])
        .unwrap();
    let (after, writes_after) = log.report(symphony_board::sweep::trigger::now());
    assert_eq!(
        after.count - before.count,
        1,
        "one round trip for every slug"
    );
    assert_eq!(writes_after.count, writes.count, "and no write");
    let prices = Prices::embedded();
    let spend = |rows: &[&str]| {
        let mut all = symphony_board::usage::Spend::default();
        for row in f.store.spent_on(rows).unwrap() {
            all.add(&row.spend(prices));
        }
        all
    };
    // A task's figure is its page's: its own and its subtasks', every model's.
    assert_eq!(read["sim/jump"], spend(&["sim/jump", "sim/jump/fix"]));
    assert_eq!(read["sim/jump/fix"], spend(&["sim/jump/fix"]));
    assert_eq!(
        read["sim/jump"].cost.unpriced.len(),
        1,
        "gpt-x has no price"
    );
    assert_eq!(
        read.keys().collect::<Vec<_>>(),
        ["sim/jump", "sim/jump/fix"],
        "one that took none, or no task at all, is left out"
    );
    assert!(f.store.spent_under(&[]).unwrap().is_empty());
}
