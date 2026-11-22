//! The snapshot a read of tasks gives ([`reading`]) and the presence reads it shares. Its
//! variables are the store's ([`crate::store::reading::vars`]), the same for each database.

use super::super::{Sessions, Which};
use super::cap;

/// The statements that read `which`, ending in the `RETURN` of a `Snapshot`, with the sessions
/// `sessions` asks for.
pub(in crate::store) fn reading(which: &Which, sessions: Sessions) -> String {
    const SCOPED: &str = " WHERE slug IN $slugs";
    const KNOWN: &str = "record::exists(type::record('task', $slug))";
    let (pick, scope, known) = match which {
        Which::All | Which::SlotHoldings => (
            "SELECT * OMIT id FROM task ORDER BY priority, sid",
            "",
            "true",
        ),
        Which::Status(_) => (
            "SELECT * OMIT id FROM task WHERE status = $status ORDER BY priority, sid",
            SCOPED,
            "true",
        ),
        Which::Slug(_) | Which::Starting(_) => (
            "SELECT * OMIT id FROM type::record('task', $slug)",
            SCOPED,
            KNOWN,
        ),
        Which::Slugs(_) => (
            "SELECT * OMIT id FROM $wanted.map(|$slug| type::record('task', $slug)) ORDER BY priority, sid",
            SCOPED,
            "true",
        ),
        Which::Children(_) => (
            "SELECT * OMIT id FROM task WHERE parent = $slug ORDER BY sid",
            SCOPED,
            KNOWN,
        ),
        Which::Pullable(_) => (
            "SELECT * OMIT id FROM task WHERE status IN $pullable ORDER BY sid",
            SCOPED,
            "record::exists(type::record('session', $handle))",
        ),
        Which::Linked(_) => (
            "SELECT * OMIT id FROM array::distinct(SELECT VALUE slug FROM link
                     WHERE kind = 'doc' AND (target = $doc OR string::starts_with(target, $doc + '#')))
                     .map(|$s| type::record('task', $s)) ORDER BY priority, slug",
            SCOPED,
            "true",
        ),
    };
    let (deps, mine) = match which {
        Which::Slug(_) | Which::Starting(_) | Which::Pullable(_) => (
            "(SELECT slug, kind, target FROM link WHERE slug IN $slugs AND kind = 'dep'
                  AND type::record('task', target).status NOT IN [NONE, 'done'] ORDER BY target)",
            if matches!(which, Which::Pullable(_)) {
                "(SELECT VALUE slug FROM role WHERE handle = $handle)"
            } else {
                "[]"
            },
        ),
        _ => ("[]", "[]"),
    };
    let pull_lets = if matches!(which, Which::Pullable(_)) {
        CLAIM_STATUSES
    } else {
        ""
    };
    let (slot_lets, slots) = if matches!(which, Which::Starting(_) | Which::SlotHoldings) {
        let wanted = if matches!(which, Which::SlotHoldings) {
            "$slugs"
        } else {
            "[$slug]"
        };
        (cap::lets(wanted), cap::field())
    } else {
        (String::new(), String::new())
    };
    let (shown, sessions) = match (sessions, which) {
        (Sessions::Whole, _) => ("[]", "session"),
        (Sessions::Shown, Which::Pullable(_)) => (
            "array::distinct(array::concat($view_roles.handle, [$handle]))",
            SHOWN,
        ),
        (Sessions::Shown, _) => ("array::distinct($view_roles.handle)", SHOWN),
    };
    format!(
        "{pull_lets}
             LET $tasks = ({pick});
             LET $slugs = $tasks.slug;
             LET $slot_wanted = $slugs;
             LET $view_roles = (SELECT slug, handle, role, since, expires FROM role{scope} ORDER BY since, handle);
             LET $shown_sessions = {shown};
             {HELD}
             {slot_lets}
             RETURN {{
                 tasks: $tasks,
                 branches: ({}),
                 links: (SELECT slug, kind, target FROM link{scope} ORDER BY target),
                 roles: $view_roles,
                 sessions: (SELECT * OMIT id FROM {sessions} ORDER BY connected, handle),
                 held: $held,
                 latest: (SELECT VALUE last FROM ONLY counter:message) ?? 0,
                 callback_sequence: $callback_sequence ?? 0,
                 has_callbacks: $has_callbacks ?? false,
                 native_presence: {NATIVE_PRESENCE},
                 open_deps: {deps},
                 known: {known},
                 mine: {mine},
                 {slots}
             }};", super::branches::OF_TASKS
    )
}

/// `$pullable` widened by the statuses that the positions an executor's claim edge leaves project
/// to, in the active custom workflow: a task it holds is offered by its claim
/// ([`crate::Store::next_for`]), whatever status that position projects to. The definition is
/// read by its id, and the task read stays on its status index.
pub(in crate::store) const CLAIM_STATUSES: &str = "LET $claiming = (SELECT VALUE body.definition FROM ONLY type::record('workflow', ['definition', (SELECT VALUE body.digest FROM ONLY workflow:['active']) ?? ''])) ?? { positions: [], edges: [] };
LET $claimed_at = $claiming.edges.filter(|$e| $e.automatic != true AND array::len($e.effects.filter(|$f| $f.kind = 'acquire_role' AND $f.role = 'executor')) > 0).from;
LET $pullable = array::union($pullable, $claiming.positions.filter(|$p| $p.at IN $claimed_at).legacy);";

/// The sessions a read's answer can show ([`Sessions::Shown`]), each by its id, never a scan of
/// the table: a handle the board holds no session of gives no row, as the whole list has none.
pub(in crate::store) const SHOWN: &str = "$shown_sessions.map(|$h| type::record('session', $h))";

/// `$held`: every session holding work another could take over (`CLAIMABLE`, for everyone), for
/// the `lost` line of health. Needs `$executing`. The roles are read by their tasks' slugs, through
/// `role_slug`, and kept by role after: one condition over both would read every role.
pub(in crate::store) const HELD: &str = "LET $open = SELECT VALUE slug FROM task WHERE status IN $executing;
LET $waiting = SELECT VALUE slug FROM task WHERE status = 'review';
LET $held = array::distinct((SELECT handle, role, slug FROM role WHERE slug IN array::concat($open, $waiting))
    .filter(|$r| ($r.role = 'executor' AND $r.slug IN $open) OR ($r.role = 'reviewer' AND $r.slug IN $waiting))
    .map(|$r| $r.handle));";

/// The Codex registrations' presence, through `callback_state_address`: an address starting
/// `codex:` is one from `$codex` up to `$codex_after`, the bounds
/// [`crate::store::reading::native_bounds`] gives. They
/// are variables, not text: the client takes a request for a read only when no `;` but those
/// between its statements is in it (`rpc::Kind::of` splits on every one, quoted or not), and a
/// read it took for a write would never be sent again when late.
pub(in crate::store) const NATIVE_PRESENCE: &str = "(SELECT handle, confirmed_until ?? 0 AS until FROM callback_state WHERE address >= $codex AND address < $codex_after)";
