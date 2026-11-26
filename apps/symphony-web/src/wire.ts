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
export const ASK_LATE_SECONDS = 600;
export const FILE_QUERY = "raw";
export const REPORTS_PATH = "/api/reports";
export const REPORTS_LANDED_PATH = "/api/reports/landed";
export const SETTINGS_PATH = "/api/settings";
export const SETTING_WRITE_PATH = "/api/setting";
export const PEOPLE_PATH = "/api/people";
export const PEOPLE_CHANGE_PATH = "/api/people-change";
export const PROFILE_PATH = "/api/people-profile";
export const CARD_OWNERS_PATH = "/api/card-owners";
export const TASK_HAND_ROUTE = "/api/task-hand";
export const CARD_HAND_ROUTE = "/api/card-hand";
export const RUNS_PATH = "/api/runs";
export const RUN_PATH = "/api/run";
export const RUN_LOG_PATH = "/api/run-log";
export const RUN_LOG_TAIL = 65536;
export const RUN_LOG_MOST = 1048576;
export const RUNS_DAYS = 14;
export const SCHEMA_UPDATES_PATH = "/api/schema-updates";
export const VERSION_HEADER = "symphony-version";
export const GETTING_STARTED =
  "https://github.com/heavenstudio/symphony/blob/main/docs/operations/GETTING_STARTED.md";
export const VERDICT_TASKS = "verdicts/";
export const UPLOAD_PATH = "/api/upload";
export const UPLOAD_MAX_BYTES = 10485760;
export const UPLOAD_ACCEPT =
  "image/png,image/jpeg,image/gif,image/webp,application/pdf,text/plain,text/markdown,text/csv,application/json,.png,.jpg,.jpeg,.gif,.webp,.pdf,.txt,.md,.csv,.json,.log,.docx,.xlsx,.pptx";
export const QUICK_TASK_TITLE_MAX = 200;
export const TASK_ACTION_ROUTE = "/api/task-action";
export const TASK_PASS_ROUTE = "/api/task-pass";
export const DOCUMENT_HELD_PATH = "/api/document-held";
export const DOCUMENT_PASS_ROUTE = "/api/document-pass";
export const TASK_TOKENS_PATH = "/api/task-tokens";
export const TASK_COSTS_PATH = "/api/task-costs";
export const TASK_COSTS_MOST = 200;
export const TASK_GATES_PATH = "/api/task-gates";
export const TASK_TIMELINE_PATH = "/api/task-timeline";
export const TASK_DIFF_PATH = "/api/task-diff";
export const TASK_DIFF_FILE_PATH = "/api/task-diff-file";
export const TIMELINE_EVERY = 60;
export const TIMELINE_HISTORY_LIMIT = 256;
export const TIMELINE_HISTORY_SOURCES = 8;
export const TASK_ATTEMPTS_PATH = "/api/task-attempts";
export const ATTEMPT_CONTROL_ROUTE = "/api/attempt-control";
export const ATTEMPT_VIEW_ROUTE = "/api/attempt-view";
export const AGENT_FOCUS_PATH = "/api/agent-focus";
export const AGENT_SCREEN_PATH = "/api/agent-screen";
export const WORKFLOW_PATH = "/api/workflow";
export const WORKFLOW_ACTIONS_PATH = "/api/workflow-actions";
export const WORKFLOW_APPLY_ROUTE = "/api/workflow-apply";
export const WORKFLOW_GUARDS_PATH = "/api/workflow-guards";
export const SYMPHONY_ROUTES = [
  "tasks",
  "task",
  "agents",
  "queue",
  "reports",
  "runs",
  "settings",
  "people",
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
  "favicon.ico",
] as const;
export const PAGE_NAMES = {
  HOME: "Home",
  QUEUE: "Queue",
  DOCS: "Docs",
  BOARD: "Board",
  REPORTS: "Reports",
  SETTINGS: "Settings",
  PEOPLE: "People",
  RUNS: "Runs",
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
   * The review guide as a link names it, by its route (`docs/workflow/REVIEW.md`); empty when the
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
  /**
   * The name the header opens with in place of the Symphony's wordmark: the theme package's,
   * once the project's theme has a name or an icon of its own. Empty is the Symphony's.
   */
  wordmark?: string;
  /**
   * The icon the header opens with in place of the Symphony's mark: the theme package's own,
   * its file project-relative with no slash first, served under the base; in the page the
   * server serves, with the version (`?v=`) its answer is kept for good under. Empty is the
   * Symphony's.
   */
  mark?: string;
  /**
   * Who reads the page, as the header names them and links to their profile: the person at the
   * machine (`owner` until they give themselves a name), or a person a board named. None in a
   * page served for nobody in particular.
   */
  you?: PersonName | null;
  /**
   * Where who reads the page writes their profile, a path of the project: the person at the
   * machine's own section of the People page (`/people#you`), else their own card there
   * (`/people#person-NAME`). Empty with no one to name.
   */
  profile?: string;
  /**
   * The person at the machine, once they gave themselves a name: every line they wrote, under
   * it and under `owner` before it, is drawn with how they are shown. None while they gave none,
   * and their lines read `owner`.
   */
  machine?: PersonName | null;
};

export type PersonName = {
  /**
   * The short name every line of theirs carries: `owner` for the person at the machine while
   * they gave themselves none.
   */
  name: string;
  /**
   * Their display name, else their short name.
   */
  shown: string;
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
  /**
   * Comments held on documents that wait on the one reading to pass them on ([`Queue::held`]).
   */
  held?: number;
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
  /**
   * The comments held on documents that wait on the one reading to pass them on, oldest first:
   * each where it was left, never its words, which its document's page shows with Pass on.
   */
  held?: Array<QueueHeld>;
};

export type QueueItem = {
  href: string;
  path: string;
  slug: string;
  title: string;
  kind: string;
  text: string;
  branch?: string | null;
  /**
   * The owner's open ask a card waiting on an agent is on the board; none for a comment, a card
   * to spot-check, a card no ask is open for, and until the board is read.
   */
  ask?: QueueAsk | null;
};

export type QueueAsk = {
  /**
   * Its number on the board: `ask N`.
   */
  id: number;
  /**
   * When the owner said it, in whole seconds since the epoch: its age runs from here, and past
   * [`ASK_LATE_SECONDS`] it is late.
   */
  asked: number;
  /**
   * The session that holds it; none is nobody.
   */
  holder?: string | null;
  /**
   * Since when its holder holds it, in whole seconds since the epoch.
   */
  held?: number | null;
};

export type QueueHeld = {
  /**
   * Its number among the comments held on documents ([`HeldComment::id`]).
   */
  id: number;
  /**
   * Where it is drawn: its document's page, at its box, `/DOC#held-N`, a branch's page under
   * its scope (`/@scope/DOC#held-N`).
   */
  href: string;
  /**
   * The document's title.
   */
  doc_title: string;
  /**
   * Who wrote it, by their short name.
   */
  author: string;
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
  /**
   * What it changed, each once: `settings`, `rules` and `skill NAME`. A server from before the
   * instructions were marked names none, and its marks are settings changes.
   */
  changed: Array<string>;
};

