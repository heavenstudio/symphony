//! The board's tables in SurrealDB, one per SQLite table of `tools/tasks_store.py`, typed so a row
//! the rules would never write is refused rather than kept. A record's id is its SQLite primary key:
//! `task:⟨slug⟩`, `session:⟨handle⟩`, `role:[slug, handle, role]`, `link:[slug, kind, target]`,
//! `message:[slug, id]`, `delivery:[message, handle]`, `barred_key:⟨key⟩`, `task_session:[slug, handle]`,
//! `delivery_key:[slug, key]` (the message a keyed post made once on a thread, `Store::say_keyed`).
//! A message is keyed by its task first so a thread is one range of keys: an index on `slug` would do
//! the same, but makes a transaction inserting many messages (the copy's) cost more than linearly
//! (15k messages: 22 CPU-s with it, 2 without, measured on 3.3.0).
//! SQLite's row ids are kept as `sid`; `counter:message` and `counter:task` hold the last id given out,
//! `counter:tree` counts the moves of a task under another, and `counter:['agent', key]` the binds of
//! one agent (`Store::bind_agent`): each a record the writes that must not pass each other write.
//! Mutation events advance `counter:board` in the transaction that changes board inputs,
//! callback registration/observations or owner chat. Readers compare that marker without writing it.
//! An event UPDATEs the marker, or CREATEs it on a board's first write, and never UPSERTs it: an
//! UPSERT of a record that exists tries a create and rolls it back to a savepoint, and SurrealKV's
//! rollback walks every key the transaction has written, so the copy's one transaction, an event a
//! row, took time quadratic in its rows (the board of 2026-11-04: 17 min for that transaction; the
//! whole copy now takes 20 s).
//! Per-recipient `callback_lease` rows retain their revision and periodic PULL checkpoint across
//! callback retirement; an event key must never inherit an earlier holder's revision.
//!
//! What a write, each recipient of its dispatch and each tick read is found through an index, never a
//! scan of the table (the board crate's `perf` tests plan every one): messages by `sid` (a cursor's
//! next), by `ts` (an inbox since its session connected), by recipient then `sid` (the owner's
//! newest); deliveries by handle then message; roles by role; registrations by address. `said:⟨sender⟩`
//! names each sender's newest `msg` or verdict, the greatest `[sid, slug]` (a copy takes one `sid` on
//! two threads), and `DEFINE` fills it once when it holds nothing. An event keeps it as messages are
//! made, deleted or edited: a message made, or edited into a sender's word, is that sender's word if
//! no greater one is; a word deleted, or edited to another sender or out of the words, gives way to
//! the sender's word before it, read back along `sid`. The board makes messages only: it deletes them
//! all at once, in a copy, which empties `said` first so no deletion looks back, and edits none, so
//! that read back is only ever a hand's at the server. These cost the copy's one transaction
//! less than the machine's load moves it: the live board's 49k messages and 36k deliveries swapped
//! in 123 s without them and 97 s with them, one after the other at load 140 (3.3.0, 2026-11-07);
//! defining them on that board took 15 s.
//!
//! `moved:sqlite` is the move from SQLite (`Store::move_in`): the board it moved, when, and a hash of
//! every row the move left, so a move into a board written since is refused.
//!
//! `meta:schema` names the version defined, so opening a board already in this shape writes nothing.

/// Bumped when `DEFINE` gains a table, field, index or event: a board at another version runs `DEFINE`
/// again, and its `IF NOT EXISTS` adds what is new but leaves every definition already there as it
/// was. Changing one (a field's type, an assertion, an index) also needs `DEFINE … OVERWRITE` for it.
pub const VERSION: i64 = 13;

// Full-text index construction needs a committed analyzer, installed before the schema transaction.
pub const ANALYZERS: &str = "DEFINE ANALYZER IF NOT EXISTS task_words TOKENIZERS blank, class, punct FILTERS lowercase, ascii;";

pub const DEFINE: &str = "
DEFINE TABLE IF NOT EXISTS task SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS sid ON task TYPE int;
DEFINE FIELD IF NOT EXISTS slug ON task TYPE string;
DEFINE FIELD IF NOT EXISTS title ON task TYPE string;
DEFINE FIELD IF NOT EXISTS body ON task TYPE string;
DEFINE FIELD IF NOT EXISTS status ON task TYPE string
    ASSERT $value IN ['backlog', 'concept', 'design', 'ready', 'in_progress', 'review', 'done', 'blocked', 'cancelled'];
