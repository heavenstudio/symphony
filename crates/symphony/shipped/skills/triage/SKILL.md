---
name: triage
description: Cut the owner's review queue. Measures the owner's past verdicts, auto-approves art the owner's taste predicts they would approve, fixes art the taste predicts they would reject, and hands the owner a short ranked list of what only they can decide. Trigger on "too many things to review", "what should I review next", "auto mode", "triage the cards", or /triage.
---

# Triage the review queue

The owner decides; this skill makes sure they only see what needs them.
A design card is auto-approved only when the owner's own words elsewhere
already answer it (`auto-approve --answered-by`); otherwise it goes to the owner. Art is auto-approved only when the
owner's taste clearly predicts approval.

1. **Refresh.** Run `just symphony-status --tidy`, then, every run,
   `just triage stats`. Use those live numbers in the
   judging and in the owner report; never quote rates from
   [taste.md](taste.md), which keeps none. Add to taste.md a pattern for every
   owner rejection note since the last run (card slugs as evidence).
2. **Judge the pending art,** one subagent per faction page. Each looks at
   every candidate (shrink it with `sips -Z 600`) against [taste.md](taste.md)
   and decides one of:
   - **auto-approve `<letter>`**: at least one candidate passes every pattern
     in taste.md with nothing doubtful. Pick the best by the owner's own
     reasons (clearer mounts, a more functional shape, a bolder outline).
     Run `tools/rs-bin.sh art triage auto-approve <doc> <slug> <letter> "<basis>"`,
     where the basis names the checks it passed.
   - **fix**: every candidate fails a pattern. Redraw the failing ones
     (the art rules in docs/art/CONCEPT_ART.md, the route in
     docs/art/IMAGE_GENERATION.md) and post a replacement card; the
     next run judges it.
   A hull drawn from another hull (SHIP_ROSTER's Biological donor hulls) is
   judged only once its donor's picture is approved; until then it waits, and
   the donor's card ranks first.
   - **owner**: a real taste call (two strong candidates, a new idea, a
     pattern taste.md does not cover). Leave it pending.
3. **Check the variety** with `tools/rs-bin.sh art roster-variety`. It must
   exit 0 before anything is auto-approved.
4. **Hand the owner a short list:** `just triage queue 10`.
   Report what was auto-approved (the owner spot-checks them under
   "Auto-approved: spot-check" and can overturn any), what is being redrawn,
   and those 10, highest leverage first.
5. **Learn from the spot-checks.** An owner verdict that overturns an
   auto-approval is the most valuable signal there is: add its reason to
   taste.md, and re-judge any auto-approved card that shares the problem.
