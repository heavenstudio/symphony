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
export const BOARD_CARDS = 5;
export const FILE_QUERY = "raw";
export const REPORTS_PATH = "/api/reports";
export const VERSION_HEADER = "symphony-version";
export const VERDICT_TASKS = "verdicts/";
export const UPLOAD_PATH = "/api/upload";
export const UPLOAD_MAX_BYTES = 10485760;
export const UPLOAD_ACCEPT =
  "image/png,image/jpeg,image/gif,image/webp,application/pdf,text/plain,text/markdown,text/csv,application/json,.png,.jpg,.jpeg,.gif,.webp,.pdf,.txt,.md,.csv,.json,.log,.docx,.xlsx,.pptx";
export const TASK_ACTION_ROUTE = "/api/task-action";
export const TASK_GATES_PATH = "/api/task-gates";
export const TASK_TIMELINE_PATH = "/api/task-timeline";
export const TIMELINE_EVERY = 60;
export const AGENT_FOCUS_PATH = "/api/agent-focus";
export const AGENT_SCREEN_PATH = "/api/agent-screen";
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

export type Grain = "hour" | "day" | "week";

export type ReportMark = {
  /**
   * Its commit time.
   */
  at: number;
  /**
   * Its hash, abbreviated.
   */
  commit: string;
  subject: string;
};

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
   * The plugins' board fields: plugin name, then field name, then its value.
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

export type AgentPage = {
  view: Agents;
  tasks: Array<Task>;
  total: number;
  next: string | null;
  /**
   * The page was asked from the server's own machine ([`SessionGo::control`]).
   */
  local: boolean;
};

export type Agents = {
  machine_work: MachineWork;
  generated: number;
  /**
   * Written as the wire carries it, the panes probe under both its names.
   */
  probes: { panes: string | null; herdr: string | null; slots: string };
  agents: Array<Agent>;
};

export type Agent = {
  handle: string;
  kind: string;
  model: string;
  parent: string | null;
  task: string | null;
  role: string | null;
  tasks: Array<HeldTask>;
  worktree: string;
  branch: string;
  agent: string | null;
  session_id: string | null;
  pane: string | null;
  callback: string | null;
  chat: boolean;
  state: string;
  state_detail: AgentRun | null;
  health: string;
  last_tool_call: number | null;
  last_message: AgentSaid | null;
  /**
   * The open tasks it holds against the board's cap on an orchestrator's, when one is set.
   */
  slots: AgentSlots | null;
  /**
   * The web address its session gave (`tasks register --session-url`, `tasks callback
   * --session-url`), when it gave one.
   */
  session_url: string | null;
  /**
   * It runs inside another session (a subagent), so it has no pane of its own.
   */
  in_process: boolean;
  /**
   * Where its Go to session control goes, as the server resolved it ([`resolve_sessions`]):
   * none when nothing is known.
   */
  go: SessionGo | null;
};

export type HeldTask = { slug: string; role: string; status: string };

export type AgentRun = { recipe: string; phase: string; since: number | null };

export type AgentSaid = { slug: string; ts: number; text: string };

export type AgentSlots = { open: number; cap: number };

export type SessionGo = {
  /**
   * Whose session it reaches: the agent's own, or for a subagent with no way in of its own,
   * the parent session that runs it.
   */
  handle: string;
  /**
   * That session's web address: the control links to it.
   */
  url: string | null;
  /**
   * The hc terminal that shows that session on the server's machine, offered only when the
   * project addresses its panes (`[agents] panes = "hc"`).
   */
  pane: string | null;
};

export type AgentFocus = { said: string };

export type AgentScreen = {
  /**
   * Whose session it shows: the agent's own, or its parent's.
   */
  handle: string;
  pane: string;
  text: string;
  /**
   * When it was read, in seconds since the epoch.
   */
  read_at: number;
};