export type Spend = { tokens: AgentTokens; cost: AgentCost };

export type LandedPage = {
  grain: Grain;
  /**
   * The days the window reaches back, as [`Grain::days`] took the ask.
   */
  days: number;
  /**
   * The first second of its first period and where it ends, as [`ReportsPage`]'s.
   */
  from: number;
  to: number;
  /**
   * Each one the board recorded tokens for, the dearest first, then by slug: the first ten are
   * the dearest, and all of them the dots and the table.
   */
  tasks: Array<LandedRow>;
  /**
   * Those it recorded none for: in no figure.
   */
  without: number;
  /**
   * The window's figures, of the tasks with a priced cost.
   */
  figures: LandedFigures;
  /**
   * By the kind each was given, those that cost the most in all first, the tasks given none
   * last.
   */
  kinds: Array<KindRow>;
  /**
   * By how many reviews each took, the fewest first, the last one of four or more.
   */
  reviews: Array<ReviewRow>;
  /**
   * When the first landing of the window that names a kind landed; none when none does, as in
   * a window from before kinds were kept.
   */
  kinds_from: number | null;
  /**
   * The day the price table's prices were read (`YYYY-MM-DD`).
   */
  prices: string;
  /**
   * What was not read, each in a sentence naming its file and why: a task it held is missing.
   */
  unread: Array<string>;
};

export type LandedRow = {
  slug: string;
  /**
   * When it landed.
   */
  at: number;
  /**
   * Its kind; none for one given none.
   */
  kind: string | null;
  /**
   * What its work up to its first review cost, and every round after it.
   */
  first: number;
  later: number;
  /**
   * Its tokens, all told.
   */
  tokens: number;
  /**
   * Whether some of its tokens had no price, and are in no dollar here; one none of whose had
   * one has no cost to rank, and is in no figure.
   */
  unpriced: boolean;
  /**
   * How many reviews it took: the last round it took tokens in, less the first.
   */
  reviews: number;
  /**
   * The lines its landing changed, added and removed; none where they were not read.
   */
  lines: number | null;
};

export type LandedFigures = {
  /**
   * The tasks with a priced cost.
   */
  tasks: number;
  median: number | null;
  mean: number | null;
  p90: number | null;
  /**
   * What they cost, all told, and what their review rounds cost of it.
   */
  cost: number;
  later: number;
  /**
   * Those back after review: reviewed twice or more, a round past the second.
   */
  back: number;
  /**
   * Those whose landing changed a counted line, what those cost and their lines.
   */
  lined: number;
  lined_cost: number;
  lines: number;
  /**
   * Those whose landing changed no counted line, and those whose lines were not read.
   */
  unlined: number;
  unread: number;
};

export type KindRow = {
  /**
   * The kind; none for the tasks given none.
   */
  kind: string | null;
  tasks: number;
  median: number | null;
  /**
   * What they cost, all told.
   */
  cost: number;
  /**
   * Of them, what those whose landing changed a counted line cost, and those lines.
   */
  lined_cost: number;
  lines: number;
};

export type ReviewRow = {
  /**
   * The reviews; the last row's is four, for four or more.
   */
  reviews: number;
  tasks: number;
  /**
   * The mean cost of one's first round and of its review rounds.
   */
  first: number;
  later: number;
  median: number | null;
};

export type SettingsPage = {
  /**
   * The files read, the project file first.
   */
  files: Array<SettingsFile>;
  /**
   * What is wrong with a file, each said whole with what holds in its place: a project file
   * that does not load, a local file that is refused, a limit that cannot be read, a board's
   * store that cannot be read or an entry of it that is not.
   */
  problems?: Array<string>;
  sections: Array<SettingsSection>;
  /**
   * The settings the page writes, each a preference the board's store keeps: one for each it
   * draws a control for, whether the store has an entry for it or not.
   */
  stored?: Array<StoredSetting>;
};

export type SettingsFile = {
  /**
   * Its name (`symphony.toml`).
   */
  name: string;
  scope: SettingScope;
  /**
   * Where it is, or would be.
   */
  path: string;
  /**
   * Whether it is there.
   */
  found: boolean;
};

export type SettingsSection = { id: string; title: string; entries: Array<SettingsEntry> };

export type SettingsEntry = {
  /**
   * Stable among the page's entries: its anchor.
   */
  id: string;
  name: string;
  /**
   * What it is for, which no page draws (RULES.md `code/pages-explain-nothing`): kept, and
   * read when left out, for the plugins that write it, until a later release removes it
   * (`code/consumer-api-two-releases`).
   */
  about: string;
  state?: EntryState | null;
  needs?: Array<EntryNeed>;
  settings?: Array<Setting>;
  /**
   * Where it is said as a sentence in a file: for an entry that carries no setting of its own.
   */
  source?: SettingSource | null;
};

export type EntryState = {
  /**
   * Its state in a word or two (`On`, `Off`, `None selected`).
   */
  word: string;
  /**
   * On or off, for an entry that is one of the two.
   */
  on?: boolean | null;
  /**
   * Why it is so.
   */
  why: string;
};

export type EntryNeed = {
  /**
   * What is needed (`the claude program`).
   */
  what: string;
  met: boolean;
  /**
   * What was found, or what is missing and what that means.
   */
  said: string;
};

export type Setting = {
  /**
   * Its table and key as the file spells them (`[limits] low_volume`), or the table alone for
   * one said as a sentence (`[land]`).
   */
  key: string;
  /**
   * What it is, in words.
   */
  name: string;
  kind: SettingKind;
  /**
   * The value in force, as shown.
   */
  value: string;
  /**
   * What a [`SettingKind::Choice`] may be.
   */
  choices?: Array<string>;
  source: SettingSource;
  /**
   * The scopes it may be written in.
   */
  scopes: Array<SettingScope>;
  class: SettingClass;
};

export type SettingSource = {
  /**
   * The scope of the file that wrote it; none for a default, which no file writes.
   */
  scope?: SettingScope | null;
  /**
   * Where it stands, whole, as `just tasks limits` names one
   * (`symphony.local.toml [limits] machine_agents, line 6`), or the default said as one.
   */
  said: string;
  /**
   * The file's name, when a file wrote it.
   */
  file?: string | null;
  /**
   * Its line there, when it can be found.
   */
  line?: number | null;
  /**
   * What the value stands over: for a local file's, where the committed one is written, said as
   * [`SettingSource::said`] says one; for the board's store's, the committed value and where
   * (`4 (symphony.toml [limits] low_volume, line 5)`); for a local file's over the store's, the
   * stored value and the store.
   */
  over?: string | null;
};

export type SettingScope = "project" | "board" | "machine" | "device";

