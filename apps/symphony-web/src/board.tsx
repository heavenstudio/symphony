import "./styles/boardwait.css";
// The approved B task board: complete wait facets, original-stage columns on a desk and one saved
// stage on a phone. Desk columns scroll sideways at a readable width. Columns and cards mount
// near the screen, reserving each card's two-line title
// and wait line; the first read's ghost keeps the last counts.
// What it reads and when is boardlive.tsx's.

import {
  type CSSProperties,
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { linkedSlug, linkedTask } from "./boardlink";
import { focusedLink, focusedSlug, takeFocus, useKeptPlace } from "./boardplace";
import {
  NAMES,
  OrchestratorBanner,
  PHONE_STAGES,
  STAGE_KEY,
  STAGES,
  savedStage,
  WAIT_KINDS,
  WAIT_LABELS,
  WaitCard,
  waitAge,
  waitingOrder,
  waitingStage,
  waitKind,
} from "./boardwait";
import { Button } from "./button";
import type { Flow } from "./flow";
import { href } from "./project";
import { said } from "./request";
import { useAnchor } from "./route";
import { useScrollRow } from "./scrollrow";
import { SelectField } from "./select";
import { load, save } from "./storage";
import { isTask } from "./task";
import { statusWords } from "./taskcard";
import { tick } from "./timing";
import { failed } from "./trouble";
import { NearViewport, windowWidth } from "./viewport";
import type { MachineWork, Orchestrator, Task, WaitingBoard, WaitingColumn } from "./wire";
import { freshnessWord } from "./work";

/** A card's fixed 110 px box and its 8 px gap, shared with boardwait.css. */
export const ROW = 118;
/** The rows of a column's page, as the server pages it, and the height of Load more under it. */
export const PAGE = 30;
export const MORE = 40;
/** The space under a card. */
export const GAP = 8;
export const SHAPE = "board-waiting-shape";

const width = (w: string) => ({ "--w": w }) as CSSProperties;
const range = (count: number) => Array.from({ length: Math.max(count, 0) }, (_, at) => at);

/** Where a stage's lane stands in the lane scroller: off its left edge, off its right, or whole. */
export type Side = "before" | "after" | "shown";

/** A box's left and right edges, as `getBoundingClientRect` gives them. */
export interface Span {
  left: number;
  right: number;
}

/** Where `lane` stands in the scroller's `frame`; a pixel's rounding is no cut. */
export function laneSide(frame: Span, lane: Span): Side {
  if (lane.left < frame.left - 1) return "before";
  return lane.right > frame.right + 1 ? "after" : "shown";
}

/** A status as the Status filter names it, a sentence's first word: `in_progress` reads "In
 * progress". */
export function statusLabel(status: string): string {
  const words = statusWords(status);
  const [first = ""] = words;
  return first === "" ? "" : `${first.toUpperCase()}${words.slice(first.length)}`;
}

export function firstPageHeight(count: number): number {
  return Math.min(count, PAGE) * ROW + (count > PAGE ? MORE : 0);
}

/** The lifecycle columns and their complete totals, saved for the next visit's ghost. */
export function boardShape(counts: [string, number][]): [string, number][] {
  return STAGES.map((status) => [status, counts.find(([one]) => one === status)?.[1] ?? 0]);
}
function defaultShape(): [string, number][] {
  return STAGES.map((status) => [status, status === "build" || status === "test" ? 0 : 1]);
}

/** The shape `kept` (as SHAPE holds it) gives the board; `defaultShape` for none. */
export function recallShape(kept: string | null): [string, number][] {
  const shape: [string, number][] = [];
  for (const line of (kept ?? "").split("\n")) {
    const space = line.indexOf(" ");
    if (space < 0) continue;
    const count = line.slice(space + 1);
    if (/^\+?\d+$/.test(count)) shape.push([line.slice(0, space), Number(count)]);
  }
  return shape.length === 0 ? defaultShape() : shape;
}

/** `shape` as SHAPE keeps it. */
export function keepShape(shape: [string, number][]): string {
  return shape.map(([status, count]) => `${status} ${count}`).join("\n");
}

/** A task's card while it loads: its box as tall as `card`'s pitch less the space under it, a bar
 * for its slug, one for each line of title it has room for and a pill for each row of chips. */
export function GhostCard() {
  return (
    <div className="ghost-card waiting-card-ghost" style={{ height: `${ROW - GAP}px` }}>
      <span className="bone" />
      <span className="bone" style={width("62%")} />
      <span className="bone" style={width("80%")} />
      <span className="bone" style={width("72%")} />
      <span className="bone" style={width("90%")} />
    </div>
  );
}
function GhostColumn({ status, count }: { status: string; count: number }) {
  return (
    <div className="ghost-column" data-for={status}>
      <h2>
        <span className="bone" style={width("9ch")} />
        <span className="count" style={{ visibility: "hidden" }}>
          0
        </span>
      </h2>
      <div className="column-summary">
        <span className="bone" style={width("12ch")} />
      </div>
      <div>
        {range(Math.min(count, PAGE)).map((slot) => (
          <GhostCard key={slot} />
        ))}
        {count > PAGE && <div style={{ height: `${MORE}px` }} />}
      </div>
    </div>
  );
}

/** The task board before its first read: its filters and, in the `shape` it last had, its columns
 * with a card for each task they held, each as this browser last drew it. */
export function GhostBoard({ shape }: { shape: [string, number][] }) {
  const phone = windowWidth() <= 760;
  const stages = phone ? [savedStage(load(STAGE_KEY))] : STAGES;
  return (
    <div className="waiting-board waiting-ghost">
      <div className="board-toolbar">
        <span className="ghost-field ghost-filters-toggle">
          <span className="bone" style={width("16ch")} />
        </span>
        {!phone && <span className="ghost-field ghost-backlog" />}
      </div>
      <div className="row filters">
        <div>
          Search tasks and messages
          <span className="ghost-field" style={{ width: "10.5rem" }} />
        </div>
        <div>
          Finished tasks
          <span className="ghost-field" style={{ width: "8.125rem" }} />
        </div>
      </div>
      {phone && <div className="under-way" />}
      <nav className="wait-strip" aria-hidden="true">
        <span className="open-count">0 open</span>
        {WAIT_KINDS.map((kind, index) => (
          <span key={kind} className={`wait-chip k-${kind}`}>
            {WAIT_LABELS[index]} <b>0</b>
          </span>
        ))}
      </nav>
      {phone ? (
        <div className="ghost-tabs">
          <span className="bone" style={width("95%")} />
        </div>
      ) : (
        <div className="ghost-jump" />
      )}
      <div className="ghost-kanban waiting-columns">
        <div className="waiting-pipeline">
          {stages.map((status) => (
            <GhostColumn
              key={status}
              status={status}
              count={
                shape.find(([one]) => one === status)?.[1] ??
                (status === "build" || status === "test" ? 0 : 1)
              }
            />
          ))}
        </div>
      </div>
    </div>
  );
}

/** The task board whose first read failed, with nothing drawn from this device: why, in the
 * read's own words, and a retry, in place of the ghost, which would wait for ever. */
export function UnreadBoard({ why, retry }: { why: string; retry: () => void }) {
  return (
    <section className="trouble card unread-board" role="status">
      <h2>The task board could not be read</h2>
      <p>{failed(why, true)}</p>
      <p className="trouble-actions">
        <Button type="button" onClick={retry}>
          Retry
        </Button>
      </p>
    </section>
  );
}

/** What the Build column says of the machines: how sure their observation is, and where the
 * whole inventory is. */
export function BoardContext({ work, connected }: { work: MachineWork; connected: boolean }) {
  return (
    <details className="machine-work" aria-label="Machine observations">
      <summary>Machine context</summary>
      <p>
        {!connected
          ? "Disconnected · capacity unknown"
          : work.machines.length === 0
            ? "No identified machine observations · capacity unknown"
            : `Observation status · ${freshnessWord(work.freshness)}`}
      </p>
      <a href={href("/agents#machine-context")}>Machine details and advisory targets</a>
    </details>
  );
}

/** What the phone's row of work under way is given. */
export interface UnderWayProps {
  /** The tasks in progress that no machine builds or tests, the blocked ones left out. */
  working: number;
  /** The tasks a machine builds and those one tests; none while no machine reports its work. */
  machines: [building: number, testing: number] | null;
  /** Show a stage's lane. */
  onStage: (stage: string) => void;
}

/**
 * How much work is under way, on a phone, where one stage is mounted and the Build and Test tabs
 * are past the row's edge: every task in progress, a machine's builds and tests among them and
 * named as such, each count a button to its lane. Four equal cells in one fixed row, so a count
 * that changes, or a machine's first report, moves nothing.
 */
export function UnderWay({ working, machines, onStage }: UnderWayProps) {
  const [building, testing] = machines ?? [0, 0];
  const lane = (stage: string, count: number, label: string) => (
    <button type="button" data-stage={stage} onClick={() => onStage(stage)}>
      <b>{count}</b> {label}
    </button>
  );
  return (
    <nav className="under-way" aria-label="Work under way">
      <span className="under-way-all">
        <b>{working + building + testing}</b> under way
      </span>
      {lane("in_progress", working, "working")}
      {machines !== null && lane("build", building, "building")}
      {machines !== null && lane("test", testing, "testing")}
    </nav>
  );
}

/** What a column asks of the page: a page of `status` (the next one when `more`), under the
 * search `query` and the window of `days`. */
export type OnLoad = (status: string, more: boolean, query: string, days: number) => void;

export interface RowProps {
  task: Task;
  now?: number;
}

/**
 * A row of a column: its card once near the screen, until then as tall as that card will take
 * (`ROW`). Drawn again only when its task or clock changed. A row
 * first drawn while its task's card holds the focus elsewhere is that card's new place, in the
 * column a live update moved it to: it draws the card at once and gives the focus back to it.
 */
export const BoardRow = memo(function BoardRow({ task, now = Date.now() / 1000 }: RowProps) {
  const [follows] = useState(() => focusedLink(task.slug));
  useLayoutEffect(() => {
    if (follows !== null) takeFocus(task.slug, follows);
  }, [follows, task.slug]);
  return (
    <NearViewport
      id={`row-${task.slug}`}
      height={ROW}
      ghost={<GhostCard />}
      eager={follows !== null}
    >
      <WaitCard task={task} now={now} />
    </NearViewport>
  );
}, sameRow);

/** Whether two rows draw alike. */
export function sameRow(a: RowProps, b: RowProps): boolean {
  return a.now === b.now && a.task === b.task;
}

export interface ColumnProps {
  status: string;
  work: MachineWork;
  connected: boolean;
  tasks: Task[];
  count: number;
  query: string;
  days: number;
  onLoad: OnLoad;
  refreshed: number;
  /** Which of the pipeline's columns it is (`stopPlace`); none off the pipeline. */
  meta?: WaitingColumn | undefined;
  label?: string | undefined;
  now?: number;
  cancelled?: number;
  onCancelled?: (() => void) | undefined;
  /** Load the column an explicit row link names, including while it is off screen. */
  eager?: boolean;
}

/** A column of the board: its heading and count, its rows near the screen, and Load more. Drawn again
 * only when what it is given changed: the board keeps an unchanged column's tasks (`byColumn`). */
export const TaskColumn = memo(function TaskColumn({
  status,
  work,
  connected,
  tasks,
  count,
  query,
  days,
  onLoad,
  refreshed,
  meta,
  label,
  now = Date.now() / 1000,
  cancelled = 0,
  onCancelled,
  eager = false,
}: ColumnProps) {
  const [visible, setVisible] = useState(false);
  // Complete totals make empty-column reads unnecessary. Retained rows still refresh so a
  // reconnect can remove stale cards, and a newly populated column must request its page.
  const needsPage = count > 0 || tasks.length > 0;
  const load = useRef(onLoad);
  load.current = onLoad;
  useEffect(() => {
    void refreshed;
    if (visible && needsPage) load.current(status, false, query, days);
  }, [visible, needsPage, query, days, refreshed, status]);
  const rows = tasks;
  // Keep unloaded columns in place while their first page is fetched: a card's box for each row of
  // the page, each as tall as this browser last drew the row in its place, and the Load more
  // button a column of more than a page will show.
  const reserved = firstPageHeight(count);
  const reading = count > 0 && tasks.length === 0;
  const ghosts = range(Math.min(count, PAGE)).map((slot) => <GhostCard key={slot} />);
  // A column not near the screen yet draws its rows at once when one of them is the new place of
  // the card holding the focus (`BoardRow`), as a phone's column beside the one shown is.
  const focused = visible ? null : focusedSlug();
  const follows = focused !== null && tasks.some((task) => task.slug === focused);
  return (
    <section className="column" data-status={status} aria-busy={reading}>
      <h2 tabIndex={-1}>
        {label ?? statusWords(status)} <span className="count">{meta?.ordinary ?? count}</span>
      </h2>
      <div className="column-summary">
        {status === "done" ? (
          <button type="button" data-open-stage="cancelled" onClick={onCancelled}>
            {cancelled} cancelled · {days === 1 ? "today" : "this week"}
          </button>
        ) : (
          <>
            {(meta?.blocked ?? 0) > 0 && (
              <>
                <b>+{meta?.blocked} blocked</b> ·{" "}
              </>
            )}
            {meta?.oldest != null && <>oldest {waitAge(meta.oldest, now)}</>}
          </>
        )}
      </div>
      {status === "build" && <BoardContext work={work} connected={connected} />}
      {count === 0 && (status === "build" || status === "test") && (
        <p className="muted work-empty">
          {!connected
            ? `Disconnected · ${status} activity unknown`
            : work.machines.length === 0
              ? `No ${status} observations reported.`
              : `No tasks with fresh ${status} activity.`}
        </p>
      )}
      <NearViewport
        id={`column-${status}`}
        height={reserved}
        onEnter={() => setVisible(true)}
        ghost={ghosts}
        eager={follows || eager}
      >
        {reading && (
          <div style={{ minHeight: `${reserved}px` }} data-pending="true">
            {ghosts}
          </div>
        )}
        {/* Each row holds its card's fixed box until it is near the screen. */}
        {rows.map((task) => (
          <BoardRow key={task.slug} task={task} now={now} />
        ))}
        {/* Only once the first page is in: while it is read, there is nothing more to load. */}
        {tasks.length > 0 && count > tasks.length && (
          <Button onClick={() => onLoad(status, true, query, days)}>Load more tasks</Button>
        )}
      </NearViewport>
    </section>
  );
});

/** No task: a column with none keeps this one list. */
const NONE: Task[] = [];

/**
 * `tasks` by the column each goes in (`place`), in their order. A column whose tasks are the same
 * objects as in `before` keeps `before`'s list, so it is not drawn again.
 */
export function byColumn(
  tasks: Task[],
  place: (task: Task) => string,
  before: Map<string, Task[]>,
): Map<string, Task[]> {
  const columns = new Map<string, Task[]>();
  for (const task of tasks) {
    const status = place(task);
    const list = columns.get(status);
    if (list === undefined) columns.set(status, [task]);
    else list.push(task);
  }
  for (const [status, list] of columns) {
    const kept = before.get(status);
    if (kept?.length === list.length && kept.every((task, at) => task === list[at])) {
      columns.set(status, kept);
    }
  }
  return columns;
}

/** The window of finished tasks a choice of the Finished tasks field names, in days. */
export function windowOf(value: string): number {
  return value === "7" ? 7 : 1;
}

/** What the board's filters hold: the search typed and the window of finished tasks, in days. */
export interface Filters {
  search: string;
  days: number;
}

/** The filters with `query` typed in the search: the board is read under it, in the window
 * chosen. Every key reads, `boardlive`'s pause waiting for the next. */
export function searched(filters: Filters, query: string, onFilter: Filter): Filters {
  onFilter(query, filters.days);
  return { ...filters, search: query };
}

/** The filters with the Finished tasks field's `value` chosen: the board is read in its window,
 * under the search typed, the window already chosen too, which reads the board again. */
export function windowChosen(filters: Filters, value: string, onFilter: Filter): Filters {
  const days = windowOf(value);
  onFilter(filters.search, days);
  return { ...filters, days };
}

/** What the filters ask of the page: the board read under the search `query` in the window of
 * `days`. */
export type Filter = (query: string, days: number, wait?: string) => void;

export interface TaskBoardProps {
  scope: string;
  tasks: Task[];
  work: MachineWork;
  connected: boolean;
  signoff: string[];
  counts: [string, number][];
  statuses: string[];
  refreshed: number;
  onFilter: Filter;
  onLoad: OnLoad;
  waiting?: WaitingBoard | null;
  orchestrators?: Orchestrator[];
  unstarted?: number;
}

/** The board: its filters, its columns and its task groups. */
export function TaskBoard({
  scope,
  tasks,
  work,
  connected,
  counts,
  refreshed,
  onFilter,
  onLoad,
  waiting = null,
  orchestrators = [],
  unstarted = 0,
}: TaskBoardProps) {
  const [filters, setFilters] = useState<Filters>({ search: "", days: 1 });
  const { search, days } = filters;
  const [wait, setWait] = useState("");
  // A custom workflow's lanes, tabs and strip, from a chunk of its own read once the listing says
  // one is active, and again when its generation moves (`flow.tsx`).
  const [flow, setFlow] = useState<Flow | null>(null);
  const generation = waiting?.workflow;
  useEffect(() => {
    const reading = new AbortController();
    if (generation)
      void import("./flow")
        .then((chunk) => chunk.readFlow(scope, reading.signal, NAMES, [STAGES, PHONE_STAGES]))
        .then((read) => reading.signal.aborted || setFlow(read));
    else setFlow(null);
    return () => reading.abort();
  }, [scope, generation]);
  const [stages, tabs] = flow?.lanes(waiting) ?? [STAGES, PHONE_STAGES];
  const name = (stage: string) => NAMES.get(stage) ?? statusLabel(stage);
  // The phone's stage this device chose, kept as chosen, and the one shown: the first of the
  // board's tabs when it names none of them.
  const [selected, setSelected] = useState(() => load(STAGE_KEY) ?? "");
  const tab = savedStage(selected, tabs);
  const [phone, setPhone] = useState(() => windowWidth() <= 760);
  const [opened, setOpened] = useState("");
  const [filtering, setFiltering] = useState(false);
  const [now, setNow] = useState(() => Date.now() / 1000);
  const address = useAnchor();
  const slug = linkedSlug(address);
  const [resolved, setResolved] = useState<Task | null>(null);
  const [linkError, setLinkError] = useState("");
  const [retryLink, setRetryLink] = useState(0);
  const [followed, setFollowed] = useState("");
  const known = tasks.find((task) => task.slug === slug);
  const linked = known ?? (resolved?.slug === slug ? resolved : null);
  useEffect(() => {
    void retryLink;
    if (slug === "" || known !== undefined) return;
    const controller = new AbortController();
    setLinkError("");
    void linkedTask(scope, slug, controller.signal).then(
      (task) => {
        if (!controller.signal.aborted) {
          setResolved(task);
          if (task === null) setLinkError("The linked task is no longer on this board.");
        }
      },
      (error: unknown) => {
        if (!controller.signal.aborted) setLinkError(said(error));
      },
    );
    return () => controller.abort();
  }, [scope, slug, known, retryLink]);
  // Resolve the address before the selected column mounts, so its row can land directly under
  // the header. A saved phone tab never takes precedence over an explicit row address.
  const destination = linked === null ? "" : waitingStage(work, linked, connected);
  if (followed !== address && (slug === "" || linked !== null)) {
    setFollowed(address);
    if (linked !== null) {
      if (tabs.includes(destination)) setSelected(destination);
      setOpened(
        stages.includes(destination) || (phone && tabs.includes(destination)) ? "" : destination,
      );
    }
  }
  const linkPage = useRef("");
  useEffect(() => {
    if (known !== undefined || destination === "" || wait !== "" || search !== "") return;
    if (phone ? destination !== (opened || tab) : opened !== "" && opened !== destination) return;
    const rows = tasks.filter((task) => waitingStage(work, task, connected) === destination).length;
    const total = counts.find(([stage]) => stage === destination)?.[1] ?? 0;
    const key = `${address}:${destination}:${rows}`;
    if (rows === 0 || rows >= total || linkPage.current === key) return;
    linkPage.current = key;
    onLoad(destination, true, search, days);
  }, [
    address,
    destination,
    known,
    tasks,
    counts,
    work,
    connected,
    phone,
    opened,
    tab,
    wait,
    search,
    days,
    onLoad,
  ]);
  const surface = useRef<HTMLElement>(null);
  useKeptPlace(surface, true, true);
  const showingColumns = !phone && wait === "" && opened === "";
  const columnLeft = useRef(0);
  const wasShowingColumns = useRef(showingColumns);
  const rememberColumns = () => {
    if (showingColumns && surface.current !== null) columnLeft.current = surface.current.scrollLeft;
  };
  // Which lanes the scroller cuts, a side for each in their order, read again at each scroll and
  // each change of its width.
  const [sides, setSides] = useState("");
  const measure = useCallback(() => {
    const box = surface.current;
    if (box === null) return;
    const frame = box.getBoundingClientRect();
    setSides(
      [...box.querySelectorAll(".column")]
        .map((lane) => laneSide(frame, lane.getBoundingClientRect()))
        .join(),
    );
  }, []);
  useLayoutEffect(() => {
    if (showingColumns && !wasShowingColumns.current && surface.current !== null) {
      surface.current.scrollLeft = columnLeft.current;
    }
    wasShowingColumns.current = showingColumns;
    const box = surface.current;
    if (!showingColumns || box === null) return;
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(box);
    return () => observer.disconnect();
  }, [showingColumns, measure]);
  // Show a stage's lane whole (its heading's scroll margin reaches the lane's edges) and put the
  // focus on that heading, so the next Tab is in that lane.
  const jump = (stage: string) => {
    const head = surface.current?.querySelector<HTMLElement>(`[data-status="${stage}"] > h2`);
    head?.scrollIntoView({ block: "nearest", inline: "nearest" });
    head?.focus({ preventScroll: true });
  };
  // Follow only the card holding the keyboard focus. Adjust this component's state before
  // React commits the move, while BoardRow can still read the old title link to restore it.
  const focused = wait === "" ? focusedSlug() : null;
  const focusedTask = focused === null ? undefined : tasks.find((task) => task.slug === focused);
  if (focusedTask !== undefined) {
    const destination = waitingStage(work, focusedTask, connected);
    if (phone && destination !== (opened || tab)) {
      if (tabs.includes(destination)) {
        setSelected(destination);
        setOpened("");
      } else setOpened(destination);
    } else if (!phone) {
      const next = stages.includes(destination) ? "" : destination;
      if (opened !== next) setOpened(next);
    }
  }
  useEffect(() => {
    save(STAGE_KEY, selected);
  }, [selected]);
  const previousOpen = useRef(opened);
  useLayoutEffect(() => {
    const previous = previousOpen.current;
    previousOpen.current = opened;
    if (previous === opened || document.activeElement !== document.body) return;
    const target = phone
      ? '.stage-tabs button[aria-current="page"]'
      : opened !== ""
        ? ".back-to-stages"
        : previous !== ""
          ? `[data-open-stage="${previous}"]`
          : null;
    if (target !== null)
      document.querySelector<HTMLButtonElement>(target)?.focus({ preventScroll: true });
  }, [opened, phone]);
  useEffect(() => {
    const resize = () => setPhone(windowWidth() <= 760);
    window.addEventListener("resize", resize);
    const controller = new AbortController();
    void (async () => {
      while (!controller.signal.aborted) {
        await tick(controller.signal);
        if (!controller.signal.aborted) setNow(Date.now() / 1000);
      }
    })();
    return () => {
      window.removeEventListener("resize", resize);
      controller.abort();
    };
  }, []);
  const kept = useRef(new Map<string, Task[]>());
  const columns = useMemo(
    () =>
      byColumn(
        tasks
          .filter(
            (task) =>
              (isTask(task) || search !== "") &&
              (wait === "" ||
                ((waitKind(task) === wait || `state:${task.workflow?.state}` === wait) &&
                  !["done", "cancelled"].includes(task.status))),
          )
          .sort(waitingOrder),
        (task) => (wait === "" ? waitingStage(work, task, connected) : "waiting"),
        kept.current,
      ),
    [tasks, search, wait, work, connected],
  );
  kept.current = columns;
  const count = (status: string) => counts.find(([one]) => one === status)?.[1] ?? 0;
  const meta = (status: string) => waiting?.columns.find((one) => one.status === status);
  const toggleCancelled = useCallback(() => {
    if (showingColumns && surface.current !== null) columnLeft.current = surface.current.scrollLeft;
    setOpened((before) => (before === "cancelled" ? "" : "cancelled"));
  }, [showingColumns]);
  const column = (status: string) => (
    <TaskColumn
      key={status}
      status={status}
      tasks={columns.get(status) ?? NONE}
      count={status === "waiting" ? counts.reduce((sum, [, n]) => sum + n, 0) : count(status)}
      work={work}
      connected={connected}
      query={search}
      days={days}
      onLoad={onLoad}
      refreshed={refreshed}
      meta={meta(status)}
      label={
        status === "waiting"
          ? (WAIT_LABELS[WAIT_KINDS.indexOf(wait)] ?? NAMES.get(wait))
          : NAMES.get(status)
      }
      now={now}
      cancelled={count("cancelled")}
      onCancelled={toggleCancelled}
      eager={slug !== "" && destination === status}
    />
  );
  const filter = (query: string, days: number) => onFilter(query, days, wait);
  const chooseWait = (kind: string) => {
    rememberColumns();
    const next = wait === kind ? "" : kind;
    setWait(next);
    onFilter(search, days, next);
  };
  const machine = work.machines.length > 0;
  // Show a stage on a phone: one of the tabs chosen and kept, or a machine's stage opened.
  const show = (stage: string) => {
    if (tabs.includes(stage)) {
      setSelected(stage);
      save(STAGE_KEY, stage);
      setOpened("");
    } else setOpened(stage);
  };
  // A phone's two rows of choices scroll across: each keeps its chosen one whole in view.
  const waits = useScrollRow(wait);
  const tabRow = useScrollRow(`${opened}:${tab}`);
  return (
    <div className="waiting-board">
      {slug !== "" && known === undefined && linkError !== "" && (
        <p role="status" className="trouble">
          {linkError} <Button onClick={() => setRetryLink(retryLink + 1)}>Retry</Button>
        </p>
      )}
      <div className="board-toolbar">
        <Button
          type="button"
          className="board-filters-toggle"
          aria-expanded={filtering}
          aria-controls="board-filters"
          onClick={() => setFiltering(!filtering)}
        >
          Search and filters
        </Button>
        {!phone && wait === "" && (
          <Button
            className="board-backlog"
            data-open-stage="backlog"
            aria-expanded={opened === "backlog"}
            onClick={() => {
              rememberColumns();
              setOpened(opened === "backlog" ? "" : "backlog");
            }}
          >
            Backlog {count("backlog")}
          </Button>
        )}
      </div>
      <div id="board-filters" className="row filters" data-open={filtering}>
        <label>
          Search tasks and messages{" "}
          <input
            className="dx-input"
            type="search"
            placeholder="Slug or words"
            defaultValue=""
            onInput={(event) => setFilters(searched(filters, event.currentTarget.value, filter))}
          />
        </label>
        {/* biome-ignore lint/a11y/noLabelWithoutControl: SelectField provides the labelled button */}
        <label>
          Finished tasks{" "}
          <SelectField
            label="Finished tasks"
            value={String(days)}
            options={[
              ["1", "Last 24 hours"],
              ["7", "Last week"],
            ]}
            onChange={(value) => setFilters(windowChosen(filters, value, filter))}
          />
        </label>
      </div>
      <OrchestratorBanner
        orchestrators={orchestrators}
        unstarted={unstarted}
        ready={
          wait === "" && search === "" ? (meta("ready")?.ordinary ?? count("ready")) : undefined
        }
      />
      {/* Under a wait filter, whose counts are the filter's, the row keeps its room, so the strip
          under it stays where the chip was pressed. A custom workflow's lanes are phases: its
          board draws none, from the listing that names it, before its lanes are read. */}
      {phone &&
        !generation &&
        (wait === "" ? (
          <UnderWay
            working={meta("in_progress")?.ordinary ?? count("in_progress")}
            machines={machine ? [count("build"), count("test")] : null}
            onStage={show}
          />
        ) : (
          <div className="under-way" />
        ))}
      <nav
        ref={waits}
        className="wait-strip scroll-row"
        aria-label="Filter tasks by what they wait on"
      >
        <button
          type="button"
          className="open-count"
          aria-pressed={wait === ""}
          onClick={() => chooseWait("")}
        >
          {waiting?.kinds.reduce((sum, [, n]) => sum + n, 0) ??
            counts
              .filter(([s]) => !["done", "cancelled"].includes(s))
              .reduce((sum, [, n]) => sum + n, 0)}{" "}
          open
        </button>
        {WAIT_KINDS.map((kind, index) => (
          <button
            key={kind}
            type="button"
            className={`wait-chip k-${kind}`}
            aria-pressed={wait === kind}
            onClick={() => chooseWait(kind)}
          >
            {WAIT_LABELS[index]} <b>{waiting?.kinds.find(([one]) => one === kind)?.[1] ?? 0}</b>
          </button>
        ))}
      </nav>
      {flow?.strip({ waiting, wait, choose: chooseWait, open: setOpened, opened, phone })}
      {wait === "" && (
        <nav ref={tabRow} className="stage-tabs scroll-row" aria-label="Task stages">
          {tabs.map((stage) => (
            <button
              key={stage}
              type="button"
              aria-current={
                (opened === "" && tab === stage) || (opened === "cancelled" && stage === "done")
                  ? "page"
                  : undefined
              }
              onClick={() => show(stage)}
            >
              {name(stage)} <i>{meta(stage)?.ordinary ?? count(stage)}</i>
              {(meta(stage)?.blocked ?? 0) > 0 && <b>+{meta(stage)?.blocked}</b>}
            </button>
          ))}
          {machine &&
            ["build", "test"].map((stage) => (
              <button
                key={stage}
                type="button"
                aria-current={opened === stage ? "page" : undefined}
                onClick={() => show(stage)}
              >
                {statusLabel(stage)} {count(stage)}
              </button>
            ))}
        </nav>
      )}
      {!phone && wait === "" && opened !== "" && (
        <Button className="back-to-stages" onClick={() => setOpened("")}>
          Back to stages
        </Button>
      )}
      {showingColumns && (
        <nav className="stage-jump" aria-label="Show a stage">
          {stages.map((stage, at) => {
            const side = sides.split(",")[at] || "shown";
            return (
              <button key={stage} type="button" data-side={side} onClick={() => jump(stage)}>
                {side === "before" && "‹ "}
                {name(stage)}
                {side === "after" && " ›"}
              </button>
            );
          })}
        </nav>
      )}
      <section
        ref={surface}
        aria-label="Task columns"
        tabIndex={phone ? undefined : 0}
        onScroll={() => {
          rememberColumns();
          if (showingColumns) measure();
        }}
        className={`kanban waiting-columns${wait !== "" || (!phone && opened !== "") ? " wait-list" : ""}`}
      >
        {wait !== "" ? (
          column("waiting")
        ) : phone ? (
          column(opened || tab)
        ) : opened !== "" ? (
          column(opened)
        ) : (
          <div className="waiting-pipeline">
            {stages.map((stage) => (
              <div className="stage-slot" key={stage}>
                {column(stage)}
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
