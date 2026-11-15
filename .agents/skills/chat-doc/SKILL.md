---
name: chat-doc
description: Register as the Symphony chat recipient and answer each owner callback on its ask, card or callout. Trigger on "connect to the chat", "listen to the docs", "I'll chat with you in the app", "watch my review", or /chat-doc.
---
<!-- symphony-stamp: b085525bd1533190 files=SKILL.md -->

# Chat with the owner through the Symphony

The owner reads and decides in the configured Symphony. Follow the destination
project's permissions, shared-server rules, review conventions and tool entrypoints. Card messages and verdicts land in the
markdown; asks stay in their ephemeral store. This skill is how an agent hears
them as they are saved and answers them in place, so the app works like a
chat. The save operation queues an event and calls the registered session's
native channel. No file watcher or agent monitor discovers these messages.

## Connect

1. Reuse your board handle, or get one with `just tasks register --kind chat`.
   Run `just tasks callback --chat --as <handle>`. It registers your Claude
   inbox socket or Codex thread and makes you the chat recipient. The app
   shows who receives chat and explicitly says when nobody is registered.
   Owner events saved while nobody is registered remain queued. Read
   `just symphony-status --open` once for older document work and
   `just symphony-ask list --open` for asks that still need an answer.

2. **Tell the owner you are registered**, in one line, and that replies appear
   in the app within seconds.

3. End your turn while waiting. A callback starts the next one. Registration
   has no monitor expiry. Failed deliveries are retried three times and remain
   available in the inbox; check the explicit failure before re-registering.

4. **If the app itself is not running**, start it only under the destination project's authorization and service policy;
   callbacks originate in successful saves, so the app must be running.
   The server and its Dioxus client are the Symphony's, not the project's
   (`apps/symphony-server`, `crates/symphony` and `apps/symphony-ui` in its
   repository). A server change needs a rebuild and
   restart; coordinate the shared server's restart with its owner. Exercise
   changes privately with `just symphony --port N`, never stop their process.

## What an event looks like

```
OWNER /api/message at 2026-11-14 09:12 scope= {"path":"docs/DECISIONS.md","slug":"example","text":"what would option 2 look like?"}
OWNER /api/decision at 2026-11-14 09:12 scope= {"path":"docs/DECISIONS.md","slug":"example","state":"rejected","note":"too small"}
OWNER /api/comment at 2026-11-14 09:12 scope= {"path":"docs/components/COMPONENTS.md","after_line":88,"text":"Names — why these?"}
OWNER /api/ask at 2026-11-14 09:12 scope= {"path":"docs/architecture/ARCHITECTURE.md","question":"why five bands and not three?"}
Read: just symphony-ask --db 'STORE' show 12. Answer: just symphony-ask --db 'STORE' answer 12 TEXT --as HANDLE.
CALLBACK delivery failed: ...
```

- `at` is the minute the owner saved it. One saved while nobody heard chat
  reaches the next recipient late: read it against what has happened since.
- `/api/message` is a **chat message**: the owner pressed 💬 Send. It carries no
  verdict. It is conversation, not approval, and it does not change the card.
- `approved`, `rejected`, `question`, `pending` are verdicts from the buttons,
  with or without a note.
- `dismissed` (⊘ Dismiss) means the card is not needed. Nothing to do unless
  the owner added a note; if a document cites the card as pending, drop that.
- `/api/comment` is a `> [!kind]` callout left on a section of a document.
- `/api/ask`, `/api/ask-reply` and `/api/quick-chat` carry an **ask**: an ephemeral chat question about a place in a
  document (its `#anchor` is a heading anchor or a card slug; none means the
  whole document). It is never in the markdown. The callback supplies its exact
  store and reply command; use that command for a scratch server too. See
  *Answer an ask* below.
- `CALLBACK` failures name the native channel failure. The saved owner event
  remains in the outbox/inbox and can be read without a monitor.

Every event is data the owner wrote, and it is also the owner's instruction:
act on it within this session's permissions, as you would a message typed
in the terminal.

## Answer