export type SettingClass = "preference" | "reviewed";

export type SettingKind = "text" | "number" | "switch" | "choice" | "list" | "sentence";

export type StoredSetting = {
  /**
   * The setting, by its key on the page ([`Setting::key`]: `[limits] low_volume`).
   */
  setting: string;
  /**
   * Its key in the store, which a write names (`limits.low_volume`).
   */
  key: string;
  /**
   * What the store's entry holds, as shown; none with no entry, or once it was removed.
   */
  value?: string | null;
  /**
   * Whether that value is the one in force: not where the local file writes the setting, nor
   * where the store does not read its entry.
   */
  held: boolean;
  /**
   * Why the store does not read its entry, when it refuses it.
   */
  refused?: string | null;
  /**
   * Every change of the entry, oldest first.
   */
  changes?: Array<StoredChange>;
  /**
   * Where making it the project's stands while its commit waits for its delivery by pull
   * request: the branch that delivers it, and how far it went.
   */
  waiting?: string | null;
};

export type StoredChange = {
  before?: string | null;
  after?: string | null;
  /**
   * When, as the board says a minute (`2026-11-22 20:15`).
   */
  at: string;
  by: string;
};

export type SettingAction = "store" | "remove" | "commit";

export type SettingValue = boolean | number | string;

export type SettingWrite = {
  /**
   * The setting, by its key in the store ([`StoredSetting::key`]).
   */
  key: string;
  action: SettingAction;
  /**
   * The value [`SettingAction::Store`] keeps; none for the others.
   */
  value?: SettingValue | null;
};

export type SettingWritten = {
  /**
   * What it did, in a sentence.
   */
  said: string;
  /**
   * The commit made on the default branch, for a commit made there.
   */
  commit?: string | null;
  /**
   * Whether the commit waits for its delivery by pull request, its entry kept meanwhile.
   */
  waiting: boolean;
};

export type PeoplePage = {
  /**
   * The people the board named, by short name.
   */
  people: Array<PersonRow>;
  /**
   * Its groups, by name.
   */
  groups: Array<GroupRow>;
  /**
   * Every change made to them, oldest first.
   */
  log: Array<PeopleLogLine>;
  /**
   * The number of the newest change: 0 on a board none was made on.
   */
  revision: number;
  /**
   * Whether the one reading may change who is on the board: the person at the machine, or a
   * person holding administer.
   */
  administers: boolean;
  /**
   * Whether the one reading is the person at the machine, who writes any profile.
   */
  machine: boolean;
  /**
   * The one reading's short name on the board; none for the person at the machine.
   */
  you?: string | null;
  /**
   * Why the board's people could not be read, said whole; the page then lists nobody.
   */
  problem?: string | null;
  /**
   * The person at the machine's own profile, which their You section shows and writes: for
   * them alone, none for anyone else.
   */
  mine?: MachineProfile | null;
  /**
   * The sign-in services this server knows a visitor by, which an add and a profile take a
   * sign-in of: `tailscale` while this machine is on a tailnet, none where it is on none.
   */
  signins?: Array<string>;
  /**
   * Whether a signing service's words are read here, the machine folder's `sign-in.toml`
   * setting its key: an add and a profile then take a sign-in of the service a word names,
   * which the key does not say (`docs/operations/SIGNED_IN.md`).
   */
  signed_words?: boolean;
};

export type PersonRow = {
  name: string;
  /**
   * How pages show them: their profile's display name, else their short name.
   */
  display: string;
  /**
   * Every power they hold, their own and their groups', read among them.
   */
  powers: Array<string>;
  /**
   * The powers given to them, not their groups'.
   */
  own: Array<string>;
  groups: Array<string>;
  /**
   * The services they sign in by, each once: never a subject.
   */
  signins: Array<string>;
  /**
   * Their git handles.
   */
  git: Array<string>;
  /**
   * Whether the machine holds a profile of theirs.
   */
  profile: boolean;
  /**
   * Where their notices reach them: in the answer of the person themself, of the person at the
   * machine and of the board's admins; in no one else's.
   */
  addresses?: Addresses | null;
  /**
   * The way each kind of notice goes by, every kind the board knows in its order: in the answer
   * of the person themself and of the person at the machine; in no one else's.
   */
  notices?: Array<NoticeWay> | null;
};

export type Addresses = {
  /**
   * The email a notice by mail goes to: the first of their emails.
   */
  email?: string | null;
  /**
   * Their Slack member id (`U024BE7LH`).
   */
  slack?: string | null;
  /**
   * Their WhatsApp number, `+` and its digits.
   */
  whatsapp?: string | null;
};

export type NoticeWay = {
  /**
   * The kind, as a notice writes it: `card`, `question`, `verdict`.
   */
  kind: string;
  /**
   * The way it goes by, `email`, `slack` or `whatsapp`; none where it goes by none, as the page
   * and the Mac app show every notice.
   */
  way?: string | null;
};

export type GroupRow = { name: string; powers: Array<string> };

export type PeopleLogLine = {
  /**
   * Its number, from 1.
   */
  n: number;
  /**
   * When, in seconds since the epoch.
   */
  at: number;
  /**
   * Who made it: a session's handle, a person's short name, or `owner`.
   */
  by: string;
  /**
   * What it did, as the log writes it (`add`, `group-remove`).
   */
  did: string;
  name: string;
  group?: string | null;
  powers?: Array<string>;
};

export type PeopleAction =
  | "add"
  | "remove"
  | "join"
  | "leave"
  | "grant"
  | "revoke"
  | "group-add"
  | "group-remove";

export type PeopleChange = {
  action: PeopleAction;
  /**
   * The person's short name; the group's for a group's add or removal.
   */
  name: string;
  /**
   * The group a join or a leave names, or whose powers a grant or a revoke changes.
   */
  group?: string | null;
  /**
   * The powers an add, a group's add, a grant or a revoke names.
   */
  powers?: Array<string>;
  /**
   * The groups an add puts the person in.
   */
  groups?: Array<string>;
  /**
   * An add's profile, made where the person has none: their display name.
   */
  display?: string | null;
  /**
   * An add's profile: the sign-ins it is given, each `SERVICE:SUBJECT`.
   */
  signins?: Array<string>;
  /**
   * An add's profile: its git handles.
   */
  git?: Array<string>;
};

export type ProfileWrite = {
  /**
   * The person whose profile it is.
   */
  name: string;
  display?: string | null;
  /**
   * Git handles added to the profile.
   */
  git?: Array<string>;
  /**
   * Sign-ins added to it, each `SERVICE:SUBJECT`: the person at the machine's alone to give.
   */
  signins?: Array<string>;
  /**
   * The email a notice by mail goes to, in place of the first of their emails, those after it
   * kept; an empty one takes the first away.
   */
  email?: string | null;
  /**
   * Their Slack member id; an empty one takes it away.
   */
  slack?: string | null;
  /**
   * Their WhatsApp number, `+` and its country code; an empty one takes it away.
   */
  whatsapp?: string | null;
  /**
   * The ways kinds of notice go by, each kind named set: a way of none sends it by none.
   */
  notices?: Array<NoticeWay>;
  /**
   * Whether it gives the person at the machine `name` as their own short name, once and for
   * good: theirs alone to ask, and refused once they have one.
   */
  machine?: boolean;
};

