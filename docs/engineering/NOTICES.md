# Notices

A notice reaches a person outside the page through a notice plugin: a program of its own, one for
each service, which the server hands one notice at a time, the person's notice by the way they
chose and their address for it. The core makes the message and sends nothing; the plugins send
([users and identity](../product/analysis/users-and-identity.md#what-the-symphony-ships-for-notices)).
This page is the message and its hand-off, a contract kept stable across releases, so a plugin
built on it keeps working. A program in any language that reads this page takes the message; the
Rust form is `crates/symphony-notice`.

Nothing makes notices yet: who is told, of what and by which way is step 4 of
[the way from one owner](../product/analysis/users-and-identity.md#the-way-from-one-owner), and so
is the server's part of the hand-off, its settings naming each way's program among it.

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
a plugin that cannot send to it says so ([the hand-off](#the-hand-off)).

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
own, built with it.

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
- `unstarted` says what came of a notice whose sending program did not start: `75` where the
  machine ran short for a moment (of processes, memory or open files, or the program being
  written as it was started), else `1`.
- `machine_folder` finds the machine folder, where a plugin keeps what it needs, by the server's
  rule: the folder `SYMPHONY_MACHINE_DIR` (`MACHINE_DIR`) names, else the platform's data folder
  for the Symphony. It repeats the board's rule rather than linking it; its tests, the one place
  the crate builds with the board, hold the two the same.
