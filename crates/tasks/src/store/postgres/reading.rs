//! The snapshot a read of tasks gives on PostgreSQL: the tasks picked, then what the view reads
//! beside them, over `v`. A read lists its tasks, links, roles and sessions an item a row
//! ([`reading`], [`super::Pg::Listed`]), so a whole board costs its backend a sort's memory and
//! not the board's; a write's read of its one task is one value ([`written`]).

use super::branches::of_tasks;
use super::{List, Listed, Pg, cap, row};
use crate::store::{Sessions, Which};

/// Each part of a `Snapshot` a read does not take from what it picked: `callback_sequence` (the
/// whole board's read, [`super::snapshot`]) and `has_callbacks` (a write's, [`written`]).
#[derive(Clone, Copy)]
pub(in crate::store) struct Beside {
    pub callback_sequence: &'static str,
    pub has_callbacks: &'static str,
}

const NOTHING_BESIDE: Beside = Beside {
    callback_sequence: "0",
    has_callbacks: "false",
};

/// Whether any session that has not left keeps a callback address: the channels by
/// `callback_state_address` (`super::callbacks::registrations`).
pub(in crate::store) const HAS_CALLBACKS: &str = "EXISTS (SELECT 1 FROM callback_state c \
    WHERE c.doc ->> 'address' IS NOT NULL \
    AND (SELECT s.left_at FROM session s WHERE s.handle = c.doc ->> 'handle') IS NULL)";

/// Whether the task `v ->> 'slug'` is there.
const KNOWN: &str = "EXISTS (SELECT 1 FROM task WHERE slug = v ->> 'slug')";

/// The pullable statuses, with those the positions an executor's claim edge leaves project to in
/// the active custom workflow.
const CLAIM_STATUSES: &str = "WHERE t.status IN (SELECT jsonb_array_elements_text(v -> 'pullable') \
    UNION SELECT p ->> 'legacy' FROM workflow d \
        CROSS JOIN LATERAL jsonb_array_elements(d.body -> 'definition' -> 'positions') p \
        WHERE d.id = jsonb_build_array('definition', \
            (SELECT body ->> 'digest' FROM workflow WHERE id = '[\"active\"]'::jsonb)) \
        AND EXISTS (SELECT 1 FROM jsonb_array_elements(d.body -> 'definition' -> 'edges') e \
            WHERE e -> 'from' = p -> 'at' AND coalesce((e ->> 'automatic')::boolean, false) = false \
            AND e -> 'effects' @> '[{\"kind\": \"acquire_role\", \"role\": \"executor\"}]'::jsonb))";

/// The active workflow's generation, or JSON null on a board that never activated one.
const ACTIVE_GENERATION: &str =
    "(SELECT body -> 'generation' FROM workflow WHERE id = '[\"active\"]'::jsonb)";

/// The read of `which` with the sessions `sessions` asks for.
pub(in crate::store) fn reading(which: &Which, sessions: Sessions) -> Pg<'static> {
    reading_with(which, sessions, NOTHING_BESIDE)
}

/// What `which` picks: its tasks (rows `t` of `task`), the order they come in (`p` a row of
/// them), and when the one it is about is there.
fn picking(which: &Which) -> (&'static str, &'static str, &'static str) {
    const BY_PRIORITY: &str = "p.priority, p.sid";
    match which {
        Which::All | Which::SlotHoldings => ("", BY_PRIORITY, "true"),
        Which::Status(_) => ("WHERE t.status = v ->> 'status'", BY_PRIORITY, "true"),
        Which::Slug(_) | Which::Starting(_) => ("WHERE t.slug = v ->> 'slug'", "p.sid", KNOWN),
        Which::Slugs(_) => (
            "WHERE t.slug IN (SELECT jsonb_array_elements_text(v -> 'wanted'))",
            BY_PRIORITY,
            "true",
        ),
        Which::Children(_) => ("WHERE t.parent = v ->> 'slug'", "p.sid", KNOWN),
        Which::Pullable(_) => (
            CLAIM_STATUSES,
            "p.sid",
            "EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'handle')",
        ),
        Which::Linked(_) => (
            "WHERE t.slug IN (SELECT l.slug FROM link l WHERE l.kind = 'doc' \
        AND (l.target = v ->> 'doc' OR starts_with(l.target, (v ->> 'doc') || '#')))",
            "p.priority, p.slug",
            "true",
        ),
    }
}

/// [`reading`], with what `beside` gives.
pub(in crate::store) fn reading_with(
    which: &Which,
    sessions: Sessions,
    beside: Beside,
) -> Pg<'static> {
    Pg::Listed(listed(which, sessions, beside))
}

