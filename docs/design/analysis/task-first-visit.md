# A task's first visit

What a task's page shows the first time a browser opens it, before its reads answer, and what
moves when they do: today's way (A) against drawing what is under the head only once the task's
page has answered (B), as the card in [DECISIONS](../../DECISIONS.md#task-first-visit-head-first)
asks. Measured 2026-11-16 on this Mac in headless Chrome, against the browser target's stand-in
server and its fixture task (`apps/symphony-ui/tests/browser/layout_shift.rs`), at main `325f08f1`
for A and on a throwaway build of B made for this page and not kept.

## The choice

- **A, today.** Every part draws its outline at once: the head, the gates, the timeline's box, the
  brief, the thread and the composer. On a first visit their sizes are guesses: the head and the
  brief of the last task this browser drew at that width, the gates of the last task drawn there,
  else a title of one line wide and two on a phone, one line of facts, four gates with one waited
  on and a brief of seven lines. When the answers come, everything under a wrong guess moves once.
- **B, the head first.** While this browser holds neither the task's page nor the heights it drew
  of this task at this width, the page draws the head's outline alone. Once the task's page
  answers (from this device or the server) it draws everything under the head at once: the gates'
  outline at the count the page names until their own answer, the timeline's box, the brief, the
  thread's outline and the composer, each where it stays. Gates that answer before the page wait
  for it. Every later visit is as under A.

Pictures, each the waiting page under A, under B, then the page loaded (the same under both), dark,
motion off. A task with two gates, phone and wide:

![Two gates on a phone: A waiting, B waiting, loaded](task-first-visit-two-gates-phone.webp)

![Two gates wide: A waiting, B waiting, loaded](task-first-visit-two-gates-wide.webp)

A task with six gates, phone and wide:

![Six gates on a phone: A waiting, B waiting, loaded](task-first-visit-six-gates-phone.webp)

![Six gates wide: A waiting, B waiting, loaded](task-first-visit-six-gates-wide.webp)

## What moves

Layout shift of a first visit in a fresh browser, in each order the page's and the gates' answers
can come in, the other held until the first is in (drawn; under B, whose gates wait for the page,
sent by the server); under 0.05 is the limit every page's browser test holds, and 0 is nothing
moved:

| task | way | phone, page first | phone, gates first | wide, page first | wide, gates first |
|---|---|---|---|---|---|
| two gates | A | 0.140 | 0.064 | 0.031 | 0.028 |
| two gates | B | 0 | 0 | 0 | 0 |
| six gates | A | 0.020 | 0.020 | 0.022 | 0.022 |
| six gates | B | 0 | 0 | 0 | 0 |
| four gates, the guess | A | 0.020 | 0.020 | 0 | 0 |
| four gates, the guess | B | 0 | 0 | 0 | 0 |

Under A, two gates where four were guessed shrink the gates' section by about 190 px on a phone,
bringing the timeline up into the screen; six gates grow it, pushing what is under it down and
mostly off a phone's screen, which the score counts less. The head's guess of a two-line title is
32 px taller than the fixture's one line: 0.020 on a phone in every shape. A longer title, more
facts, another brief or gates whose words wrap move what is under them as far as they differ from
the guess, so these figures hold only for the fixture's head and brief.

Under B nothing drawn moves in any of these: what is under the head comes once its sizes are known,
and the head's own outline has nothing under it to move. Its gates still show their outline, at the
right count, from the page's answer to theirs.

## What B costs

- **Less shows at first.** Under the head nothing shows until the task's page answers: here 32 to
  54 ms after the outline in the quiet runs, as under A, since the page answers first. On the live
  server the page's read is about 80 ms of its own time, plus the trip, more on a phone over the
  tailnet or a loaded server: not measured here.
- **The head is not later.** It is drawn from the same answer under both: 30 runs of each, from the
  page's start, at a median of 193 ms under A and 210 under B, the machine loaded by other work
  (the slowest runs took seconds under both), so the two are not told apart here.
- **Gates that come first wait.** Under load the server answers the page before the gates (146 and
  252 ms in [the parts' analysis](components-draw-when-their-data-arrives.md#what-makes-parts-wait));
  when the gates come first they are drawn with the page.
- **The work.** The task's page draws under its head only once the page is there, the reads under
  it asked as the page opens rather than as their parts mount, and the layout-shift test's "gates
  first" order waits for the gates' answer rather than their drawing.

## Why not the board's rows

Getting the counts onto the device before the task's own reads, for example with the board's rows,
does not do it. A first visit in a browser that kept nothing holds no rows, the board's task rows
carry no count of gates today, and nothing on the device sizes the head or the brief of a task never
drawn. Where the device does hold the task's page (read ahead from a link, or kept from a visit at
another width), B draws everything at once from it.

## Measured, read and inferred

- **Measured here:** every score in the table, by the layout-shift test's own meter, and the head's
  times, on the fixture task only; the pictures, from the same runs.
- **Read in code:** what each part guesses and when it draws (`task-page.tsx`, `gates.tsx`,
  `viewport.tsx`), and what a board row carries (`wire.ts`'s `Task`).
- **Inferred, not tried:** how long the empty space under the head lasts on a phone over the
  tailnet; how B scores for a task whose head or brief is far from the guess (it draws nothing
  under the head before both are known, so the size of the miss does not enter).
