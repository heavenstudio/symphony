// The Symphony's wire types as the web client reads them, written from crates/symphony-api
// by its `typescript` example (`just client-wire`). Never edited by hand: `just client-lint`
// fails when this file and the Rust types disagree.

export const PROJECT_ELEMENT = "symphony-project";
export const BOARD_PATH = "/api/board";
export const FEED_PATH = "/api/feed";
export const FEED_EVENT = "delta";
export const PING_EVENT = "ping";
export const SERVER_EVENT = "server";
export const CACHE_FORMAT = 2;
export const SYMPHONY_ROUTES = [
  "tasks",
  "task",
  "agents",
  "queue",
  "reports",
  "api",
  "assets",
  "static",
  "thumb",
  "x",
  "artifacts/uploads",
  "app",
  "doc",
  "history",
  "diff",
  "wasm",
  "_dioxus",
  "favicon.ico",
] as const;
export const PAGE_NAMES = {
  HOME: "Home",
  QUEUE: "Queue",
  DOCS: "Docs",
  BOARD: "Board",
  REPORTS: "Reports",
  AGENTS: "Agents",
  PROJECTS: "Projects",
} as const;

export type ProjectView = {
  /**
   * The name its theme package shows in the header and the tab: the project's own by default.
   */
  name: string;
  /**
   * The decision register as a link names it: its route when an entrypoint holds it
   * (`docs/DECISIONS.md`), whatever the folder is called, else its project-relative path; empty
   * when the project has no such file yet, and the home then offers no link to one.
   */
  register: string;
  /**
   * The review guide as a link names it, by its route (`docs/REVIEW.md`); empty when the
   * project has none, and the home then offers no link to one.
   */
  guide?: string;
  /**
   * The plugins' navigation entries, after the Symphony's own.
   */
  nav: Array<NavItem>;
  /**
   * The routes the project's entrypoints are served at, in its file's order, without slashes:
   * `docs`, or empty for one at `/`. None sent is the convention, `docs` alone.
   */
  entrypoints: Array<string>;
  /**
   * The prefix of every path of the project's pages: `/<id>` on the machine's one server,
   * which serves several projects, and empty on a server of its own. Every link the client
   * draws and every request it sends lies under it ([`ProjectView::here`]).
   */
  base?: string;
};

export type NavItem = {
  /**
   * The header's word (`▤ shelf`), drawn as the plugin gives it, after the Symphony's own
   * (`symphony_api::page_names`).
   */
  label: string;
  /**
   * The home's link text (`Shelf items`).
   */
  title: string;
  /**
   * The path it opens (`/shelf`), a client page.
   */
  href: string;
  /**
   * The document that page shows, a project-relative path.
   */
  document: string;
};

export type Board = {
  /**
   * The feed's version this snapshot is at: the client follows the feed from here.
   */
  version: number;
  /**
   * The client build the server serves: the path of the shell's module script as the shell
   * names it (`/assets/symphony-shell-….js`), whose hashed name changes with the client's
   * code. A client whose own differs loads the page again.
   */
  build: string;
  /**
   * How often the server pings the feed, in ms: at most [`PING_MAX_MS`].
   */
  ping_ms: number;
  /**
   * The first [`BOARD_CARDS`] owner decisions in mixed document-category order.
   */
  next: Array<Card>;
  counts: Counts;
  progress: Progress;
};

export type Card = {
  /**
   * The owner's view order only; no verdict or document marker changes.
   */
  postponed?: boolean;
  /**
   * The card's anchor in its document.
   */
  slug: string;
  /**
   * The document's app path: `docs/…/X.md`, or `@scope/docs/…/X.md` for a proposal.
   */
  doc: string;
  /**
   * The document's title.
   */
  doc_title: string;
  /**
   * A picture card by the slot it fills ("North · cover picture"); any other card by its
   * first sentence, cut short.
   */
  title: string;
  /**
   * The words the title did not already say, cut short; empty when there are none.
   */
  summary?: string;
  /**
   * The triage's reason, without its zero counts: "design · gates 11 items".
   */
  why: string;
  /**
   * When it was posted, as the register writes it: `YYYY-MM-DD HH:MM` or `YYYY-MM-DD`.
   */
  posted?: string | null;
  /**
   * Its pictures in the card's order: the candidates (up to four) or the items it names (up
   * to six). The first one is its thumbnail.
   */
  pictures?: Array<Picture>;
  /**
   * The task proposing this version of the card, when it comes from another checkout.
   */
  task?: string | null;
};

export type Picture = {
  /**
   * Where the picture is served; `None` for an item not drawn yet ("picture coming").
   */
  src?: string | null;
  /**
   * The candidate's letter ("A"), or the item's name.
   */
  caption: string;
  /**
   * The picture still waits on the owner's review.
   */
  waiting?: boolean;
};

export type Counts = {
  /**
   * Cards waiting on the owner: the whole queue, not only the ones the board shows.
   */
  waiting: number;
  /**
   * The documents those cards are in.
   */
  docs_waiting: number;
  /**
   * Auto-approved cards to spot-check.
   */
  spot_check: number;
  /**
   * Cards an agent answered the owner on today.
   */
  answered_today: number;
  /**
   * The task board's counts; `None` when the server has no task board.
   */
  tasks: TaskCounts | null;
};

export type TaskCounts = { in_progress: number; in_review: number; agents_live: number };

export type Progress = {
  /**
   * The regions the project's plugins add to the home, in the order they are registered.
   */
  regions: Array<Region>;
  /**
   * The design documents (a folder's README is navigation, not one).
   */
  docs: number;
  /**
   * The decisions waiting across them.
   */
  decisions_waiting: number;
};