/// The read of `which` as its lists and its other fields, with what `beside` gives.
fn listed(which: &Which, sessions: Sessions, beside: Beside) -> Listed {
    let (picked, order, known) = picking(which);
    let whole = matches!(which, Which::All | Which::SlotHoldings);
    let scoped = |alias: &str| match whole {
        true => "true".to_string(),
        false => format!("{alias}.slug IN (SELECT slug FROM picked)"),
    };
    let one = matches!(which, Which::Slug(_) | Which::Starting(_));
    let open_deps = if one || matches!(which, Which::Pullable(_)) {
        "coalesce((SELECT jsonb_agg(to_jsonb(l) - 'id' \
            ORDER BY l.target, l.slug) \
        FROM link l JOIN task t ON t.slug = l.target \
        WHERE l.slug IN (SELECT slug FROM picked) AND l.kind = 'dep' AND t.status <> 'done'), '[]')"
    } else {
        "'[]'::jsonb"
    };
    let (mine, asker) = match which {
        Which::Pullable(_) => (
            "coalesce((SELECT jsonb_agg(slug ORDER BY slug, role) FROM role WHERE handle = v ->> 'handle'), \
        '[]')",
            " OR s.handle = v ->> 'handle'",
        ),
        _ => ("'[]'::jsonb", ""),
    };
    let sessions = match sessions {
        Sessions::Whole => "session s".to_string(),
        Sessions::Shown => {
            format!("session s WHERE s.handle IN (SELECT handle FROM view_roles){asker}")
        }
    };
    // The cap's read beside a start's task and the whole board's holdings (`super::cap`), and the
    // branches of every task it reads; else those of the tasks picked.
    let (slot_lets, slots, wanted) = match which {
        Which::Starting(_) => (
            format!(",\n    {}", cap::lets("SELECT v ->> 'slug'")),
            cap::field(),
            "slot_wanted",
        ),
        Which::SlotHoldings => (
            format!(",\n    {}", cap::lets("SELECT slug FROM task")),
            cap::field(),
            "slot_wanted",
        ),
        _ => (String::new(), String::new(), "picked"),
    };
    let branches = of_tasks(&format!("b.slug IN (SELECT slug FROM {wanted})"));
    // The whole board's read carries the active workflow's generation, read by its id.
    let generation = match which {
        Which::All => ACTIVE_GENERATION,
        _ => "'null'::jsonb",
    };
    let Beside {
        callback_sequence,
        has_callbacks,
    } = beside;
    let with = format!(
        "picked AS (SELECT t.* FROM task t {picked}), \
    view_roles AS (SELECT r.slug, r.handle, r.role, r.since, r.expires FROM role r WHERE {}), \
    open_slugs AS (SELECT slug FROM task WHERE status IN (SELECT jsonb_array_elements_text(v -> 'executing'))), \
    held AS (SELECT DISTINCT r.handle FROM role r \
        WHERE (r.role = 'executor' AND r.slug IN (SELECT slug FROM open_slugs)) \
            OR (r.role = 'reviewer' AND r.slug IN (SELECT slug FROM task WHERE status = 'review'))){slot_lets}",
        scoped("r"),
    );
    let lists = vec![
        List {
            key: "tasks",
            item: row("p"),
            from: "picked p".into(),
            order: order.into(),
        },
        List {
            key: "links",
            item: "to_jsonb(l) - 'id'".into(),
            from: format!("link l WHERE {}", scoped("l")),
            order: "l.target, l.slug, l.kind".into(),
        },
        List {
            key: "roles",
            item: "jsonb_strip_nulls(to_jsonb(r))".into(),
            from: "view_roles r".into(),
            order: "r.since, r.handle, r.slug, r.role".into(),
        },
        List {
            key: "sessions",
            item: row("s"),
            from: sessions,
            order: "s.connected, s.handle".into(),
        },
    ];
    let rest = format!(
        "'branches', {branches}, \
    'held', coalesce((SELECT jsonb_agg(handle) FROM held), '[]'), \
    'latest', coalesce((SELECT to_jsonb(last) FROM counter WHERE id = '\"message\"'), '0'::jsonb), \
    'workflow_generation', {generation}, \
    'callback_sequence', {callback_sequence}, \
    'has_callbacks', {has_callbacks}, \
    'native_presence', coalesce((SELECT jsonb_agg(jsonb_build_object('handle', c.doc -> 'handle', \
            'until', coalesce(c.doc -> 'confirmed_until', '0'::jsonb))) \
        FROM callback_state c WHERE starts_with(c.doc ->> 'address', 'codex:')), '[]'), \
    'open_deps', {open_deps}, \
    {slots} \
    'known', {known}, \
    'mine', {mine}"
    );
    Listed {
        with,
        lists,
        rest: Some(rest),
    }
}