export type Block =
  | { type: "heading"; level: number; anchor: string; spans: Array<Span> }
  | { type: "paragraph"; spans: Array<Span> }
  | { type: "list"; ordered: boolean; start: number; items: Array<Array<Block>> }
  | { type: "table"; head: Array<Cell>; rows: Array<Array<Cell>>; presentation?: Presentation }
  | { type: "code"; language: string; text: string }
  | { type: "quote"; blocks: Array<Block> }
  | { type: "rule" }
  | {
      type: "callout";
      kind: CalloutKind;
      title: Array<Span>;
      resolved: boolean;
      blocks: Array<Block>;
    }
  | {
      type: "card";
      slug: string;
      mark: CardState;
      title: string;
      body: Array<Block>;
      verdict: Verdict;
      thread: Array<ThreadEntry>;
      /**
       * Source line or offset supplied by the section pipeline, never guessed by the client.
       */
      line: number | null;
      line_offset: number | null;
      comparison: Array<Block>;
    }
  | { type: "component"; kind: string; blocks: Array<Block> }
  | { type: "media"; media: Media }
  | { type: "image"; image: Image }
  | {
      type: "figure";
      picture: Image | null;
      caption: Array<Span>;
      href: string | null;
      badge: string | null;
      size: FigureSize;
    }
  | { type: "caption"; blocks: Array<Block> }
  | { type: "comparison"; figures: Array<Block> }
  | { type: "group"; blocks: Array<Block> }
  | { type: "container"; tag: ContainerKind; presentation: Presentation; blocks: Array<Block> }
  | { type: "details"; summary: Array<Span>; open: boolean; blocks: Array<Block> }
  | { type: "settled-card"; summary: Array<Span>; open: boolean; blocks: Array<Block> }
  | { type: "thread"; entries: Array<ThreadEntry> }
  | { type: "comment"; text: string }
  | { type: "unknown"; tag: string; text: string };

export type Span =
  | { type: "text"; text: string }
  | { type: "emphasis"; spans: Array<Span> }
  | { type: "strong"; spans: Array<Span> }
  | { type: "code"; text: string }
  | { type: "link"; href: string; target: LinkTarget | null; title: string; spans: Array<Span> }
  | { type: "wiki-link"; href: string; target: LinkTarget | null; spans: Array<Span> }
  | { type: "line-break" }
  | { type: "image"; image: Image }
  | { type: "media"; media: Media }
  | { type: "component"; kind: string; blocks: Array<Block> }
  | { type: "icon"; shape: Array<VectorShape>; label: string; named: boolean; tip?: string }
  | { type: "styled"; tag: InlineKind; presentation: Presentation; spans: Array<Span> }
  | { type: "graphic"; view_box: string; presentation: Presentation; shapes: Array<VectorShape> }
  | { type: "blocks"; blocks: Array<Block> }
  | { type: "unknown"; tag: string; text: string };

export type LinkTarget = {
  /**
   * Project-relative path, or the current document for a fragment-only link.
   */
  path: string;
  anchor: string | null;
};

export type Presentation = {
  class?: string;
  id?: string;
  title?: string;
  /**
   * Only attributes recognized by the server, including sizing, accessibility, and the
   * document's existing CSS declarations. Unknown attributes are separate Unknown nodes.
   */
  attributes?: { [key in string]: string };
};

export type Image = { src: string; alt: string; presentation: Presentation };

export type Cell = { header: boolean; spans: Array<Span>; presentation: Presentation };

export type Verdict = {
  group: CardGroup;
  open: boolean;
  posted: string | null;
  decided: string | null;
};

export type ThreadEntry = {
  who: ThreadAuthor;
  timestamp: string | null;
  verdict: string | null;
  blocks: Array<Block>;
};

export type CardState =
  | "pending"
  | "approved"
  | "auto-approved"
  | "rejected"
  | "question"
  | "resolved"
  | "dismissed";

export type CardGroup = "decide" | "auto" | "confirm" | "agent" | "rejected" | "resolved";

export type ThreadAuthor = "owner" | "agent" | "unknown";

export type CalloutKind = "comment" | "like" | "dislike" | "question" | "decision" | "resolved";

export type FigureSize = "small" | "row" | "large" | "tile" | "cell" | "slot" | "wide";

export type ContainerKind =
  | "div"
  | "section"
  | "figure"
  | "figcaption"
  | "aside"
  | "header"
  | "footer"
  | "address"
  | "center";

export type InlineKind =
  | "span"
  | "small"
  | "sub"
  | "sup"
  | "time"
  | "mark"
  | "kbd"
  | "samp"
  | "delete"
  | "insert"
  | "underline"
  | "abbreviation";