export type MachineProfile = {
  /**
   * The short name they gave themselves, fixed once given: none until then, and their lines
   * read `owner`.
   */
  name?: string | null;
  /**
   * Their display name: empty where they gave none.
   */
  display?: string;
  /**
   * Their git handles.
   */
  git?: Array<string>;
};

export type Owner = {
  /**
   * The group's or the person's short name, or `owner` for the person at the machine.
   */
  name: string;
  /**
   * `group`, `person` or `machine`.
   */
  kind: string;
  /**
   * What gave it: `named` (a name written on it), `map` or `default`.
   */
  by: string;
  /**
   * What gave it, as words a page shows beside the name: `named on it`, `from the area app`.
   */
  said: string;
  /**
   * Whom the one reading may hand it to, groups then people: empty where they may not.
   */
  hand_to?: Array<string>;
  /**
   * Whether the one reading may decide it: a card's verdict, a task's move, priority, cancel
   * and approve. A page draws those controls for them alone.
   */
  decides: boolean;
};

export type CardOwners = { cards: { [key in string]: Owner } };

export type DocumentHeld = {
  held: Array<HeldComment>;
  /**
   * The one reading may pass them on ([`DOCUMENT_PASS_ROUTE`]): the person at the machine, or
   * a person holding speak and direct agents.
   */
  passes: boolean;
};

export type HeldComment = {
  /**
   * Its number, which a pass names.
   */
  id: number;
  /**
   * Who wrote it, by their short name.
   */
  author: string;
  /**
   * A callout's kind (`comment`, `question`, `decision`), or `message` for a card's message.
   */
  kind: string;
  /**
   * Their words; a callout's title is the first line.
   */
  text: string;
  /**
   * The card, for a message.
   */
  card?: string | null;
  /**
   * The section a callout goes after, by its key ([`Section::key`]): the one its pass would
   * write it after now, the block it was left after or, that block changed, the end of its
   * section; else the one it was left after.
   */
  key?: string | null;
  /**
   * That section's last line.
   */
  line?: number | null;
};

export type RunsPage = {
  /**
   * The runs that match, newest first by [`RunLine::queued`], each once ([`fold`]): its end
   * line, else its start line.
   */
  runs: Array<RunLine>;
  /**
   * Whether more runs match, asked for before the last of these.
   */
  more: boolean;
  /**
   * The pipelines the runs read name, by name: what the page offers to filter by.
   */
  pipelines: Array<string>;
  /**
   * The day files that were not read, each a sentence naming it: their runs are missing.
   */
  unread: Array<string>;
};

export type RunPage = {
  run: RunLine;
  /**
   * Its steps and theirs, each after the one it ran inside, in the order they were asked for.
   */
  steps: Array<RunLine>;
  /**
   * The day files that were not read, each a sentence naming it.
   */
  unread: Array<string>;
};

export type RunLine = {
  /**
   * [`RUN_SCHEMA`].
   */
  schema: number;
  /**
   * 32 hex digits: the id of the operation that recorded it, which its live record in the work
   * folder carries too.
   */
  run: string;
  /**
   * The run or step this is a step of; none for a run.
   */
  parent: string | null;
  /**
   * The project's id.
   */
  project: string | null;
  /**
   * The machine's installation id, when it has one.
   */
  machine: string | null;
  /**
   * Its run's pipeline ([`PIPELINES`]), a step's too.
   */
  pipeline: string;
  /**
   * The step's name; none for a run.
   */
  step: string | null;
  /**
   * The recipe or the command line.
   */
  label: string;
  task: string | null;
  /**
   * The session it ran for.
   */
  handle: string | null;
  /**
   * Who or what started it: a session's handle, or one of [`TRIGGERS`].
   */
  trigger: string;
  branch: string | null;
  /**
   * The commit it checked.
   */
  commit: string | null;
  /**
   * That commit's tree.
   */
  tree: string | null;
  /**
   * Whether the checkout differed from its commit (tracked changes, or files git does not
   * ignore) when it began: its tree then says only what it started from.
   */
  dirty: boolean | null;
  /**
   * When it was asked for, started and ended, in seconds since the epoch on the recording
   * machine's clock.
   */
  queued: number;
  started: number | null;
  ended: number | null;
  /**
   * None on a start line.
   */
  end: End | null;
  exit: number | null;
  signal: number | null;
  /**
   * The CPU seconds, user and system, of its command and every child it reaped; a run's
   * include its steps'.
   */
  cpu_s: number | null;
  /**
   * Beside `cpu_s`, whether its recorder knows it leaves out CPU the command's processes took
   * (`true`), saw no such loss (`false`), or could not watch for one (absent): docs/workflow/RUNS.md.
   */
  cpu_partial?: boolean | null;
  /**
   * The newest earlier run of the same pipeline, step and label on the same tree, both clean.
   */
  rerun_of: string | null;
  /**
   * Its log, relative to the day files' folder: `logs/YYYY-MM-DD/RUN.log`, the day it was asked
   * for. Named on its start line too, so a run that still runs can be read.
   */
  log?: string | null;
  /**
   * The bytes its log held as its end line was written.
   */
  log_bytes?: number | null;
  /**
   * Per test binary its log names, on an end line; absent when it names none.
   */
  suites?: Array<Suite> | null;
  /**
   * The tests that failed, beside `suites`: empty when none did.
   */
  failed?: Array<FailedTest> | null;
};

export type End = "green" | "red" | "timed_out" | "interrupted" | "incomplete";

export type Suite = {
  /**
   * Cargo's words for it, the executable's hash cut: `unittests src/lib.rs (symphony_board)`,
   * `tests/cli.rs (cli)`, `Doc-tests symphony_board`.
   */
  name: string;
  passed: number;
  failed: number;
  ignored: number;
  /**
   * Its time, in seconds.
   */
  s: number;
  /**
   * Whether it ended with no result line (it crashed, or its run was stopped first): never a
   * pass, its counts and time those of the results it said, none.
   */
  error?: boolean;
};

export type FailedTest = {
  /**
   * The binary it is in, when known.
   */
  suite: string | null;
  name: string;
  /**
   * Its failure's first line.
   */
  line: string;
};

export type RunLog = {
  /**
   * The run's id.
   */
  run: string;
  /**
   * The log, relative to the day files' folder, as [`RunLine::log`] names it.
   */
  log: string;
  /**
   * Its whole length when read, in bytes: a run that still runs adds to it.
   */
  size: number;
  /**
   * Where the bytes answered start in it.
   */
  from: number;
  /**
   * Where they end, never past `size`.
   */
  to: number;
  /**
   * Those bytes as text; a character a range cuts through reads as U+FFFD.
   */
  text: string;
};

