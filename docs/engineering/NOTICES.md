# Notices

A notice reaches a person outside the page through a notice plugin: a program of its own, one for
each service, which the server hands one notice at a time, the person's notice by the way they
chose and their address for it. The core makes the message and sends nothing; the plugins send
([users and identity](../product/analysis/users-and-identity.md#what-the-symphony-ships-for-notices)).
This page is the message and its hand-off, a contract kept stable across releases, so a plugin
built on it keeps working. A program in any language that reads this page takes the message; the
Rust form is `crates/symphony-notice`.

A person's profile holds their address for each way and the way each kind of notice goes by
([TASKS](../workflow/TASKS.md#people)). Who is told, of what, and when a notice is handed again is
[the server's part](#the-servers-part), its programs named by the machine, not a project.

## The message

One JSON object, written on one line:

```json
{
  "notice": 1,
  "id": "0193f1c2-7a4e-7b3c-9d2e-5f6a7b8c9d0e",
  "kind": "card",
  "project": "symphony",
  "link": "https://mac.example.ts.net/symphony/cards#notice-sender",
  "to": { "way": "email", "address": "ana@example.com" }
}
```

| Field | What it is | Its rule |
|---|---|---|
| `notice` | the format's number | `1` |
| `id` | the notice's own id, the same on every hand of it and unique on the server ([below](#the-same-notice)) | 1 to 128 ASCII letters, digits, dots, dashes and underscores |
| `kind` | what it tells of | `card`, `question` or `verdict` ([kinds](#kinds)) |
| `project` | the project's id, as in its `/<id>/` path | lowercase letters and digits, single dashes between them: `[project] id`'s rule |
| `link` | the page it is about | an absolute `http` or `https` URL with a host, all printable ASCII |
| `to.way` | the way the person chose | a word: lowercase letters and digits, single dashes between them ([ways](#ways-and-addresses)) |
| `to.address` | their address for that way, as their profile holds it | not empty, no control character, no space at either end |

`notice` is a number, `to` an object, and every other field text.

**A link alone.** A notice carries no card title, no question and no words of a thread, and no
name: what a notice carries outside the Symphony, a link alone or the words it is about, is a
[later round's](../product/analysis/users-and-identity.md#later-rounds) card. The kind is there all
the same because a sender needs one: WhatsApp opens a conversation only with a template Meta has
approved, one for each kind of notice, and an email needs a subject.

### Kinds

The three notices the Symphony gives today, which the Mac app shows:

- `card`: a card waits on the person.
- `question`: an agent's question on a task waits on them.
- `verdict`: a reviewer gave a verdict.

A kind is added without a new format: a reader that does not know one sends it as a notice that
something on the Symphony waits on the person, at its link (`Kind::Other` in the crate, which no
notice is made of).

### Ways and addresses

- `email`: an email address.
- `slack`: the person's Slack member id (`U024BE7LH`).
- `whatsapp`: the person's number in international form, its country code after a `+`
  (`+15551234567`).

Another way is another word, with no new format. The address is written as the profile holds it;
a plugin that cannot send to it says so ([the hand-off](#the-hand-off)). A profile holds an
address of these three ways by the rule its plugin sends by (`ways` in [the crate](#the-crate)),
so the server hands none a plugin refuses.

## The same notice

Two hands carry the same notice when they carry the same id. The core gives an id to one notice
of one thing, to one person's address by one way, and hands it again, after a `75`
([below](#the-hand-off)), with that id and every other field unchanged. Anything told again is a
new notice with an id of its own: the same card waiting on the person again after they answered it,
or the same notice to the same person by another way.

**An id is unique across every project the server serves, and never given to another notice**, as
one plugin program takes every project's notices. A random id, such as the UUID in the example
above, gives that; an id made from the thing, the person and the way does not, as it would come
back when the same card waits on the person again. So a plugin keys on the id alone: one that keeps
the ids it sent can answer a hand of one of them as sent, sending nothing, and one whose service
takes an idempotency key can pass the id as that key.

## The hand-off

- **One run for each notice.** The server starts the program of the notice's way with no
  arguments, writes the message on its stdin, one line and its newline, and closes it.
- **On stdin alone.** The message never goes in an argument or the environment, where another
  process on the machine reads it.
- **The program keeps its own secret**: a Slack app's token, a WhatsApp business account's token, a
  mail server's password, from a file or settings of its own. The server hands it none.
- **Its exit status says what came of it:**
  - `0`: sent.
  - `75` (sysexits' `EX_TEMPFAIL`): not sent this time, worth handing again. The service did not
    answer or asked to be called later, or the program could not read its stdin at all, so
    nothing was read to send.
  - any other: not sent, and handing it again will not send it. The service refused the address or
    the message, or the message the program read is not one it takes.
- **Stderr says why** a notice was not sent. On a sent one, the program writes nothing there.
- **A message the program read and does not take is never sent**: one that is no JSON object, one
  of a format it does not read, one missing a field or holding one of another type, one that breaks
  a rule above. It exits `1`, saying why.

When the server hands a notice again after a `75`, how often, and what it does with stderr are its
own: [the server's part](#the-servers-part).

## The server's part

The machine's one server makes the notices of every project it serves; a server of a linked
worktree or a preview makes none, so nothing is told twice. Its module doc is
`apps/symphony-server/src/notices.rs`.

- **What tells.** A card starting to wait on the main page's queue, an agent's question on a task
  to the person at the machine (a thread line to `owner` that no person wrote), and a reviewer's
  verdict. What stood when the server first looked at a project is taken as told.
- **Who is told.** Whose the thing is, by the board's map of places and the name written on it
  ([TASKS](../workflow/TASKS.md#which-group-a-thing-belongs-to)): each member of its group, or the
  person it is written to. The person at the machine has no profile, so a board that names nobody
  tells no one.
- **By which way.** The way the person's profile chose for that kind, at their address for it. A
  person who chose none is not told, nor one whose way the machine names no program for.
- **Once.** A card waiting tells each person once while it waits, and again only after it stopped
  waiting, or when it moves to someone not told yet; a line tells once.
- **The programs are the machine's**: `notices/programs.toml` in the machine folder, each way named
  to its program's absolute path. No `symphony.toml` key names one, so nothing a project or its
  board holds starts a program. The server refuses the folder or the file when another account
  owns it, when another may write it, or when a link stands in its place: it runs no program from
  it then, says so once, and sends nothing until it is mended.

  ```toml
  slack = "/Users/ana/.cargo/bin/symphony-notice-slack"
  email = "/Users/ana/.cargo/bin/symphony-notice-email"
  whatsapp = "/Users/ana/.cargo/bin/symphony-notice-whatsapp"
  ```

- **Handed again** after a `75`: a minute after, then twice as long each time, at most an hour,
  eight hands in all; after the eighth it is dropped, said. Any other status is said once and the
  notice dropped. A program that does not end within 60 s, or whose end the server cannot watch,
  is ended with its whole process group, as a `75`. A notice whose person's way or address for it changed since it was made, or whose way
  has no program now, is dropped unhanded.
- **Through a restart.** The project's record is `notices.json` in its artifacts folder: the newest
  board line looked at, who was told of each card while it waits, and each notice still to hand
  with its id, its person, its way and a digest of their address, never the address. The digest
  is keyed by the machine's own secret, `notices/record.key` in the machine folder, made once and
  its owner's alone, so the record says nothing of an address, a phone number's among them, to
  whoever reads it without the key. A hand is written there before its program starts and settled
  after, so a notice handed when the server stopped is handed again, with the same id, and a
  restart in a notice's wait hands it once, when it is due.
- **Its link** is the page under the project, `http://NAME:PORT/<id>/…`, by the machine's tailnet
  name (`127.0.0.1` without one).
- **What it says.** Each outcome is a `NOTICE` line in the server's log, by the notice's id, kind,
  person and way; what a program said on stderr is repeated there with the address and the link
  hidden. No address is in the log, the record or a refusal.

## What a plugin must not do

- **Log the address or the link.** Neither goes to a log, a file or stderr. When a notice is not
  sent, the program says why without them. In the crate a printed `Notice` shows neither, and a
  refusal names the field and its rule, never the value it found there.
- **Pass the message on** to anything but the service it sends by, or keep it beyond the ids it
  sent.
- **Decide anything.** It is handed one notice for one address and sends one message there. It is
  handed no list of people and asks for none: the board decides who is told, and a person's
  preferences by which way.
- **Refuse what it does not know.** A field it does not know is ignored, wherever it stands, and a
  kind it does not know is sent as above.

## Versions

The format is `1`, and the exit statuses are part of it.

- **A reader ignores a field it does not know**, wherever it stands, so what a newer server adds
  reaches an older plugin unread.
- **A new field comes with a default**, which a newer reader gives a message without it, and is no
  new format. So is a new kind or a new way.
- **Anything else is a new format**: a field removed or renamed, a rule narrowed, a meaning or an
  exit status changed. It is `2`, and a reader of `1` refuses it by name, saying to take a build
  of it that reads `2`.

## The crate

`symphony-notice` builds with no workspace member, so the core and each plugin link it alone of
each other. It is not published: the plugins are this workspace's, and a program elsewhere reads
this page. In it:

- `Notice::new` checks every field when the notice is made, refusing `Kind::Other`. `Notice::line`
  writes the message.
- `Notice::read` reads a message by the rules above, `to` as an object and every other field as
  text, and its refusal names the field and its rule, never the value found. `To` is the way and
  the address, and `Kind::of` reads a kind's word.
- `take(send)` is a plugin's `main`. It reads the notice on stdin and hands it to `send`, and
  answers the exit status: `0` when `send` answers `Ok`, `75` for `Unsent::Later(why)`, `1` for
  `Unsent::Never(why)` or a message it cannot read, `why` on stderr. A stdin that cannot be read at
  all is a `75`, as nothing was read to send.
- `EMAIL`, `SLACK`, `WHATSAPP`, `SENT`, `LATER`, `NEVER` and `FORMAT` name the words and numbers
  above.
- `ways` is the address each way takes: `email`, `slack` and `whatsapp` check one (the last
  answering the number as `+` and its digits), `address(way, text)` answers it as a profile holds
  it, each refusal saying the rule and never the address, and `WAYS` and `KINDS` list the words. The
  three plugins send by it, and the board, which is published where this crate is not, carries the
  file as a copy (`crates/tasks/src/people/ways.rs`) that `crates/symphony/tests/packages.rs` holds
  equal byte for byte, so a profile holds no address its plugin refuses.
- `unstarted` says what came of a notice whose sending program did not start: `75` where the
  machine ran short for a moment (of processes, memory or open files, or the program being
  written as it was started), else `1`.
- `machine_folder` finds the machine folder, where a plugin keeps what it needs, by the server's
  rule: the folder `SYMPHONY_MACHINE_DIR` (`MACHINE_DIR`) names, else the platform's data folder
  for the Symphony. It repeats the board's rule rather than linking it; its tests, the one place
  the crate builds with the board, hold the two the same.
