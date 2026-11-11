---
name: triage
description: Reduce the owner's review queue using current verdict statistics, project-local taste evidence and a short ranked list of decisions.
---
<!-- symphony-stamp: 66f55432c3bfe5a5 files=SKILL.md -->

# Triage the review queue

The owner decides. Read the project's review conventions, permissions and local
taste guidance. Follow its model, validation and landing policy.
Generic Symphony supplies no visual taste data.

1. Run `just symphony-status --tidy`, then `just symphony-cli triage stats --root .`.
   Use live numbers, never remembered approval rates.
2. Read open notes and inspect every candidate. An approval with a note still
   creates work. Use apply-review for document changes.
3. Record a design as answered only when the owner's exact words decide it. Real
   choices stay pending. Visual auto-approval requires explicit project delegation,
   taste evidence and its plugin's command; otherwise recommend to the owner.
4. Check dependencies and project quality checks before recommending approval.
   Rank an unapproved source artifact first. Repair concrete defects within scope
   and post replacements using project plugin commands.
5. Run `just symphony-cli triage queue 10 --root .` and give the owner the ranked list
   with reasons. Say what was fixed, what their words answered and what still
   needs their judgment.
6. Record owner corrections in project-local taste or guidance with their source.
   Revisit recommendations that share the problem. Never distribute one project's
   taste as generic Symphony policy.