export type SchemaUpdates = {
  /**
   * Every open update and the [`SCHEMA_UPDATES_SHOWN`] newest, newest first: the order the
   * ledger was written in, turned round.
   */
  updates: Array<SchemaUpdate>;
  /**
   * Why the ledger could not be read, in a sentence naming it; none while it was read, or there
   * is none yet.
   */
  unread: string | null;
};

export type SchemaUpdate = {
  /**
   * `ID@N`, `ID@N.G` from its second generation.
   */
  id: string;
  /**
   * The project whose board it is: its id on the machine's server.
   */
  project: string;
  /**
   * The schema its board reached.
   */
  schema: number;
  /**
   * Its generation: 1, then 2 for the board's next arrival at the same schema after it went back.
   */
  generation: number;
  state: UpdateState;
  /**
   * When its board reached the schema, as far as a record says.
   */
  reached: UpdateTime | null;
  /**
   * When the first green release serving the schema was installed on the machine.
   */
  available: UpdateTime | null;
  /**
   * When the machine's server first wrote the board at the schema.
   */
  served: UpdateTime | null;
  /**
   * From reached to served, in seconds, once both are known.
   */
  total: number | null;
  /**
   * [`SchemaUpdate::total`] as the owner reads a wait: `4 h 46 min`, `3 min 10 s`, `40 s`.
   */
  total_said: string | null;
  /**
   * How much of the wait came after the release was installed, in seconds, once both are known.
   */
  after_available: number | null;
  /**
   * [`SchemaUpdate::after_available`] as the owner reads a wait.
   */
  after_available_said: string | null;
  /**
   * How many refreshes ended with it incomplete though a green release serving it was here.
   */
  failures: number;
  /**
   * What the last of them found; none while none failed.
   */
  failure: string | null;
  /**
   * The page of its task, filed at its first failure, on the server that answers: an app path
   * (`/<project>/task/SLUG` under the machine's one server, while the project is registered);
   * none while it has none, its board is an unregistered one's, or its task is another
   * project's board's and the server serves its one project alone.
   */
  task: string | null;
};

export type UpdateState = "open" | "served" | "left";

export type UpdateTime = {
  /**
   * Seconds since the epoch.
   */
  at: number;
  /**
   * Where it comes from, named as `release --status` names it, the record's words before their
   * first colon: `schema-seen.log`, `release-2026-11-19's manifest, written by its install`,
   * `the server's hand-off`, `this refresh's move`, or a refresh's own look.
   */
  source: string;
  /**
   * The record's words whole, its name first and then what it says of the time (`schema-seen.log:
   * the first store that saw the board at this schema; …`), as the ledger holds them.
   */
  words: string;
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
  /**
   * Each session holding the orchestrator role on an open task, by handle, whatever the
   * listing selects: the board says once when none is live.
   */
  orchestrators: Array<Orchestrator>;
  /**
   * The board's ready tasks no live executor holds with nothing open before them, whatever the
   * listing selects ([`crate::Tasks::unstarted`]).
   */
  unstarted: number;
  /**
   * Complete totals for the opt-in waiting-columns projection; absent on ordinary listings.
   */
  waiting?: WaitingBoard | null;
};

export type WaitingBoard = {
  /**
   * Open tasks by [`crate::WAITS_KINDS`], in that order, before the selected wait kind.
   */
  kinds: Array<[string, number]>;
  /**
   * Every column after the selected wait kind, including columns outside the requested page.
   */
  columns: Array<WaitingColumn>;
  /**
   * Open tasks a custom workflow holds by their coordination state's ID, ordered by ID, before
   * the selected wait kind and state: empty, and left out, on a board with none.
   */
  states?: Array<[string, number]>;
  /**
   * The active custom workflow's activation generation: absent on a board whose own writers keep
   * its tasks, so a page draws a workflow's phases and states only where one is active, and
   * reads its definition again when it changes (docs/workflow/WORKFLOW.md, "The app").
   */
  workflow?: number | null;
};

export type WaitingColumn = {
  status: string;
  ordinary: number;
  blocked: number;
  /**
   * Seconds since the epoch, not a changing age; none for an empty column.
   */
  oldest: number | null;
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
  /**
   * Replayed stretches in review, including an active round, for open and finished tasks.
   * None means the summary is unavailable (including older servers), not zero reviews.
   */
  review_rounds?: number | null;
  /**
   * What an open task waits on; none for a closed one, or from a server before it.
   */
  waits_on?: WaitsOn | null;
  /**
   * The status a task the board holds blocked was blocked from, where it is known. A block that
   * names nothing is not blocked on the wire: the task is sent in that status, `from` beside it,
   * and its [`WaitsOn`] says no reason is recorded (docs/workflow/TASKS.md, what a task waits on).
   */
  from?: string | null;
  /**
   * Where a custom workflow holds it ([`workflow::TaskPlace`]): none for a task the board's own
   * writers keep, every task of a board with no custom workflow among them, and from a server
   * before it.
   */
  workflow?: WorkflowPlace | null;
};

export type WaitsOn = {
  /**
   * One of [`WAITS_KINDS`]. A client that does not know a kind draws it as `nobody`.
   */
  kind: string;
  /**
   * Who or what: the holder at work (`amber-heron`, `amber-heron (stale)`), the decision's
   * name, the checks' names, the task's slug; empty where the kind names nothing.
   */
  who: string;
  /**
   * Why, in a few words: the role at work, a block's own words, what a landing lacks.
   */
  why: string;
  /**
   * When the task took the status it holds, in seconds since the epoch.
   */
  since: number;
};

export type Orchestrator = { handle: string; health: string };

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
  /**
   * Present when the known review-round count changes. Losing the summary replaces the task.
   */
  review_rounds?: number | null;
  /**
   * Present only when what it waits on changes; one of no kind (empty) clears it.
   */
  waits_on?: WaitsOn | null;
  /**
   * Present only when the status it was blocked from changes. Empty string clears it.
   */
  from?: string | null;
  /**
   * Present only when its place in a custom workflow changes; a place is never taken away.
   */
  workflow?: WorkflowPlace | null;
};

export type WorkflowPlace = {
  phase: string;
  state: string;
  resume?: WorkflowPair | null;
  revision: number;
  manual?: WorkflowManualRecovery | null;
};

export type WorkflowManualRecovery = { trigger: string; holder: string; role: string };

export type WorkflowPair = { phase: string; state: string };

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
  /**
   * How many open tasks it orchestrates, which `tasks` leaves out: its Following list's count.
   */
  following: number;
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
  /**
   * What its model calls took, all told, as its board's heartbeat recorded them; none before
   * any was, or from a board that records none.
   */
  tokens: AgentTokens | null;
  /**
   * What those tokens cost, estimated at API list prices; none beside no tokens, or from a board
   * that prices none.
   */
  cost: AgentCost | null;
};