export type Region = {
  /**
   * The region's id among the project's regions, stable across snapshots.
   */
  id: string;
  title: string;
  /**
   * The line under the headline's percentage (`21 of 31 items approved`).
   */
  note: string;
  bars: Array<ProgressBar>;
  /**
   * The legend's words for done, waiting and remaining, in that order.
   */
  legend: [string, string, string];
};

export type ProgressBar = {
  key: string;
  name: string;
  /**
   * Where the bar links: a project-relative path.
   */
  page: string;
  /**
   * What the bar says on hover.
   */
  title: string;
  done: number;
  waiting: number;
  total: number;
};

export type Delta = {
  /**
   * The version after this change: the next `since`.
   */
  version: number;
  /**
   * Each region that moved, with its new data.
   */
  changes: Array<Change>;
};

export type Change =
  | { region: "card"; data: Card }
  | { region: "queue"; data: Array<Card> }
  | { region: "counts"; data: Counts }
  | { region: "progress"; data: Progress }
  | { region: "client"; data: { build: string } };

export type Queue = {
  waiting: Array<Card>;
  spot_check: Array<QueueItem>;
  agent: Array<QueueItem>;
  /**
   * Always empty; retained for clients from the preceding build that require this wire field.
   */
  branches: Array<QueueItem>;
};

export type QueueItem = {
  href: string;
  path: string;
  slug: string;
  title: string;
  kind: string;
  text: string;
  branch?: string | null;
};

export type Activity = { items: Array<ActivityItem> };

export type ActivityItem = {
  id: string;
  kind: string;
  path: string;
  slug: string;
  ts: string;
  text: string;
  href: string;
  answered: boolean;
};

export type Pending = { items: Array<PendingItem>; docs: Array<string> };

export type PendingItem = { path: string; slug: string; href: string; branch?: string | null };

export type TaskListing = {
  machine_work: MachineWork;
  tasks: Array<Task>;
  statuses: Array<string>;
  signoff: Array<string>;
  counts: Array<[string, number]>;
  /**
   * Complete filtered/windowed lifecycle totals, used when the client's feed is lost.
   */
  lifecycle_counts: Array<[string, number]>;
  next: string | null;
};

export type Task = {
  slug: string;
  title: string;
  body: string;
  status: string;
  priority: number;
  parent: string | null;
  created_by: string;
  created: number;
  updated: number;
  links: Array<string>;
  depends: Array<string>;
  roles: Array<Role>;
  instruction: boolean;
  /**
   * The plugins' board fields: plugin name, then field name, then its value. With the feature
   * `skip-board-fields` a task is read without them, whatever the wire holds, and the map stays
   * empty, so a task written back carries none: the browser's client turns it on, as it draws
   * no field, and so leaves out the reader of the two maps and of their values (a number's text
   * parser among it), about 18 KB gzipped of the board's first visit.
   */
  fields?: { [key in string]: { [key in string]: FieldValue } };
};

export type Role = {
  handle: string;
  role: string;
  since: number;
  expires: number | null;
  health: string;
};

export type FieldValue = number | string;

export type TaskChange = { key: string; fields: TaskFields };

export type TaskFields = {
  title?: string | null;
  body?: string | null;
  status?: string | null;
  priority?: number | null;
  /**
   * Present only when parent changes. Empty string clears the parent.
   */
  parent?: string | null;
  updated?: number | null;
  links?: Array<string> | null;
  depends?: Array<string> | null;
  roles?: Array<Role> | null;
  instruction?: boolean | null;
};

export type ItemChange<T> = { key: string; value?: T | null; removed?: boolean };

export type MachineWork = {
  freshness: WorkFreshness;
  machines: Array<MachineObservation>;
  unassigned: Array<WorkOperation>;
  /**
   * Project-configured advisory targets by stable machine id. Never admission limits.
   */
  guidance: { [key in string]: WorkGuidance };
};

export type MachineObservation = {
  machine_id: string;
  label: string;
  producer_id: string;
  sequence: number;
  age_ms: number | null;
  freshness: WorkFreshness;
  complete: boolean;
  reason: string;
  /**
   * None is unknown occupancy, never free capacity. Counts exclude reservations/commands.
   */
  counts: WorkCounts | null;
  operations: Array<WorkOperation>;
};

export type WorkOperation = {
  machine_id: string | null;
  /**
   * None when legacy evidence has no operation nonce.
   */
  operation_id: string | null;
  parent_id: string | null;
  handle: string | null;
  task: string | null;
  checkout: string;
  recipe: string;
  kind: WorkKind;
  state: WorkState;
  resource: string;
  reason: string;
  /**
   * Producer time for display only. Freshness uses receiver-assigned evidence.
   */
  phase_since: number | null;
  /**
   * The tasks `task` is under on the board, nearest first, as the Symphony read the board
   * ([`MachineWork::place_under`]): a run for a subtask is its parents' work too. `task` stays
   * the recorder's binding.
   */
  under?: Array<string>;
};

export type WorkCounts = {
  running_builds: number;
  queued_builds: number;
  running_tests: number;
  queued_tests: number;
};

export type WorkGuidance = { build: number | null; test: number | null };

export type WorkFreshness =
  | "unknown"
  | "fresh"
  | "stale"
  | "disconnected"
  | "incomplete"
  | "conflict";

export type WorkKind = "build" | "test" | "command" | "setup" | "reservation";

export type WorkState = "queued" | "running";
