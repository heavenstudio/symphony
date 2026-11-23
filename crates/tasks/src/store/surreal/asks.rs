//! The statements of `store/asks.rs`.

use std::sync::LazyLock;

/// The open ask about `$about` joined by the owner's line said at `$at`, or a new one numbered by
/// `counter:ask` (`$ask` names it after), from `NewAsk`'s variables (`Store::ask_vars`).
const JOIN_OR_OPEN: &str = "
LET $joined = (SELECT VALUE sid FROM ask WHERE about = $about AND open = true LIMIT 1)[0];
LET $ask = IF $joined != NONE { $joined } ELSE { (UPSERT ONLY counter:ask SET last += 1).last };
IF $joined != NONE {
    UPDATE type::record('ask', $ask) SET lines += 1, last = math::max([last, $at]) RETURN NONE;
} ELSE {
    CREATE type::record('ask', $ask) CONTENT {
        sid: $ask, kind: $kind, about: $about, path: $path, card: $card, block: $block, tree: $tree,
        task: $task, thread: $thread, words: $words, asked: $at, lines: 1, last: $at,
        before: [], open: true, closed: false,
    } RETURN NONE;
};";

/// [`JOIN_OR_OPEN`], the task a task's ask names being on the board, answering the ask.
pub(in crate::store) static OPEN: LazyLock<String> = LazyLock::new(|| {
    format!(
        "IF $task != NONE AND !record::exists(type::record('task', $task)) {{ THROW $no_task }};
{JOIN_OR_OPEN}
RETURN (SELECT * OMIT id FROM ONLY type::record('ask', $ask));"
    )
});

/// The owner's save, as the Symphony hands it to the board: the chat event `$key` queued with its
/// words for the chat's recipient, and, with `$asking`, its ask opened or joined ([`JOIN_OR_OPEN`]),
/// both in this one write and once per key, so a save handed over again changes nothing. A save with
/// no words to deliver (a rejection with no note) queues its event as delivered already, which only
/// keeps the key.
pub(in crate::store) static OWNER_SAVE: LazyLock<String> = LazyLock::new(|| {
    format!(
        "IF !record::exists(type::record('chat_event', $key)) {{
    CREATE type::record('chat_event', $key) SET key = $key, text = $text, delivered = ($text = '') RETURN NONE;
    IF $asking {{
        {JOIN_OR_OPEN}
    }};
}};
RETURN NONE;"
    )
});

/// What a post on a task's thread does to the task's ask, in the post's write, after its line
/// (`$sid`, said at `$ts` by `$sender`): the owner's line, or a person's (`$by_person`), `$asking`,
/// opens the ask about the task (`$task_about`) or joins the open one; another line of the owner's
/// changes nothing; a line by a session holding a role on the task, or by the session the last
/// line there of the owner's or a person's was sent to, answers the open one, the answer being
/// that line.
pub(in crate::store) const ON_THREAD: &str = "
LET $task_ask = (SELECT VALUE sid FROM ask WHERE about = $task_about AND open = true LIMIT 1)[0];
IF ($sender = 'owner' OR ($by_person != NONE AND $sender = $by_person)) AND $asking {
    IF $task_ask != NONE {
        UPDATE type::record('ask', $task_ask) SET lines += 1, last = math::max([last, $ts]) RETURN NONE;
    } ELSE {
        LET $opened = (UPSERT ONLY counter:ask SET last += 1).last;
        CREATE type::record('ask', $opened) CONTENT {
            sid: $opened, kind: 'task', about: $task_about, task: $slug, words: $task_words,
            asked: $ts, lines: 1, last: $ts, before: [], open: true, closed: false,
        } RETURN NONE;
    };
} ELSE IF $task_ask != NONE {
    LET $holds = array::len((SELECT VALUE id FROM role:[$slug, $sender, NONE]..[$slug, $sender, ..] LIMIT 1)) > 0;
    LET $named = IF $holds { NONE } ELSE {
        (SELECT VALUE recipient FROM message:[$slug, NONE]..[$slug, ..] WHERE sender = 'owner' OR person = true ORDER BY sid DESC LIMIT 1)[0]
    };
    IF $holds OR $named = $sender {
        UPDATE type::record('ask', $task_ask) SET open = false, answered = $ts, answered_by = $sender,
            answer = string::concat('#', <string> $sid) RETURN NONE;
    };
};";

