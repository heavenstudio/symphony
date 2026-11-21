# Product specs

What each page of the Symphony is for and how it behaves, one page a document, written for
whoever uses it rather than whoever builds it. How it is built (formats, routes, wire types,
files) stays in the documents the [index](../README.md) lists; a spec links them and repeats none
of it.

- [The review queue](QUEUE.md) — every card waiting on the owner, in the order to answer them.
- [Reading a document](DOCUMENT.md) — contents, comments, evidence, versions and branch comparisons.
- [Cards and verdicts](CARDS.md) — deciding, notes, Later, branch drafts and saved history.

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
`pictures/NAME-phone.png` and `pictures/NAME-wide.png`. It reaches no shared server and no board,
and stops the server it started.

- **A new picture** is a line in `pictures.txt`: its name, the widths a document shows it at, the
  page's path and, for a state that takes a press to reach, the steps of `symphony shot` that
  reach it (`--click`, `--type`, `--key`). A state the fixture lacks is added to the fixture. Only
  a picture a document shows is taken and committed.
- **One command retakes them all**, or those named: `tools/product-pictures.sh queue`.
- **A retake that changes nothing writes the same bytes**, so after one, `git status docs/product`
  lists exactly the pictures that had gone stale. That holds on one Chrome and one set of fonts: a
  Chrome update can redraw every picture with no page changed, and the whole set is then retaken
  and committed once.
- **A task that changes how a page looks retakes its pictures** and commits them with the change:
  they are the after pictures its evidence shows ([CHECKS](../workflow/CHECKS.md#evidence-by-kind-of-change)).
- **They are committed beside the specs**, so a clone and GitHub show them. A wide picture is
  about 0.1 MB and a phone picture, at three pixels to the point, about 0.3 MB.
- **A page that shows the time of day or an age** (the home page's greeting, "waiting 39 d") is
  not pictured until its picture can be taken at a fixed time, as it would differ every day.

## Analysis

An analysis holds a proposal and its evidence: an approved design can still await implementation, and its approval alone does not make it current behavior.

- [The agents page, orchestrator-first](analysis/agents-page.md) — three layouts of an Agents page
  that opens on the orchestrators.
- [The board at a glance](analysis/board-design.md) — what a board says before anything is opened:
  what is stuck, on whom it waits and for how long.
- [Owner asks](analysis/owner-asks.md) — what becomes of what the owner says that waits on an
  agent's answer.
- [Reports and monitoring](analysis/reports-and-monitoring.md) — what a project's runs, task pages
  and reports would show, and where each figure comes from.
- [Desktop client analysis](analysis/desktop-client.md) — the design and evidence behind the Mac
  app.
- [Product documentation review, November 2026](analysis/product-docs-review-2026-11.md) — what a
  person using the Symphony can read about it, and the form of a spec.