export type HeldTask = { slug: string; role: string; status: string };

export type AgentRun = { recipe: string; phase: string; since: number | null };

export type AgentSaid = { slug: string; ts: number; text: string };

export type AgentSlots = { open: number; cap: number };

export type AgentTokens = {
  /**
   * Input read neither from the cache nor into it.
   */
  input: number;
  /**
   * Input written into the cache for five minutes.
   */
  cache_write_5m: number;
  /**
   * Input written into the cache for an hour.
   */
  cache_write_1h: number;
  /**
   * Input read from the cache.
   */
  cache_read: number;
  /**
   * Output, thinking included.
   */
  output: number;
  /**
   * The model calls counted.
   */
  calls: number;
};

export type AgentCost = {
  /**
   * The priced kinds all told.
   */
  dollars: number;
  input: number;
  cache_write_5m: number;
  cache_write_1h: number;
  cache_read: number;
  output: number;
  /**
   * The tokens of each model and speed the table has no price for.
   */
  unpriced: Array<AgentUnpriced>;
  /**
   * The day the table's prices were read (`YYYY-MM-DD`): the figures are an estimate at them.
   */
  prices: string;
};

export type AgentUnpriced = { model: string; speed: string; tokens: AgentTokens };

export type TaskTokens = {
  total: AgentTokens;
  cost: AgentCost;
  /**
   * By role, executor first.
   */
  by_role: Array<RoleTokens>;
  /**
   * By review round, the first first: round 1 is the work up to the first review, round 0 the
   * calls of a session that worked no task.
   */
  by_round: Array<RoundTokens>;
  /**
   * The rows the sums are of, which a task's timeline puts each on the stretch its calls were
   * made for; none from a server that sends none.
   */
  rows: Array<TokenRow>;
};

export type RoleTokens = { role: string; tokens: AgentTokens; cost: AgentCost };

export type RoundTokens = { round: number; tokens: AgentTokens; cost: AgentCost };

export type TokenRow = {
  handle: string;
  /**
   * The task its calls were made on: the page's task, or one of its subtasks.
   */
  slug: string;
  /**
   * Its role on the task (`executor`, `reviewer`, `reserved` while it held the task reserved
   * before its claim), else the session's kind.
   */
  role: string;
  model: string;
  speed: string;
  /**
   * 1 and the moves into review its task's thread held when the calls were made, so round 1 is
   * the work up to the first review; 0 for a session that worked no task.
   */
  round: number;
  tokens: AgentTokens;
  cost: AgentCost;
  /**
   * When the first and the last of its calls were recorded, server seconds.
   */
  first: number;
  last: number;
};

export type TaskCosts = {
  /**
   * The day the API list prices every figure is estimated at were read.
   */
  prices: string;
  /**
   * Each task asked for that took any tokens, by its slug; one that took none is left out.
   */
  costs: Array<TaskCost>;
};