DEFINE FIELD IF NOT EXISTS priority ON task TYPE int;
DEFINE FIELD IF NOT EXISTS parent ON task TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_by ON task TYPE string;
DEFINE FIELD IF NOT EXISTS created ON task TYPE float;
DEFINE FIELD IF NOT EXISTS updated ON task TYPE float;
DEFINE FIELD IF NOT EXISTS instruction ON task TYPE bool DEFAULT false;
DEFINE FIELD IF NOT EXISTS version ON task TYPE int DEFAULT 0;
DEFINE FIELD IF NOT EXISTS fields ON task TYPE option<object> FLEXIBLE;
DEFINE INDEX IF NOT EXISTS task_sid ON task FIELDS sid UNIQUE;
DEFINE INDEX IF NOT EXISTS task_status ON task FIELDS status;
DEFINE INDEX IF NOT EXISTS task_parent ON task FIELDS parent;
DEFINE INDEX IF NOT EXISTS task_slug ON task FIELDS slug UNIQUE;
DEFINE INDEX IF NOT EXISTS task_title_search ON task FIELDS title FULLTEXT ANALYZER task_words BM25;
DEFINE INDEX IF NOT EXISTS task_body_search ON task FIELDS body FULLTEXT ANALYZER task_words BM25;
DEFINE INDEX IF NOT EXISTS task_slug_search ON task FIELDS slug FULLTEXT ANALYZER task_words BM25;

DEFINE TABLE IF NOT EXISTS task_embedding_source SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS slug ON task_embedding_source TYPE string;
DEFINE FIELD IF NOT EXISTS source ON task_embedding_source TYPE string;
DEFINE FIELD IF NOT EXISTS source_hash ON task_embedding_source TYPE string;
DEFINE FIELD IF NOT EXISTS model ON task_embedding_source TYPE string;
DEFINE FIELD IF NOT EXISTS parts ON task_embedding_source TYPE int;
DEFINE TABLE IF NOT EXISTS task_embedding_checkpoint SCHEMALESS;
DEFINE TABLE IF NOT EXISTS task_chunk SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS slug ON task_chunk TYPE string;
DEFINE FIELD IF NOT EXISTS source ON task_chunk TYPE string;
DEFINE FIELD IF NOT EXISTS source_hash ON task_chunk TYPE string;
DEFINE FIELD IF NOT EXISTS text_hash ON task_chunk TYPE string;
DEFINE FIELD IF NOT EXISTS model ON task_chunk TYPE string;
DEFINE FIELD IF NOT EXISTS text ON task_chunk TYPE string;
DEFINE FIELD IF NOT EXISTS embedding ON task_chunk TYPE array<float, 384>;
DEFINE INDEX IF NOT EXISTS task_chunk_source ON task_chunk FIELDS slug, source;
DEFINE INDEX IF NOT EXISTS task_embedding ON task_chunk FIELDS embedding HNSW DIMENSION 384 DIST COSINE TYPE F32;

DEFINE TABLE IF NOT EXISTS link SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS slug ON link TYPE string;
DEFINE FIELD IF NOT EXISTS kind ON link TYPE string ASSERT $value IN ['doc', 'dep'];
DEFINE FIELD IF NOT EXISTS target ON link TYPE string;
DEFINE INDEX IF NOT EXISTS link_slug ON link FIELDS slug;

DEFINE TABLE IF NOT EXISTS session SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS handle ON session TYPE string;
DEFINE FIELD IF NOT EXISTS kind ON session TYPE string;
DEFINE FIELD IF NOT EXISTS parent ON session TYPE option<string>;
DEFINE FIELD IF NOT EXISTS worktree ON session TYPE string;
DEFINE FIELD IF NOT EXISTS branch ON session TYPE string;
DEFINE FIELD IF NOT EXISTS model ON session TYPE string;
DEFINE FIELD IF NOT EXISTS pid ON session TYPE option<int>;
DEFINE FIELD IF NOT EXISTS host ON session TYPE string;
DEFINE FIELD IF NOT EXISTS affinity ON session TYPE string;
DEFINE FIELD IF NOT EXISTS connected ON session TYPE float;
DEFINE FIELD IF NOT EXISTS last_seen ON session TYPE float;
DEFINE FIELD IF NOT EXISTS left_at ON session TYPE option<float>;
DEFINE FIELD IF NOT EXISTS agent ON session TYPE option<string>;
DEFINE FIELD IF NOT EXISTS busy_until ON session TYPE option<float>;
DEFINE FIELD IF NOT EXISTS busy_call ON session TYPE option<string>;
DEFINE FIELD IF NOT EXISTS last_tool ON session TYPE option<float>;
DEFINE INDEX IF NOT EXISTS session_agent ON session FIELDS agent;

