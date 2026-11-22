# Design

How the app looks: the header, the mark, a page while it loads and the timeline. It holds no
reference document yet; what each page is for is in the [product specs](../product/README.md), and
the other areas are in the [index](../README.md).

## Analysis

An analysis holds a proposal and its evidence: an approved design can still await implementation, and its approval alone does not make it current behavior.
The pictures beside them that no analysis shows are the ones a card in the
[decision register](../DECISIONS.md) shows.

- [The header](analysis/header-names.md) — what the header calls the pages, how it sits on a
  phone, and where Appearance lives.
- [The header on a phone, in one row](analysis/phone-header.md) — a smaller header for a phone.
- [Symphony's mark](analysis/symphony-mark.md) — four options for the mark and the wordmark; the
  fermata is chosen.
- [A task's first visit](analysis/task-first-visit.md) — what a task's page shows before its reads
  answer, and what moves when they do.
- [Components draw when their data arrives](analysis/components-draw-when-their-data-arrives.md) —
  each component drawn as soon as its data is there, so no page waits on its slowest part.
- [Navigating a task's timeline](analysis/timeline-navigation.md) — zoom and sideways movement on
  the wide timeline, with builds and tests by their recorded times.
- [Orchestrators and messages on a task's timeline](analysis/timeline-orchestrator.md) — who
  orchestrated a task and which way its messages went, on the timeline's clock.
- [A task's cost on its card, its page and its timeline](analysis/token-cost.md) — a task's
  estimated cost after its time on its board card, its page and each stretch of its timeline.
