# Product specs

What each page of the Symphony is for and how it behaves, one page a document, written for
whoever uses it rather than whoever builds it. How it is built (formats, routes, wire types,
files) stays in the documents the [index](../README.md) lists; a spec links them and repeats none
of it.

- [The review queue](QUEUE.md) — every card waiting on the owner, in the order to answer them.
- [Reading a document](DOCUMENT.md) — contents, comments, evidence, versions and branch comparisons.
- [Cards and verdicts](CARDS.md) — deciding, notes, Later, branch drafts and saved history.
- [The task board](BOARD.md) — every task by its stage, and what each waits on.
- [A task's page](TASK.md) — its gates, timeline, brief, changes and thread, and the owner's approval.
- [Reports](REPORTS.md) — the project's work over time: what was done, by how many agents, at what cost.
- [Runs](RUNS.md) — every recorded build and test, what runs now, and one run's steps, failures and log.
- [Themes and appearance](THEMES.md) — a project's own name, icon and colours, and one device's own look.

## The form of a spec

Product specs follow this approved form and the pictures' rule below
([its card](../DECISIONS.md#product-spec-form)).

A spec is short enough to read on a phone in a few minutes, and has these parts in this order.
A part with nothing to say is left out.

| Part | What it says |
| --- | --- |
| Opening | what the page is, its address, who it is for, and one wide picture |
| What the owner does here | each thing a person comes to the page to do, in their words |
| What it shows | its regions or groups, and what a row or a card of each holds |
| Rules | a section for each rule the page keeps: an order, a filter, a limit |
| States | a table: loading, empty, failed, out of date, and each state of its own |
| On a phone | what differs at a phone's width, with a phone picture |
| Edges | the cases that surprise: ties, two devices, a missing file |
| Related | the documents that hold its formats and commands |

A spec says how the page is now. It names no file of the source, no route but the page's own
address, and no wire type.

## Pictures

A spec's pictures are taken, never drawn or pasted: `tools/product-pictures.sh` starts a server of
its own over the fixture project (`tools/product-pictures/fixture`, a made-up recipe site with a
few cards in three folders), takes every line of [pictures.txt](pictures.txt) with `symphony shot`
on a phone (390x844), wide (1280x800) or both, as the line says, in the light scheme, and writes
`pictures/NAME-phone.png` and `pictures/NAME-wide.png`. The fixture has a board of its own, which
`tools/product-pictures/board.sh` fills with a few tasks, one at least in every stage and waiting
on each kind of thing the board names, and `tools/product-pictures/records.sh` writes there what
its machine recorded: the runs of the last days and twelve days of hours, for Runs and Reports.
The pictures after the list's `=theme` line are of the same project with a theme of its own
(`tools/product-pictures/theme.toml`). The script reaches no shared server and no project's
board, and stops the server it started.

- **A new picture** is a line in `pictures.txt`: its name, the widths a document shows it at, the
  page's path and, for a state that takes a press to reach, the steps of `symphony shot` that
  reach it (`--click`, `--type`, `--key`). A state the fixture lacks is added to the fixture, a
  task's to `board.sh`, a run's or an hour's to `records.sh`. Only a picture a document shows is taken and committed.
- **One command retakes them all**, or those named: `tools/product-pictures.sh queue`.
- **A retake that changes nothing writes the same bytes**, so after one, `git status docs/product`
  lists exactly the pictures that had gone stale. That holds on one Chrome, one set of fonts and
  one machine's scroll bars (macOS draws a wide page's bar or not by its *Show scroll bars*
  setting and whether a mouse is plugged in): a change of any of them can redraw every picture
  with no page changed, and the whole set is then retaken and committed once.
- **A task that changes how a page looks retakes its pictures** and commits them with the change:
  they are the after pictures its evidence shows ([CHECKS](../CHECKS.md#evidence-by-kind-of-change)).
- **They are committed beside the specs**, so a clone and GitHub show them. A wide picture is
  about 0.1 MB and a phone picture, at three pixels to the point, about 0.3 MB.
- **Every picture is taken at one moment**, a Monday at 09:30 UTC that the script names: the
  fixture's board and its machine's records are written at moments counted back from it, the
  server's board clock and its reads of those records (Reports' window, Runs' days) are held
  there, and `symphony shot --clock` holds the page's own. So a page that shows an
  age or a time ("oldest 6 d", a thread's dates) is the same picture on any day. A page whose
  words come from somewhere else that moves (a commit's date read from git, the machine's name)
  is pictured only once the fixture fixes that too.