DEFINE TABLE IF NOT EXISTS role SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS slug ON role TYPE string;
DEFINE FIELD IF NOT EXISTS handle ON role TYPE string;
DEFINE FIELD IF NOT EXISTS role ON role TYPE string ASSERT $value IN ['orchestrator', 'executor', 'reviewer', 'reserved'];
DEFINE FIELD IF NOT EXISTS since ON role TYPE float;
DEFINE FIELD IF NOT EXISTS expires ON role TYPE option<float>;
DEFINE INDEX IF NOT EXISTS role_slug ON role FIELDS slug;
DEFINE INDEX IF NOT EXISTS role_handle ON role FIELDS handle;
DEFINE INDEX IF NOT EXISTS role_role ON role FIELDS role;

DEFINE TABLE IF NOT EXISTS message SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS sid ON message TYPE int;
DEFINE FIELD IF NOT EXISTS slug ON message TYPE string;
DEFINE FIELD IF NOT EXISTS sender ON message TYPE string;
DEFINE FIELD IF NOT EXISTS recipient ON message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS kind ON message TYPE string ASSERT $value IN ['system', 'msg', 'verdict'];
DEFINE FIELD IF NOT EXISTS text ON message TYPE string;
DEFINE FIELD IF NOT EXISTS ts ON message TYPE float;
DEFINE INDEX IF NOT EXISTS message_text_search ON message FIELDS text FULLTEXT ANALYZER task_words BM25;
DEFINE INDEX IF NOT EXISTS message_sid ON message FIELDS sid;
DEFINE INDEX IF NOT EXISTS message_ts ON message FIELDS ts;
DEFINE INDEX IF NOT EXISTS message_recipient ON message FIELDS recipient, sid;

DEFINE TABLE IF NOT EXISTS said SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS sid ON said TYPE int;
DEFINE FIELD IF NOT EXISTS slug ON said TYPE string;

DEFINE TABLE IF NOT EXISTS delivery SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS message ON delivery TYPE int;
DEFINE FIELD IF NOT EXISTS handle ON delivery TYPE string;
DEFINE FIELD IF NOT EXISTS acked ON delivery TYPE float;
DEFINE INDEX IF NOT EXISTS delivery_handle ON delivery FIELDS handle, message;

DEFINE TABLE IF NOT EXISTS delivery_key SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS slug ON delivery_key TYPE string;
DEFINE FIELD IF NOT EXISTS key ON delivery_key TYPE string;
DEFINE FIELD IF NOT EXISTS message ON delivery_key TYPE int;
DEFINE FIELD IF NOT EXISTS ts ON delivery_key TYPE float;

DEFINE TABLE IF NOT EXISTS barred_key SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS key ON barred_key TYPE string;

DEFINE TABLE IF NOT EXISTS task_session SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS slug ON task_session TYPE string;
DEFINE FIELD IF NOT EXISTS handle ON task_session TYPE string;
DEFINE FIELD IF NOT EXISTS role ON task_session TYPE string;
DEFINE FIELD IF NOT EXISTS claude_session ON task_session TYPE string;
DEFINE FIELD IF NOT EXISTS worktree ON task_session TYPE string;
DEFINE FIELD IF NOT EXISTS branch ON task_session TYPE string;
DEFINE FIELD IF NOT EXISTS pane ON task_session TYPE string;
DEFINE FIELD IF NOT EXISTS model ON task_session TYPE string;
DEFINE FIELD IF NOT EXISTS started ON task_session TYPE float;
DEFINE FIELD IF NOT EXISTS closed ON task_session TYPE option<float>;
DEFINE FIELD IF NOT EXISTS closed_why ON task_session TYPE string;
DEFINE FIELD IF NOT EXISTS restore ON task_session TYPE string;

DEFINE TABLE IF NOT EXISTS counter SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS last ON counter TYPE int;
DEFINE FIELD IF NOT EXISTS ts ON counter TYPE option<float>;

DEFINE TABLE IF NOT EXISTS moved SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS board ON moved TYPE string;
DEFINE FIELD IF NOT EXISTS at ON moved TYPE float;
DEFINE FIELD IF NOT EXISTS rows ON moved TYPE string;

DEFINE TABLE IF NOT EXISTS meta SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS version ON meta TYPE int;

DEFINE TABLE IF NOT EXISTS callback_state SCHEMALESS;
DEFINE INDEX IF NOT EXISTS callback_state_address ON callback_state FIELDS address;
DEFINE TABLE IF NOT EXISTS callback_lease SCHEMALESS;
DEFINE TABLE IF NOT EXISTS callback_event SCHEMALESS;
DEFINE INDEX IF NOT EXISTS callback_event_handle ON callback_event FIELDS handle, delivered;
DEFINE TABLE IF NOT EXISTS chat_event SCHEMALESS;