/// `$sid` held by `$holder` (none: nobody) from `$now`, the holder before it kept with why it moved.
pub(in crate::store) const HOLD: &str = "
LET $a = (SELECT holder, held, open FROM ONLY type::record('ask', $sid));
IF $a = NONE { THROW $no_ask };
IF !$a.open { THROW $answered };
IF $holder != NONE AND !record::exists(type::record('session', $holder)) { THROW $unknown };
IF $a.holder != $holder {
    UPDATE type::record('ask', $sid) SET
        before += IF $a.holder != NONE { [{ handle: $a.holder, since: $a.held ?? $now, until: $now, why: $why }] } ELSE { [] },
        holder = $holder,
        held = IF $holder != NONE { $now } ELSE { NONE }
    RETURN NONE;
};
RETURN (SELECT * OMIT id FROM ONLY type::record('ask', $sid));";

/// `$sid` answered at `$now`, unless an answer came first: the first one stands.
pub(in crate::store) const ANSWER: &str = "
LET $a = (SELECT open FROM ONLY type::record('ask', $sid));
IF $a = NONE { THROW $no_ask };
IF $a.open {
    UPDATE type::record('ask', $sid) SET open = false, answered = $now, answered_by = $by, answer = $answer RETURN NONE;
};
RETURN (SELECT * OMIT id FROM ONLY type::record('ask', $sid));";

/// The open ask about `$about` answered at `$now`, if one is open.
pub(in crate::store) const ANSWER_ABOUT: &str = "
LET $sid = (SELECT VALUE sid FROM ask WHERE about = $about AND open = true LIMIT 1)[0];
IF $sid != NONE {
    UPDATE type::record('ask', $sid) SET open = false, answered = $now, answered_by = $by, answer = $answer RETURN NONE;
};
RETURN IF $sid = NONE { NONE } ELSE { (SELECT * OMIT id FROM ONLY type::record('ask', $sid)) };";

/// `$sid` closed by hand by the session `$by`, as needing no answer.
pub(in crate::store) const CLOSE: &str = "
IF !record::exists(type::record('session', $by)) { THROW $unknown };
LET $a = (SELECT open FROM ONLY type::record('ask', $sid));
IF $a = NONE { THROW $no_ask };
IF !$a.open { THROW $answered };
UPDATE type::record('ask', $sid) SET open = false, answered = $now, answered_by = $by, answer = $why, closed = true RETURN NONE;
RETURN (SELECT * OMIT id FROM ONLY type::record('ask', $sid));";

/// `$sid`, while open, marked from `$now` as what it is about read gone, in the words `$why`, or no
/// longer (`$why` none). A mark as it stands, or one on an answered ask, changes nothing; a mark
/// reworded keeps its time.
pub(in crate::store) const GONE: &str = "
LET $a = (SELECT open, gone, gone_since FROM ONLY type::record('ask', $sid));
IF $a = NONE { THROW $no_ask };
IF $a.open AND $a.gone != $why {
    UPDATE type::record('ask', $sid) SET gone = $why,
        gone_since = IF $why = NONE { NONE } ELSE { $a.gone_since ?? $now }
    RETURN NONE;
};
RETURN (SELECT * OMIT id FROM ONLY type::record('ask', $sid));";

/// The open asks, oldest first, and those answered since `$since` (none: none of them), newest first.
pub(in crate::store) const ASKS: &str = "RETURN {
    open: (SELECT * OMIT id FROM ask WHERE open = true ORDER BY asked, sid),
    answered: IF $since = NONE { [] } ELSE { (SELECT * OMIT id FROM ask WHERE answered >= $since ORDER BY answered DESC, sid DESC) },
};";

/// Ask `$sid`, open or answered, or none.
pub(in crate::store) const ASK: &str =
    "RETURN (SELECT * OMIT id FROM ONLY type::record('ask', $sid));";
