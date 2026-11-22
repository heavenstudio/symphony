//! A project's writers on its live board ([`symphony_board::Writer`]): where its default branch
//! commits no board source, `tasks migrate` in its main checkout names a writer seen in the last
//! week that its move would leave behind and moves nothing; the writer, rebuilt and run once, writes
//! its line though the board refuses it; the migrate then moves the board; and a writer built
//! before it is refused, named as what to rebuild. `--leave-behind` moves a board all the same.
//! A line's time is the machine's clock, which `TASKS_NOW` does not pin, so these write a line seen
//! some hours before now by hand.

use super::{Held, Live, before, refused, this};
use crate::common::NOW;
use serde_json::json;
use symphony_board::rpc::Client;
use symphony_board::sweep::trigger::now;
use symphony_board::{Opening, Program, Raw, Schema, Store, Writer};

const HOUR: f64 = 3600.0;

/// The project's own gate, which opens its board to write through the board's crate.
fn gate() -> Program {
    Program::named("gate-review", "/work/live/.target/debug/gate-review")
}

/// `program`'s line, written on `client`'s board as a build at `schema` that carries the record
/// writes it, last seen at `seen`.
fn write_line(client: &Client, program: &Program, host: &str, schema: i64, seen: f64) {
    client
        .query(
            "UPSERT type::record('writer', [$host, $path]) CONTENT { program: $program, path: \
             $path, host: $host, schema: $schema, seen: $seen } RETURN NONE;",
            json!({
                "host": host,
                "path": program.path,
                "program": program.name,
                "schema": schema,
                "seen": seen,
            }),
        )
        .unwrap();
}

/// Every writer's line on `client`'s board.
fn lines(client: Client) -> Vec<Writer> {
    Store::unchecked(client, Box::new(|| NOW), Box::new(|_| true), "")
        .writers()
        .unwrap()
}

#[test]
fn a_migrate_waits_for_a_writer_it_would_leave_behind_to_be_rebuilt_and_run_then_moves_the_board() {
    let live = Live::older("main-only-writers", Held::Nothing);
    write_line(
        &live.client("tasks"),
        &gate(),
        "studio",
        before(),
        now() - 3.0 * HOUR,
    );
    let left_behind = format!(
        "the live board has a writer seen in the last week behind schema {this}, the one this \
         migrate moves it to, and each would be refused the board from then on: gate-review at \
         schema {before} (/work/live/.target/debug/gate-review on studio, last seen 3 h ago). \
         Nothing was moved: rebuild each at schema {this} and run it once, which records it, then \
         `just tasks migrate` again; or `just tasks migrate --leave-behind` moves it now and \
         leaves them refused",
        this = this(),
        before = before()
    );
    refused(&live.tasks(&live.main, &["migrate"]), &left_behind);
    assert_eq!(live.version("tasks"), Some(before()), "nothing was moved");

    // The gate, rebuilt at this build's schema, runs once: the board is behind it, and only the
    // migrate moves it, so it is refused, but its line now says where it is.
    let start = now();
    let rebuilt = Store::opening(
        live.client("tasks"),
        Box::new(|| NOW),
        Box::new(|_| false),
        "studio",
        &Opening::write().by(gate()).of(Some("live")),
    );
    let why = match rebuilt {
        Err(symphony_board::Error::Refused(why)) => why,
        other => panic!("the board behind it is refused: {:?}", other.map(drop)),
    };
    assert!(
        why.starts_with(&format!(
            "the live board is at schema {}, behind this build's {}, and only `just tasks \
             migrate` run in its main checkout moves it forward",
            before(),
            this()
        )),
        "{why}"
    );
    let gate_line = |live: &Live| {
        lines(live.client("tasks"))
            .into_iter()
            .find(|w| w.program == "gate-review")
            .unwrap()
    };
    let line = gate_line(&live);
    assert_eq!(line.schema, this());
    assert!(line.seen >= start, "seen as it ran: {}", line.seen);

    // So the migrate moves the board, its own build's line among the writers it leaves out.
    let ran = live.tasks(&live.main, &["migrate"]);
    assert_eq!(
        (ran.code, ran.out, ran.err),
        (
            0,
            format!(
                "✔ moved the live board from schema {} to {}\n",
                before(),
                this()
            ),
            String::new()
        )
    );
    let tasks = lines(live.client("tasks"))
        .into_iter()
        .find(|w| w.program == "tasks")
        .expect("the migrate's own line");
    assert_eq!(tasks.schema, this());

    // A writer built before the move is refused it, named as what to rebuild.
    let older = Program::named("old-gate", "/work/live/old/gate-review");
    let refused_older = Store::opening(
        live.client("tasks"),
        Box::new(|| NOW),
        Box::new(|_| false),
        "studio",
        &Opening::write()
            .at(Schema::this_build().before())
            .by(older)
            .of(Some("live")),
    );
    let why = match refused_older {
        Err(symphony_board::Error::Refused(why)) => why,
        other => panic!("a board ahead of it is refused: {:?}", other.map(drop)),
    };
    assert_eq!(
        why,
        format!(
            "the live board is at schema {}, and this build writes only a board at its own \
             schema, {}: nothing was written; rebuild old-gate (/work/live/old/gate-review) at the \
             board's schema, and `just symphony` in live brings its Symphony to it",
            this(),
            before()
        )
    );
    assert_eq!(gate_line(&live), line, "the gate's line as it was");
}

#[test]
fn migrate_leave_behind_moves_a_board_whose_writers_are_behind_and_says_so_in_its_help() {
    let env = crate::env();
    let made = crate::run(&env, &["migrate"], "");
    assert_eq!(made.code, 0, "{}", made.err);
    let client = Client::new(&env.vars["TASKS_SERVER"], "sf", &env.vars["TASKS_DATABASE"]);
    // A board one schema behind this build that carries the record, its gate's line two days old.
    Raw::on(&client)
        .row("meta", "schema")
        .set(json!({"version": before()}))
        .unwrap();
    write_line(&client, &gate(), "studio", before(), now() - 48.0 * HOUR);
    let board = format!(
        "the board in database {} at {}",
        env.vars["TASKS_DATABASE"], env.vars["TASKS_SERVER"]
    );
    let ran = crate::run(&env, &["migrate", "--as", "owner"], "");
    assert_eq!(ran.code, 2, "{}", ran.out);
    assert!(
        ran.err.starts_with(&format!(
            "✖ this board has a writer seen in the last week behind schema {}, the one this \
             migrate moves it to",
            this()
        )),
        "{}",
        ran.err
    );
    assert!(ran.err.contains("gate-review at schema"), "{}", ran.err);
    assert!(ran.err.contains("last seen 48 h ago"), "{}", ran.err);
    let moved = crate::run(&env, &["migrate", "--leave-behind"], "");
    assert_eq!(
        (moved.code, moved.out, moved.err),
        (
            0,
            format!("✔ moved {board} from schema {} to {}\n", before(), this()),
            String::new()
        )
    );
    let help = crate::run(&env, &["migrate", "--help"], "");
    assert!(
        help.out.contains(
            "  --leave-behind  move the board though writers seen in the last week are\n"
        ),
        "{}",
        help.out
    );
}