/// `slug` read as `get` reads it after a write, with whether any session keeps a callback address
/// (`Store::written`), as one expression: one task's read is short, and follows its block.
pub(in crate::store) fn written(which: &Which) -> String {
    let beside = Beside {
        callback_sequence: "0",
        has_callbacks: HAS_CALLBACKS,
    };
    listed(which, Sessions::Shown, beside).whole()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list `key` of `read`.
    fn list<'a>(read: &'a Listed, key: &str) -> &'a List {
        let found = read.lists.iter().find(|list| list.key == key);
        found.unwrap_or_else(|| panic!("no list {key}"))
    }

    #[test]
    fn each_read_picks_its_tasks_and_only_a_start_and_the_holdings_read_the_cap() {
        let wanted = ["work/a".to_string()];
        let reads = [
            Which::All,
            Which::SlotHoldings,
            Which::Status("ready"),
            Which::Slug("work/a"),
            Which::Starting("work/a"),
            Which::Slugs(&wanted),
            Which::Children("work/a"),
            Which::Linked("docs/a.md"),
            Which::Pullable("kite"),
        ];
        for (at, which) in reads.iter().enumerate() {
            let read = listed(which, Sessions::Shown, NOTHING_BESIDE);
            let rest = read.rest.as_deref().expect("a read has other fields");
            let (picked, order, known) = picking(which);
            assert!(
                read.with
                    .starts_with(&format!("picked AS (SELECT t.* FROM task t {picked}),")),
                "{at}: {}",
                read.with
            );
            let keys: Vec<&str> = read.lists.iter().map(|list| list.key).collect();
            assert_eq!(keys, ["tasks", "links", "roles", "sessions"], "{at}");
            let tasks = list(&read, "tasks");
            assert_eq!(
                (tasks.from.as_str(), tasks.order.as_str()),
                ("picked p", order)
            );
            assert!(rest.contains(&format!("'known', {known},")), "{at}");
            let capped = matches!(which, Which::Starting(_) | Which::SlotHoldings);
            assert_eq!(read.with.contains("slot_wanted AS ("), capped, "{at}");
            assert_eq!(
                rest.contains("'slots', jsonb_build_object("),
                capped,
                "{at}"
            );
            let branches_of = if capped { "slot_wanted" } else { "picked" };
            assert!(
                rest.contains(&format!("b.slug IN (SELECT slug FROM {branches_of})")),
                "{at}"
            );
            let whole = matches!(which, Which::All | Which::SlotHoldings);
            assert_eq!(read.with.contains("FROM role r WHERE true)"), whole, "{at}");
            let scope = if whole {
                "link l WHERE true"
            } else {
                "link l WHERE l.slug IN (SELECT slug FROM picked)"
            };
            assert_eq!(list(&read, "links").from, scope, "{at}");
            let one = matches!(
                which,
                Which::Slug(_) | Which::Starting(_) | Which::Pullable(_)
            );
            assert_eq!(
                rest.contains("l.kind = 'dep' AND t.status <> 'done'"),
                one,
                "{at}"
            );
            let pulling = matches!(which, Which::Pullable(_));
            assert_eq!(
                list(&read, "sessions")
                    .from
                    .ends_with(" OR s.handle = v ->> 'handle'"),
                pulling,
                "{at}"
            );
            assert_eq!(
                rest.contains("FROM role WHERE handle = v ->> 'handle'"),
                pulling,
                "{at}"
            );
        }
    }

    #[test]
    fn a_whole_read_takes_every_session_and_a_written_one_whether_any_listens() {
        let whole = listed(&Which::All, Sessions::Whole, NOTHING_BESIDE);
        assert_eq!(list(&whole, "sessions").from, "session s");
        let rest = whole.rest.as_deref().unwrap();
        assert!(rest.contains("'has_callbacks', false,"), "{rest}");
        let written = written(&Which::Slug("work/a"));
        assert!(written.contains("s.handle IN (SELECT handle FROM view_roles)"));
        assert!(written.contains(&format!("'has_callbacks', {HAS_CALLBACKS},")));
    }

    #[test]
    fn a_read_lists_an_item_a_row_and_a_written_one_is_one_value_of_the_same_lists() {
        let Pg::Listed(read) = reading(&Which::All, Sessions::Whole) else {
            panic!("a read lists its rows");
        };
        assert_eq!(read, listed(&Which::All, Sessions::Whole, NOTHING_BESIDE));
        let written = written(&Which::Slug("work/a"));
        assert!(written.starts_with("(WITH picked AS (SELECT t.* FROM task t WHERE"));
        for key in ["tasks", "links", "roles", "sessions"] {
            let whole = format!("'{key}', coalesce((SELECT jsonb_agg(");
            assert!(written.contains(&whole), "{key}: {written}");
        }
        assert!(written.contains("ORDER BY p.sid) FROM picked p), '[]')"));
        assert!(written.ends_with("'mine', '[]'::jsonb))"), "{written}");
    }
}