export type TaskCost = {
  slug: string;
  /**
   * In US dollars, at API list prices: never money spent.
   */
  dollars: number;
  /**
   * Whether some of its tokens were of a model the price table names no price for, in no dollar.
   */
  unpriced: boolean;
};

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
   * the device tells by this whether the thread changed since (docs/engineering/analysis/client-cache.md,
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
  /**
   * The kinds of the first and last blocks the section draws ([`Section::ends_of`]), so the
   * outline tells a page how a section it has not read yet will meet its neighbours. Left out
   * for a section that draws nothing.
   */
  ends?: [string, string] | null;
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

export type BoardWorkflow = { project: string; workflow: WorkflowRead | null };

export type WorkflowRead = {
  definition: WorkflowDefinition;
  revision: WorkflowRevision;
  activation_generation: number;
};

export type WorkflowDefinition = {
  format: number;
  initial: WorkflowPair;
  task_kinds: Array<string>;
  phases: Array<WorkflowPhase>;
  states: Array<WorkflowState>;
  positions: Array<WorkflowPosition>;
  edges: Array<WorkflowEdge>;
  capabilities: Array<string>;
  adapters: Array<string>;
  guards: Array<WorkflowGuard>;
};

export type WorkflowPhase = {
  id: string;
  label: string;
  order: number;
  phone_order: number;
  /**
   * False for the default's off-pipeline backlog/blocked/cancelled groups.
   */
  column: boolean;
};

export type WorkflowState = { id: string; label: string };

export type WorkflowPosition = {
  at: WorkflowPair;
  /**
   * None is explicit in JSON; TOML omits it. Phase/state names never imply staffing.
   */
  staffing: WorkflowRole | null;
  slot: WorkflowSlot;
  dependencies: WorkflowDependency;
  unsuccessful_dependency: WorkflowUnsuccessfulDependency;
  accepts_work: boolean;
  terminal: boolean;
  legacy: WorkflowLegacyStatus;
  outcome: WorkflowOutcome | null;
  satisfies_dependencies: boolean;
  completion: WorkflowCompletion;
  recovery: WorkflowRecovery | null;
};

export type WorkflowRecovery = {
  release: WorkflowRecoveryAction;
  lost_holder: WorkflowRecoveryAction;
  attempt_failure: WorkflowRecoveryAction;
  cancellation_acknowledged: WorkflowRecoveryAction;
};

export type WorkflowRecoveryAction = {
  /**
   * None means stay; manual must then be true. No implicit move to ready.
   */
  target: WorkflowPair | null;
  manual: boolean;
  slot: WorkflowSlot;
  release_role: boolean;
  release_reservation: boolean;
};

export type WorkflowGuard = { id: string; adapter: string };

export type WorkflowAuthority =
  | { kind: "task_role"; role: WorkflowRole }
  | { kind: "project_capability"; capability: string }
  | { kind: "system_capability"; capability: string };

export type WorkflowEffect =
  | { kind: "acquire_role"; role: WorkflowRole }
  | { kind: "release_role"; role: WorkflowRole }
  | { kind: "slot"; policy: WorkflowSlot }
  | { kind: "review"; approved: boolean }
  | { kind: "block"; save_resume: boolean }
  | { kind: "clear_block" }
  | { kind: "outcome"; outcome: WorkflowOutcome; satisfies_dependencies: boolean }
  | { kind: "parent_integration" }
  | { kind: "prepare"; adapter: string }
  | { kind: "finalize"; adapter: string };

export type WorkflowRetryBound = { maximum: number; exhausted: WorkflowPair };

export type WorkflowEdge = {
  id: string;
  label: string;
  from: WorkflowPair;
  to: WorkflowPair;
  operation: WorkflowOperation;
  /**
   * A legacy alias resolves only when unique at a source pair.
   */
  alias: string | null;
  authority: Array<WorkflowAuthority>;
  inputs: Array<string>;
  dependencies: WorkflowDependency;
  guards: Array<string>;
  effects: Array<WorkflowEffect>;
  automatic: boolean;
  retry: WorkflowRetryBound | null;
  human_wait: boolean;
};

export type WorkflowRevision = { format: number; digest: string; source: WorkflowSource };

export type WorkflowSource = { commit: string; project_blob: string };

export type WorkflowPositionSnapshot = {
  workflow: WorkflowRevision;
  activation_generation: number;
  task_revision: number;
  at: WorkflowPair;
};

export type WorkflowInputValue =
  | { kind: "text"; value: string }
  | { kind: "revision"; value: string }
  | { kind: "pair"; value: WorkflowPair }
  | { kind: "flag"; value: boolean };

export type WorkflowOperationRequest = {
  project: string;
  task: string;
  edge: string;
  expected: WorkflowPositionSnapshot;
  input_revisions: { [key in string]: string };
  idempotency_key: string;
  inputs: { [key in string]: WorkflowInputValue };
};

export type WorkflowNextAction = {
  edge: string;
  operation: WorkflowOperation;
  destination: WorkflowPair;
  label: string;
  responsible: Array<WorkflowAuthority>;
  required_inputs: Array<string>;
  unavailable: Array<WorkflowRefusalCode>;
};

export type WorkflowNextActions = {
  snapshot: WorkflowPositionSnapshot;
  actions: Array<WorkflowNextAction>;
};

export type WorkflowActionsRead = {
  snapshot: WorkflowPositionSnapshot;
  actions: Array<WorkflowNextAction>;
  takes: Array<WorkflowTakenOutput>;
};

export type WorkflowTakenOutput = { task: string; output: string | null };

export type WorkflowOperationResult = {
  transition: string;
  next: WorkflowNextActions;
  effects: Array<WorkflowEffect>;
};

export type WorkflowRefusalCode =
  | "stale_task"
  | "stale_workflow"
  | "stale_activation"
  | "stale_input"
  | "unknown_id"
  | "removed_id"
  | "unauthorized"
  | "dependency"
  | "capacity"
  | "human_wait"
  | "guard_unavailable"
  | "unsupported_activation"
  | "unsupported_writer"
  | "unsupported_adapter"
  | "explicit_action_required";

export type WorkflowRole = "executor" | "reviewer" | "orchestrator";

export type WorkflowSlot = "retain" | "acquire" | "release";

export type WorkflowDependency = "none" | "done" | "landed" | "accepted_output";

export type WorkflowUnsuccessfulDependency = "block" | "cancel" | "manual";

export type WorkflowOutcome = "success" | "cancellation" | "failure";

export type WorkflowCompletion = "none" | "landing" | "covered_close" | "parent_integration";

export type WorkflowOperation =
  | "create"
  | "claim"
  | "release"
  | "advance"
  | "request_review"
  | "approve"
  | "changes"
  | "block"
  | "resume"
  | "retry"
  | "cancel"
  | "complete_subtask"
  | "close_covered"
  | "prepare_land"
  | "confirm_land"
  | "prepare_external"
  | "confirm_external";

export type WorkflowLegacyStatus =
  | "backlog"
  | "concept"
  | "design"
  | "ready"
  | "in_progress"
  | "review"
  | "done"
  | "blocked"
  | "cancelled";

export type WorkflowTaskGuards = {
  authorities: Array<WorkflowGuardAuthority>;
  guards: Array<WorkflowGuardNeed>;
  edges: Array<WorkflowGuardedEdge>;
  entry?: WorkflowGuardEntry | null;
};

export type WorkflowGuardAuthority = {
  adapter: string;
  issuer: string;
  epoch?: string | null;
  generation?: number | null;
  conflicted: boolean;
  admitted: number;
  asked_here: boolean;
};

export type WorkflowGuardNeed = { guard: string; adapter: string; page?: string | null };

export type WorkflowGuardedEdge = { edge: string; guards: Array<string> };

export type WorkflowGuardEntry = {
  edge: string;
  label: string;
  revision: number;
  principal: WorkflowAttestee;
  /**
   * Seconds since the Unix epoch.
   */
  at: number;
  guards: Array<WorkflowGuardMet>;
};

export type WorkflowGuardMet = {
  guard: string;
  adapter: string;
  issuer: string;
  epoch: string;
  generation: number;
  standing: WorkflowGuardStanding;
  record?: WorkflowGuardRecord | null;
};

export type WorkflowGuardRecord = {
  result: WorkflowAttested;
  facts: Array<string>;
  reasons: Array<string>;
  epoch: string;
  generation: number;
  /**
   * Seconds since the Unix epoch.
   */
  issued_at: number;
  expires_at: number;
  task_revision: number;
  at: WorkflowPair;
  input_revisions: { [key in string]: string };
  principal: WorkflowAttestee;
};

export type WorkflowGuardRefusal = {
  guard: string;
  adapter: string;
  why: WorkflowGuardWhy;
  record?: WorkflowGuardRecord | null;
};

export type WorkflowGuardWhy =
  | "uncommitted"
  | "unreported"
  | "conflict"
  | "missing"
  | "unanswered"
  | "unverified"
  | "stale"
  | "not_yours"
  | "refused"
  | "unknown"
  | "unfounded"
  | "expired"
  | "early"
  | "overlong"
  | "decided_again"
  | "not_reported";

export type WorkflowGuardStanding =
  | "met"
  | "uncommitted"
  | "unverified"
  | "unreported"
  | "conflict"
  | "decided_again";

export type WorkflowAttested = "pass" | "refuse" | "unknown";

export type WorkflowAttestee = { kind: string; issuer: string; subject: string };

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
  /**
   * How many execution attempts the task has had ([`TASK_ATTEMPTS_PATH`] reads them), left out
   * at none: a page draws their history only for a task that has some.
   */
  attempts?: number;
  /**
   * The one reading may pass a held message on ([`TASK_PASS_ROUTE`]): the person at the machine,
   * or a person holding speak and direct agents.
   */
  passes: boolean;
  /**
   * Whose the task is ([`Owner`]): none on a board that names nobody.
   */
  owner?: Owner | null;
};

export type TaskEntry = {
  message: Message;
  stamp: string;
  blocks: Array<Block>;
  /**
   * The message held for people this line is of: on the board's line that says it is held,
   * `id` its own, and on the line its words became once passed on, `id` the board's line. Only
   * a person's read of the thread carries it; none on every other line.
   */
  held?: HeldWords | null;
};

export type HeldWords = {
  /**
   * The number of the board's line that says it is held, which a pass names.
   */
  id: number;
  /**
   * Who wrote it, by their short name.
   */
  author: string;
  text: string;
  /**
   * Whom it was written to: a role, a session's handle, or nobody.
   */
  to?: string | null;
  /**
   * Who passed it on: none while it is held.
   */
  passed_by?: string | null;
};