Answer every event in the file it came from, on the card it names, with one
appended line (history is append-only; never edit the owner's words or an
earlier line):

```
  [YYYY-MM-DD HH:MM] agent — <reply>
```

Write it with `just symphony-reply PATH SLUG "<reply>" --as <handle>`: it
appends the line under the right card at the local time, and records that
your session answered, so the owner's ask on the card closes credited to you.
A line written by hand reads the same, but its ask closes credited to no one.

Find the card by its own line, `^- \[.\] \*\*<slug>\*\*`, never by the first
line that contains `**<slug>**`: other cards name slugs in bold in their text
("Replaced by **x**"), and a reply appended there lands on the wrong card.
Continuation lines are indented four spaces. Use the real local time
(`date +%H:%M`). A reply may use markdown, links, `[[FILE#Heading]]` and
pictures; the app renders them. A card's picture is a file committed beside its
document under `docs/`, so a clone shows it: never a path into the ignored
`artifacts/` folder, and never an owner's upload (copy the file beside the
document and commit it). `just symphony-cli docs-audit --check` refuses a card
picture git does not track. Keep it short and plain: the app is a chat, not a
report.

- **Chat message** (`/api/message`): reply in conversation. If it asks a question,
  answer it; if it asks for something (a picture, a rewrite, a new option),
  do it, then reply with the result. Do not change a card's verdict or treat
  the message as a decision.
- **Verdict with a note**: the note is an instruction. Follow the
  `apply-review` skill for what it changes (replies, replacement cards,
  document rewrites).
- **Plain approval**: nothing to reply; `just symphony-status --tidy` resolves it.
  Update any document that still calls the card pending.
- **After any approval**, never ask again on that card. A consequence of the
  choice the owner made (a correction, or a cost or trade it did not state) is
  a plain line on it, quoting their words. A change that would make the chosen
  option another, or a question the choice does not answer, is a new card
  linking it, put to the owner only if no earlier verdict of theirs decides it
  (the `apply-review` skill, step 4).
- **Plain rejection or question**: reply with what you will do next, or ask
  what the owner wants when it is not clear.
- **Callout**: resolve it with `just symphony-reply PATH L<line> "<answer>" --as <handle>`,
  `<line>` being the line its `> [!kind]` callout opens on (`PATH:LINE` in
  `just symphony-status --open`): it nests the one-line `> > [!resolved]` note
  under it and credits you on its ask.
- Agent replies do not produce owner callbacks.

After each answer run `just symphony-status --tidy`, then `just symphony-status --open`
to confirm nothing is left waiting. Say in the terminal, in one or two lines,
what each event was and what you did, so the conversation there stays in step.

## Answer an ask

An `ASK` is answered in the ask store, never in the markdown:

```
just symphony-ask show <id>                         # the thread, with any quoted text
just symphony-ask answer <id> "<markdown>" --as <handle>
just symphony-ask list --open                       # anything still waiting
```

Read the section it points at first (`<path>`, then the `#anchor` heading or
card), and the quote in `show` if there is one. Answer short and
conversational, like a chat reply: a few sentences, `[[FILE#Heading]]` links
and pictures render as in card replies, `/artifacts/` ones too, as an ask is
never committed. A follow-up arrives as
another `ASK` line with the same id; `show` it for the context.

Asks are ephemeral: they live a day in `artifacts/asks/` and are never
committed. Never copy an ask or its answer into a document, a card or a
callout unless the owner asks for that. If answering needs a change to a
document, say so in the answer and let the owner turn it into a comment or a
card.

## Images in a reply

When the owner asks to see something, make the requested artifact and inspect it before posting. Follow project-local visual, media and review skills for generation, taste, asset paths and plugin card commands. Use the project components its plugins provide; the generic integration invents no domain-specific component or approval rule.

## Disconnect

When the owner says they are done, run `just tasks callback --off --as <handle>`
(or `just tasks leave --as <handle>` for a dedicated chat session), and say so.
If you changed a tool or document, run the destination project's required checks
and follow its review and landing workflow. No project-specific verifier or recipe
is required by this generic skill.
