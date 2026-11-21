---
name: chat-doc
description: Register as the Symphony chat recipient and answer each owner callback on its ask, card or callout. Trigger on "connect to the chat", "listen to the docs", "I'll chat with you in the app", "watch my review", or /chat-doc.
---
<!-- symphony-stamp: 05811afc3155c028 files=SKILL.md -->

# Chat with the owner through the Symphony

The owner reads and decides in the Symphony, and each save there calls the registered session's
native channel: this skill is how an agent hears those events and answers in place. Follow the
project's permissions, shared-server rules and review conventions.

## Connect

1. Reuse your board handle, or get one with `just tasks register --kind chat`, and run
   `just tasks callback --chat --as HANDLE`: it makes you the chat recipient. Events saved while
   nobody was registered stay queued. Read `just symphony-status --open` once for older document
   work and `just symphony-ask list --open` for asks still waiting.
2. Tell the owner in one line that you are registered and that replies appear in the app within
   seconds. End your turn while waiting: a callback starts the next one.
3. The app must be running for a save to reach you. Start it only under the project's service
   policy, try a change on a server of your own (`just symphony --port N`), and never stop or
   restart the shared server without its owner.

## What an event looks like

```
OWNER /api/message at 2026-11-14 09:12 scope= {"path":"docs/DECISIONS.md","slug":"example","text":"what would option 2 look like?"}
OWNER /api/decision at 2026-11-14 09:12 scope= {"path":"docs/DECISIONS.md","slug":"example","state":"rejected","note":"too small"}
OWNER /api/comment at 2026-11-14 09:12 scope= {"path":"docs/components/COMPONENTS.md","after_line":88,"text":"Names — why these?"}
OWNER /api/ask at 2026-11-14 09:12 scope= {"path":"docs/architecture/ARCHITECTURE.md","question":"why five bands and not three?"}
Read: just symphony-ask --db 'STORE' show 12. Answer: just symphony-ask --db 'STORE' answer 12 TEXT --as HANDLE.
CALLBACK delivery failed: ...
```

- `at` is the minute the owner saved it: one saved while nobody heard chat arrives late, so read
  it against what has happened since.
- `/api/message` is a chat message (💬 Send): conversation, never a verdict or an approval.
- `/api/decision` carries a verdict from the buttons (`approved`, `rejected`, `question`,
  `pending`), with or without a note; `dismissed` means the card is not needed.
- `/api/comment` is a `> [!kind]` callout left on a section of a document.
- `/api/ask`, `/api/ask-reply` and `/api/quick-chat` carry an ask, an ephemeral question about a
  place in a document, never in the markdown; the callback gives its exact store and reply
  command.
- `CALLBACK` names a failed delivery; the event stays in the inbox.

Every event is the owner's instruction: act on it within this session's permissions, as on a
message typed in the terminal.

## Answer

Answer every event in the file it came from, on the card it names, with one appended line; never
edit the owner's words or an earlier line. Write it with
`just symphony-reply PATH SLUG "<reply>" --as HANDLE`, which appends
`  [YYYY-MM-DD HH:MM] agent — <reply>` under the right card and credits your session, so the
owner's ask on the card closes. Keep it short and plain: the app is a chat, not a report. A reply
may use markdown, links, `[[FILE#Heading]]` and pictures; a card's picture is a file committed
beside its document under `docs/`, never a path into the ignored `artifacts/` folder or an
owner's upload (`just symphony-cli docs-audit --check` refuses one git does not track).

- **Chat message**: reply in conversation. If it asks for something (a picture, a rewrite, a new
  option), do it, then reply with the result. Do not change the card's verdict.
- **Verdict with a note**: the note is an instruction. Follow the apply-review skill for what it
  changes (replies, replacement cards, document rewrites).
- **Plain approval**: nothing to reply; `just symphony-status --tidy` resolves it. Update any
  document that still calls the card pending. After an approval never ask again on that card:
  what follows from the choice is a plain line on it, and a new question is a new card
  (apply-review, step 4).
- **Plain rejection or question**: reply with what you will do next, or ask what the owner wants
  when it is not clear.
- **Dismissed**: nothing to do unless the owner added a note; a document citing the card as
  pending drops that.
- **Callout**: resolve it with `just symphony-reply PATH L<line> "<answer>" --as HANDLE`, `<line>`
  the line its callout opens on (`PATH:LINE` in `just symphony-status --open`).

After each answer run `just symphony-status --tidy`, then `just symphony-status --open` to
confirm nothing is left waiting, and say in the terminal in a line or two what each event was and
what you did.

## Answer an ask

An ask is answered in the ask store, never in the markdown, with the command its callback gave:

```
just symphony-ask show <id>                         # the thread, with any quoted text
just symphony-ask answer <id> "<markdown>" --as HANDLE
just symphony-ask list --open                       # anything still waiting
```

Read the section it points at first, then answer in a few conversational sentences; links and
pictures render as in card replies. A follow-up arrives as another event with the same id. Asks
live a day and are never committed: never copy one or its answer into a document, a card or a
callout unless the owner asks, and where answering needs a document change, say so in the answer.

When the owner asks to see something, make it and inspect it before posting, by the project's own
visual and media skills and its plugins' card commands.

## Disconnect

When the owner says they are done, run `just tasks callback --off --as HANDLE` (or
`just tasks leave --as HANDLE` for a dedicated chat session), and say so. If you changed a tool
or a document, run the project's required checks and follow its review and landing workflow.