export type VectorShape = {
  kind: VectorKind;
  /**
   * Geometry and paint attributes recognized for this SVG primitive, never markup.
   */
  attributes: { [key in string]: string };
};

export type VectorKind = "path" | "circle" | "ellipse" | "rect" | "line" | "polyline" | "polygon";

export type Media =
  | { kind: "mockup"; src: string; label: string }
  | { kind: "viewer"; src: string; label: string; poster?: string | null; viewer: Viewer }
  | { kind: "gallery"; items: Array<GalleryItem>; viewer: Viewer };

export type GalleryItem = { name: string; src: string; poster: string | null };

export type Viewer = {
  /**
   * The element's tag (`orbit-view`): a custom element's name, lower case with a hyphen.
   */
  element: string;
  /**
   * The module script that defines the element, loaded once, when the first viewer shows.
   */
  script: string;
  /**
   * Its attributes after `src` and `alt`, in order; an empty value is a bare attribute.
   */
  attributes: Array<[string, string]>;
  /**
   * The buttons under it, in order.
   */
  views: Array<View>;
  /**
   * The download link's text; none leaves the link out.
   */
  download: string | null;
  /**
   * What a gallery's button says to open a file in the viewer, and to close it again; none
   * says `Open` and `Close`.
   */
  open_label: string | null;
  close_label: string | null;
};

export type View = {
  label: string;
  /**
   * The button's tooltip.
   */
  title: string | null;
  /**
   * The attributes a press sets, in order.
   */
  set: Array<[string, string]>;
  /**
   * The element's method a press calls after setting them.
   */
  call: string | null;
};

export type Document = {
  version: number;
  path: string;
  title: string;
  waiting: Array<string>;
  crumbs: Array<Crumb>;
  headings: Array<Heading>;
  sections: Array<Section>;
  cards: Array<DocumentCard>;
  threads: { [key in string]: Fragment };
  tasks: Array<DocumentTask>;
  comparison?: DocumentComparison | null;
};

export type DocumentComparison = { task: string };

export type DocumentOutline = {
  document: Document;
  anchors: { [key in string]: Array<string> };
  /**
   * The hash of each card's thread, by the card's slug, as a section's `hash` is taken. A thread
   * is no part of its card section's `hash`, so a client that keeps a card's body and thread on
   * the device tells by this whether the thread changed since (docs/analysis/client-cache.md,
   * "What is kept").
   */
  thread_hashes?: { [key in string]: string };
};

export type DocumentSections = { version: number; change: DocumentChange };

export type Crumb = { name: string; href: string | null };

export type Heading = { level: number; anchor: string; text: string };

export type Fragment = { blocks: Array<Block> };

export type Section = {
  start: number;
  end: number;
  key: string;
  hash: string;
  part: number | null;
  blocks: Array<Block>;
  comparison?: SectionComparison | null;
};

export type Difference = "added" | "changed" | "removed";

export type SectionComparison = { kind: Difference; before?: Array<Block> };

export type DocumentCard = {
  slug: string;
  state: string;
  text: string;
  group: string;
  open: boolean;
};

export type DocumentTask = { slug: string; title: string; status: string };

export type Folder = {
  path: string;
  title: string;
  crumbs: Array<Crumb>;
  docs: number;
  intro: Document | null;
  entries: Array<FolderEntry>;
};

export type FolderEntry = {
  kind: string;
  path: string;
  title: string;
  docs: number;
  waiting: number;
  summary: string;
};

export type DocumentChange = {
  path: string;
  removed?: boolean;
  sections?: Array<Section>;
  deleted?: Array<string>;
  positions?: Array<SectionPosition>;
  order?: Array<string> | null;
  threads?: { [key in string]: Fragment | null };
  title?: string | null;
  waiting?: Array<string> | null;
  crumbs?: Array<Crumb> | null;
  headings?: Array<Heading> | null;
  cards?: Array<ItemChange<DocumentCard>>;
  tasks?: Array<DocumentTask> | null;
  /**
   * An empty vector clears the comparison; otherwise contains its single context.
   */
  comparison?: Array<DocumentComparison> | null;
};

export type SectionPosition = { key: string; start: number; end: number; part: number | null };

export type ItemChange<T> = { key: string; value?: T | null; removed?: boolean };

export type Version = { sha: string; date: string; subject: string };