DEFINE EVENT OVERWRITE board_changed ON task WHEN $before != $after THEN { IF record::exists(counter:board) { UPDATE counter:board SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board SET last = 1 RETURN NONE; }; };
DEFINE EVENT OVERWRITE board_changed ON session WHEN $before != $after THEN { IF record::exists(counter:board) { UPDATE counter:board SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board SET last = 1 RETURN NONE; }; };
DEFINE EVENT OVERWRITE board_changed ON role WHEN $before != $after THEN { IF record::exists(counter:board) { UPDATE counter:board SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board SET last = 1 RETURN NONE; }; };
DEFINE EVENT OVERWRITE board_changed ON link WHEN $before != $after THEN { IF record::exists(counter:board) { UPDATE counter:board SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board SET last = 1 RETURN NONE; }; };
DEFINE EVENT OVERWRITE board_changed ON message WHEN $before != $after THEN { IF record::exists(counter:board) { UPDATE counter:board SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board SET last = 1 RETURN NONE; }; };
DEFINE EVENT OVERWRITE board_changed ON delivery WHEN $before != $after THEN { IF record::exists(counter:board) { UPDATE counter:board SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board SET last = 1 RETURN NONE; }; };
DEFINE EVENT OVERWRITE board_changed ON task_session WHEN $before != $after THEN { IF record::exists(counter:board) { UPDATE counter:board SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board SET last = 1 RETURN NONE; }; };
DEFINE EVENT OVERWRITE board_changed ON callback_state
    WHEN [$before.address, $before.owner, $before.host, $before.chat, $before.chat_fallback, $before.chat_resume, $before.confirmed_until, $before.in_process, $before.seen, $before.pull, $before.registration_revision]
      != [$after.address, $after.owner, $after.host, $after.chat, $after.chat_fallback, $after.chat_resume, $after.confirmed_until, $after.in_process, $after.seen, $after.pull, $after.registration_revision]
    THEN { IF record::exists(counter:board) { UPDATE counter:board SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board SET last = 1 RETURN NONE; }; };
DEFINE EVENT OVERWRITE board_changed ON chat_event WHEN $before != $after THEN { IF record::exists(counter:board) { UPDATE counter:board SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board SET last = 1 RETURN NONE; }; };

DEFINE EVENT OVERWRITE said ON message
    WHEN $event != 'UPDATE' OR [$before.sender, $before.sid, $before.slug, $before.kind IN ['msg', 'verdict']]
        != [$after.sender, $after.sid, $after.slug, $after.kind IN ['msg', 'verdict']]
    THEN {
    IF $event != 'CREATE' {
        LET $was = (SELECT sid, slug FROM ONLY type::record('said', $before.sender));
        IF $was.sid = $before.sid AND $was.slug = $before.slug {
            LET $top = (SELECT VALUE sid FROM message WHERE sender = $before.sender AND kind IN ['msg', 'verdict'] ORDER BY sid DESC LIMIT 1);
            LET $newest = array::max((SELECT VALUE [sid, slug] FROM message WHERE sid IN $top AND sender = $before.sender AND kind IN ['msg', 'verdict']));
            IF $newest = NONE {
                DELETE type::record('said', $before.sender) RETURN NONE;
            } ELSE {
                UPDATE type::record('said', $before.sender) SET sid = $newest[0], slug = $newest[1] RETURN NONE;
            };
        };
    };
    IF $event != 'DELETE' AND $after.kind IN ['msg', 'verdict'] {
        IF record::exists(type::record('said', $after.sender)) {
            UPDATE type::record('said', $after.sender) SET sid = $after.sid, slug = $after.slug
                WHERE sid < $after.sid OR (sid = $after.sid AND slug < $after.slug) RETURN NONE;
        } ELSE {
            CREATE type::record('said', $after.sender) SET sid = $after.sid, slug = $after.slug RETURN NONE;
        };
    };
};
IF (SELECT VALUE id FROM said LIMIT 1) = [] {
    FOR $spoke IN (SELECT sender, array::max(array::group([[sid, slug]])) AS last FROM message WHERE kind IN ['msg', 'verdict'] GROUP BY sender) {
        LET $newest = $spoke.last[0];
        CREATE type::record('said', $spoke.sender) SET sid = $newest[0], slug = $newest[1] RETURN NONE;
    };
};
";