export type Message = {
  id: number;
  slug: string;
  sender: string;
  recipient: string | null;
  kind: string;
  text: string;
  ts: number;
  /**
   * A person a board named wrote it, `sender` their short name: drawn as a person's line, as
   * the owner's is. False for the owner's, whose sender says so, and for a session's; an older
   * server sends none.
   */
  person: boolean;
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

export type TaskAttempts = { attempts: Array<AttemptView> };

export type AttemptView = {
  /**
   * Its UUID, 32 hex digits.
   */
  id: string;
  /**
   * When it was admitted, in ms since the epoch.
   */
  created: number;
  /**
   * The revision a control names, refused as stale once it moved.
   */
  revision: number;
  /**
   * Whether it holds its lane: a later attempt of the lane supersedes it.
   */
  current: boolean;
  lane: string;
  role: string;
  handle: string | null;
  /**
   * The provider configuration it ran under.
   */
  configuration: string;
  /**
   * The attempt it follows, and how: `retry` or `resume`.
   */
  predecessor: string | null;
  relation: string | null;
  /**
   * The attempt that followed it.
   */
  successor: string | null;
  /**
   * `pending`, `starting`, `unknown`, `running`, `paused`, `held` or `terminal`.
   */
  dispatch: string;
  /**
   * How a terminal attempt ended: `succeeded`, `failed`, `cancelled`, `stopped`, `rejected` or
   * `superseded`.
   */
  ending: string | null;
  /**
   * Why it waits on a person.
   */
  held: string | null;
  /**
   * `none`, `requested`, `confirmed` or `unconfirmed`.
   */
  cancellation: string;
  /**
   * `not_started`, `collecting`, `complete`, `partial`, `failed` or `none`.
   */
  collection: string;
  /**
   * `not_due`, `running`, `complete`, `failed` or `unknown`.
   */
  cleanup: string;
  /**
   * The provider's opaque id of the resource bound to it, and its generation: a reference that
   * outlives the handle, never an address or a credential.
   */
  resource: string | null;
  generation: string | null;
  /**
   * The runtime conversation bound to it.
   */
  conversation: string | null;
  /**
   * Who asked for it: `KIND ISSUER/SUBJECT`.
   */
  initiator: string;
  /**
   * What the one who read it may ask of it here.
   */
  controls: AttemptControls;
};

export type AttemptControls = {
  cancel: boolean;
  /**
   * The stop modes it may be asked: `pause`, `terminate`, `hard_stop`.
   */
  stop: Array<string>;
  /**
   * The modes a resume of it may take: `new_resource`, `reuse_with_new_generation`.
   */
  resume: Array<string>;
  /**
   * Whether a view of its resource may be asked.
   */
  view: boolean;
  /**
   * Why a control its state allows is not offered here, as a sentence the page shows: its
   * provider is not one this server holds, or it is a local terminal its launcher steps.
   */
  unserved: string | null;
};

export type AttemptControl = {
  slug: string;
  attempt: string;
  revision: number;
  /**
   * `cancel`, `stop` or `resume`.
   */
  act: string;
  /**
   * The stop's or the resume's mode, one of the attempt's [`AttemptControls`].
   */
  mode: string | null;
  /**
   * A resume's request key: the same key admits the same attempt, however often it is sent.
   */
  key: string | null;
};

export type AttemptViewAsk = { slug: string; attempt: string };

export type AttemptLink = { url: string; valid_for_ms: number };

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
  /**
   * Each place a custom workflow put the task at, oldest first, from its recorded transitions:
   * each lasting until the next began, the last until the timeline's end. Time at a place is
   * the clock's, never the machine's time on runs ([`Self::machine_seconds`]). Empty, and left
   * out, for a task no workflow held, or whose transitions could not be read.
   */
  places?: Array<PlaceSpan>;
  /**
   * Recorded roles and message addresses. Absent on servers without this projection; never
   * interpret absence, a partial source or a failed source as an empty history.
   */
  history?: History | null;
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

export type PlaceSpan = {
  /**
   * Its phase's ID, then its label in the definition the move was made under: the ID where
   * that definition is not recorded or does not name it.
   */
  phase: string;
  phase_label: string;
  /**
   * Its coordination state's ID and label, named as its phase is.
   */
  state: string;
  state_label: string;
  /**
   * The board status the place projects to, which a page colours it by, as it colours a
   * status.
   */
  status: string;
  /**
   * When the task was put there.
   */
  from: number;
  /**
   * The edge that moved it there: none for a move an activation or a filing made.
   */
  edge?: string | null;
  /**
   * Who moved it: an agent session's handle, or the owner.
   */
  by: string;
};

export type History = {
  sources: Array<HistorySource>;
  /**
   * False when the source-count bound or a failed read left ancestry unexplored. Truncated
   * ranges are reported by `sources`, even when every referenced source was visited.
   */
  sources_complete: boolean;
  roles: Array<RoleSpan>;
  /**
   * Non-system thread entries, ordered by exact timestamp then id. No text or reply inference.
   */
  messages: Array<TimelineMessage>;
};

export type HistorySource = {
  slug: string;
  state: HistoryState;
  first_id: number | null;
  last_id: number | null;
  /**
   * Older entries remain reachable through `/api/task-history?slug=SLUG&before=ID`.
   * Unlike `partial`, this specifically means earlier recorded rows were left out.
   */
  before: number | null;
};

export type HistoryState = "complete" | "partial" | "unavailable";

export type RoleSpan = {
  role: string;
  by: string;
  source: string;
  from: number;
  from_id: number;
  to: number | null;
  to_id: number | null;
  uncertain: boolean;
};

export type TimelineMessage = {
  id: number;
  at: number;
  sender: string;
  recipient: string | null;
  kind: string;
  from: MessageEndpoint;
  to: MessageEndpoint;
  /**
   * Unknown when incomplete records cannot establish whether the task was closed then.
   */
  after_close: boolean | null;
};

export type MessageEndpoint = { state: EndpointState; candidates: Array<RoleEndpoint> };

export type EndpointState = "known" | "ambiguous" | "everyone" | "outside" | "unknown";

export type RoleEndpoint = { role: string; by: string; source: string };

export type TaskDiffSource = "branch" | "landing" | "unavailable";

export type TaskDiff = {
  source: TaskDiffSource;
  /**
   * The full base commit id; null when no comparison is available.
   */
  base: string | null;
  /**
   * The full head commit id; null when no comparison is available.
   */
  head: string | null;
  files: Array<TaskDiffFile>;
  /**
   * The answer leaves out some of the comparison because a read bound was reached.
   */
  truncated: boolean;
  /**
   * Why the comparison is unavailable or incomplete, otherwise null. An unavailable answer
   * explains a gone branch without a landing, missing history or a failed or timed-out read.
   */
  reason: string | null;
};

export type TaskDiffFile = { path: string; added: number | null; removed: number | null };

export type TaskDiffPatch = {
  base: string;
  head: string;
  /**
   * The repository-relative path the request named.
   */
  path: string;
  patch: string;
  /**
   * The patch text stops at a read bound.
   */
  truncated: boolean;
};