export type DocumentDiff = { sha: string; subject: string; diff: string };

export type Ask = {
  id: number;
  path: string;
  anchor: string;
  quote: string;
  question: string;
  asked: number;
  updated: number;
  answers: Array<AskAnswer>;
  messages: Array<AskMessage>;
  status: string;
};

export type AskAnswer = { text: string; by: string; at: number };

export type AskMessage = {
  seq: number;
  kind: string;
  by: string;
  text: string;
  at: number;
  blocks?: Array<Block>;
};

export type Upload = {
  /**
   * The file's address, `/artifacts/uploads/<sha256>.<ext>`, the same on every branch's pages.
   */
  url: string;
  /**
   * The owner's file name, made safe for one Markdown line.
   */
  name: string;
  /**
   * A picture the page draws inline; any other file is a link.
   */
  picture: boolean;
  bytes: number;
  /**
   * The one line a post carries to keep the file: `![name](url)` or `[name](url)`.
   */
  markup: string;
};

export type TaskPage = {
  machine_work: MachineWork;
  task: Task;
  blocks: Array<Block>;
  children: Array<Task>;
  children_count: number;
  /**
   * Bytes of the brief the page left out; `/api/task-brief` has all of it.
   */
  brief_omitted: number;
  /**
   * Where the shown part of the brief ends; ask `/api/task-brief` from there.
   */
  brief_next: number;
  /**
   * The revision of the whole brief, which moves whenever its text does: what was read of one
   * revision is never shown after the part another revision's page sent.
   */
  brief_revision: string;
  /**
   * Where each of the task's role holders' Go to session controls go, by handle: a holder with
   * none known is left out ([`SessionGo`]).
   */
  sessions: { [key in string]: SessionGo };
  /**
   * The page was asked from the server's own machine, where a pane is focused rather than its
   * screen shown ([`SessionGo::control`]).
   */
  local: boolean;
  /**
   * How many gates the last answer the server gave for the task had, and how many it waited on:
   * the room the page holds for its gates while it reads them. `None` while the server has given
   * none since it started.
   */
  gates?: GatesSeen | null;
};

export type TaskEntry = { message: Message; stamp: string; blocks: Array<Block> };

export type Message = {
  id: number;
  slug: string;
  sender: string;
  recipient: string | null;
  kind: string;
  text: string;
  ts: number;
};

export type TaskHistory = {
  entries: Array<TaskEntry>;
  before: number | null;
  after: number | null;
};

export type TaskBrief = { blocks: Array<Block>; next: number; omitted: number; revision: string };

export type TaskOffer = {
  /**
   * The board action's name.
   */
  action: string;
  label: string;
  branch: string;
  payload: string | null;
};

export type TaskGates = {
  slug: string;
  /**
   * The branch the gates were answered for: the task's recorded branch.
   */
  branch: string;
  /**
   * The branch's tip, a full commit id; `None` when the project's repository has no such branch.
   */
  tip?: string | null;
  /**
   * In [`GateState::rank`] order. Empty when nothing gates the task.
   */
  gates: Array<Gate>;
};

export type Gate = {
  /**
   * Its name, unique among a task's gates (`review`, `certificate`).
   */
  name: string;
  state: GateState;
  /**
   * Who met, waived or holds it, or whose run or verdict it reads: a handle, `owner`, or a
   * pipeline (`nightly`).
   */
  by?: string | null;
  /**
   * When it took this state; for a run still going, when the run started.
   */
  at?: number | null;
  /**
   * The commit it holds for, a full or short id.
   */
  commit?: string | null;
  /**
   * Why, for a waiver or a hold.
   */
  why?: string | null;
  /**
   * One line saying what it is in this state.
   */
  detail: string;
  /**
   * Where its evidence is: an app path (`/task/SLUG#message-ID`, `/docs/X.md#card`) or an
   * `http(s)` URL. Any other (`symphony_api::linkable`) is dropped before a page sees it.
   */
  link?: string | null;
  /**
   * The states it took before this one, oldest first, for the timeline's gates lane; empty
   * when its answerer keeps none. Each lasted until the next began, the last one until `at`.
   */
  past?: Array<GatePast>;
};

export type GatePast = {
  state: GateState;
  /**
   * When it took that state.
   */
  at: number;
  by?: string | null;
  commit?: string | null;
  detail: string;
};

