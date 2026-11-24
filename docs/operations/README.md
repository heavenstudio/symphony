# Operations

Setting the Symphony up and running it: a first project, the commands, the project file, the board's database, GitHub, notices by Slack and by email and the repositories around this one. The other areas are in the [index](../README.md).

- [Getting started](GETTING_STARTED.md) — a project of your own, in order: install, `symphony
  init`, a first document and decision card, a first task to its landing, agents on the work.
- [The commands](COMMANDS.md) — the recipes, which command to reach for, and where each is
  explained.
- [The project file](PROJECT_FILE.md) — `symphony.toml`: every table and key a project can set,
  with its default.
- [Operating the board](BOARD_OPERATIONS.md) — the board's store and the schema contract between
  its builds, which build moves a live board, who writes it, its server, snapshots and holds, what
  the command may write, stuck servers, a board that went back or was copied, and board health.
- [PostgreSQL board lifecycle](POSTGRESQL.md) — the board's PostgreSQL: its engine identity, a
  board still on SurrealDB and how it moves, server selection, holds, snapshots, recovery and
  space reclamation; the hosted board, which no command reaches yet.
- [Setting up GitHub](GITHUB_SETUP.md) — the optional `symphony-github` from nothing to a check
  that passes: building it, the token for the issue exchange, the GitHub Apps and rulesets a
  delivery by pull request needs, in order, and what each failure a newcomer meets says.
- [GitHub issue exchange and pull request delivery](GITHUB.md) — the reference for that adapter,
  turned off unless the default branch commits its settings: its settings and token files, its
  commands, what a reconcile writes, what it keeps, what pauses it, and how a land is delivered
  and the protection that delivery reads.
- [Sending notices by Slack](NOTICE_SLACK.md) — the optional `symphony-notice-slack`, a notice
  plugin apart from the core: the Slack app and its token, where the token is kept, building it,
  what it sends, its exit statuses and what each Slack refusal asks of you.
- [Notices by email](NOTICE_EMAIL.md) — the email notice plugin, apart from the core: building
  it, its settings and password in the machine folder, the mail it sends by SMTP and what each
  exit says.
- [WhatsApp notices](NOTICE_WHATSAPP.md) — `symphony-notice-whatsapp`, the notice plugin that
  sends a person's notice by WhatsApp: what Meta must give and approve first, the person's
  agreement and the cost, its folder, settings and token, and what its exit says.
- [Upstream and downstream repositories](UPSTREAM_DOWNSTREAM.md) — the regular import of the
  public repository into a private copy with its ancestry kept, the export of selected reusable
  changes back, their checks, refusals and recovery; manual tasks, with PR delivery not enabled.

## Analysis

An analysis holds a proposal and its evidence: an approved design can still await implementation, and its approval alone does not make it current behavior.

- [The board's cutover to PostgreSQL](analysis/board-pg-cutover.md) — the order for moving live
  boards, one board's move, the way back, and this repository's board's move on 2026-11-21.
- [The board's store, measured before it moves](analysis/board-store-baseline.md) — what the board
  costs on SurrealDB, and the meter the two stores are compared by.
- [Managed PostgreSQL](analysis/hosted-postgresql.md) — the proposed hosted connection, connection
  budget, database boundary and PostgreSQL 16/18 validation contract.
- [Where the board's PostgreSQL comes from](analysis/postgresql-binary-source.md) — each place a
  machine could take its PostgreSQL from, and what each costs.
- [The read-only window after a schema move](analysis/schema-move-read-only-window.md) — the wait
  between a landing that moves the board's schema and a release that serves it.
- [Hosted ingress and board readiness](analysis/hosted-ingress.md) — proposed trusted proxy and
  visitor admission, project permissions, foreground lifecycle and readiness; the local gate
  stays.
- [Optional GitHub integration](analysis/github-integration.md) — tasks exchanged with GitHub
  issues and work published as pull requests.
- [Historical personal-path treatment](analysis/github-history-treatment.md) — scratch-only
  evidence for how the public history's path findings are treated.
- [Public-readiness audit](analysis/public-readiness.md) — the read-only audit and remediation
  plan recorded for the GitHub integration.
- [Tracked media privacy review](analysis/public-media-review.md) — what the tracked pictures show
  of a person or a machine.