export type GateState = "missing" | "held" | "waived" | "met" | "unknown";

export type GatesSeen = { count: number; waiting: number; unsigned: number };

export type TaskTimeline = {
  slug: string;
  /**
   * When the server answered: where whatever still goes on ends. The answer's validator leaves
   * it out, so a read that finds nothing else changed answers `304`: a client that keeps the
   * answer counts on from this by its own clock.
   */
  now: number;
  /**
   * The server's local clock over the timeline's hours: its offset from UTC at `created`, then
   * each change of it (summer time), oldest first.
   */
  zone: Array<Offset>;
  /**
   * How often, in seconds, a client reads the timeline again while the task is open;
   * [`TIMELINE_EVERY`] when the answer does not say.
   */
  every?: number;
  /**
   * The oldest run the board's recorder still keeps, where the task began before it: the task's
   * runs before then may have fallen out of the recorder's history.
   */
  runs_from?: number | null;
  /**
   * When the task was made.
   */
  created: number;
  /**
   * When it closed, done or cancelled; none while it is open.
   */
  closed?: number | null;
  /**
   * When an executor first claimed it; the time before it, in ready, is time unclaimed.
   */
  first_claimed?: number | null;
  /**
   * How often review sent it back to in progress.
   */
  bounces: number;
  /**
   * Each stretch in one status, oldest first, one after another from `created`.
   */
  stages: Array<Stage>;
  /**
   * Who held the executor role while it was in progress, oldest first.
   */
  claims: Array<Claim>;
  /**
   * Each review round, oldest first.
   */
  rounds: Array<Round>;
  /**
   * Its builds and tests, by when they were queued. Empty until the board's recorder keeps one
   * for the task: then a client shows no builds or tests at all, never a zero.
   */
  runs: Array<Run>;
  /**
   * Each gate's states, by the gate's name; empty when the task has none, or its gates could
   * not be read.
   */
  gates: Array<GateLane>;
};

export type Offset = { from: number; seconds: number };

export type Stage = {
  /**
   * A board status: `ready`, `in_progress`, `review`, `blocked`, `done`…
   */
  status: string;
  from: number;
  /**
   * When it left it; none for the status it holds.
   */
  to?: number | null;
  /**
   * Who moved it there, where its line says.
   */
  by?: string | null;
  /**
   * For a stretch in review, its round, from 1; 0 otherwise.
   */
  round?: number;
  /**
   * A stretch in review after its round was approved: the task waits to land.
   */
  approved?: boolean;
};

export type Claim = {
  by: string;
  from: number;
  /**
   * When it let go, or the task left in progress; none while it works on it.
   */
  to?: number | null;
};

export type Round = {
  /**
   * Its number, from 1 (`round` on the wire).
   */
  round: number;
  /**
   * When the task went to review.
   */
  from: number;
  /**
   * When a reviewer took it up; the time before is the wait for one. Its start when a reviewer
   * already held the role; none while nobody has.
   */
  taken?: number | null;
  /**
   * Its verdict's time, or when the task left review without one; none while it goes on.
   */
  to?: number | null;
  /**
   * The reviewer.
   */
  by?: string | null;
  /**
   * None while it goes on, or when it ended without one.
   */
  verdict?: RoundVerdict | null;
};

export type RoundVerdict = "approved" | "changes" | "other";

export type Run = {
  /**
   * `build` or `test`.
   */
  phase: string;
  /**
   * The recipe or command line (`just test-rs -p symphony-board`).
   */
  label: string;
  /**
   * Who ran it.
   */
  by?: string | null;
  /**
   * The commit it ran at.
   */
  commit?: string | null;
  /**
   * The machine it ran on.
   */
  host?: string;
  queued: number;
  started?: number | null;
  ended?: number | null;
  result: RunResult;
};

export type RunResult = "queued" | "running" | "green" | "red" | "unknown";

export type GateLane = {
  name: string;
  /**
   * Oldest first, each lasting until the next began, the last one until the timeline's end.
   */
  states: Array<GateSpan>;
};

export type GateSpan = {
  state: GateState;
  /**
   * When it took it; a state taken before the task was made, or at a time its answerer does not
   * keep, from the task's creation.
   */
  from: number;
  by?: string | null;
  detail: string;
};
