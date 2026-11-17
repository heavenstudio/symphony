import "./styles/boardwait.css";
// The approved B task board: complete wait facets, original-stage columns on a desk and one saved
// stage on a phone. Columns and cards mount near the screen, reserving each card's two-line title
// and wait line; the first read's ghost keeps the last counts.
// What it reads and when is boardlive.tsx's.

import {
  type CSSProperties,
  memo,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { focusedLink, focusedSlug, takeFocus, useKeptPlace } from "./boardplace";
import {
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
import { href } from "./project";
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
  return STAGES.map((status) => [status, 1]);
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
      <div>
        <span className="bone" style={width("9ch")} />
      </div>
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
      <span className="ghost-field ghost-filters-toggle">
        <span className="bone" style={width("16ch")} />
      </span>
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
      <nav className="wait-strip" aria-hidden="true">
        <span className="open-count">0 open</span>
        {WAIT_KINDS.map((kind, index) => (
          <span key={kind} className={`wait-chip k-${kind}`}>
            {WAIT_LABELS[index]} <b>0</b>
          </span>
        ))}
      </nav>
      {phone && (
        <div className="ghost-tabs">
          <span className="bone" style={width("95%")} />
        </div>
      )}
      <div className="ghost-kanban waiting-columns">
        {!phone && <div className="backlog-rail">Backlog</div>}
        {stages.flatMap((status) => [
          <GhostColumn
            key={status}
            status={status}
            count={shape.find(([one]) => one === status)?.[1] ?? 1}
          />,
          ...(!phone && status === "in_progress"
            ? [<GhostColumn key="build-test" status="build-test" count={0} />]
            : []),
        ])}
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

const UnreportedRail = memo(function UnreportedRail({ connected }: { connected: boolean }) {
  return (
    <section className="column rail" data-status="build-test" aria-label="Build and test">
      <h2>build · test</h2>
      <p className="muted work-empty">
        {connected
          ? "No machine reports this project's builds or tests."
          : "Disconnected · build and test activity unknown"}
      </p>
      <a href={href("/agents#machine-context")}>Machine details</a>
    </section>
  );
});

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
      <h2>
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
        <p className="muted work-empty">{`No tasks with fresh ${status} activity.`}</p>
      )}
      <NearViewport
        id={`column-${status}`}
        height={reserved}
        onEnter={() => setVisible(true)}
        ghost={ghosts}
        eager={follows}
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
  const [selected, setSelected] = useState(() => savedStage(load(STAGE_KEY)));
  const [phone, setPhone] = useState(() => windowWidth() <= 760);
  const [opened, setOpened] = useState("");
  const [filtering, setFiltering] = useState(false);
  const [now, setNow] = useState(() => Date.now() / 1000);
  const surface = useRef<HTMLDivElement>(null);
  useKeptPlace(surface, true, true);
  // Follow only the card holding the keyboard focus. Adjust this component's state before
  // React commits the move, while BoardRow can still read the old title link to restore it.
  const focused = wait === "" ? focusedSlug() : null;
  const focusedTask = focused === null ? undefined : tasks.find((task) => task.slug === focused);
  if (focusedTask !== undefined) {
    const destination = waitingStage(work, focusedTask, connected);
    if (phone && destination !== (opened || selected)) {
      if (PHONE_STAGES.includes(destination)) {
        setSelected(destination);
        setOpened("");
      } else setOpened(destination);
    } else if (!phone) {
      const next = STAGES.includes(destination) ? "" : destination;
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
                (waitKind(task) === wait && !["done", "cancelled"].includes(task.status))),
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
      label={status === "waiting" ? WAIT_LABELS[WAIT_KINDS.indexOf(wait)] : undefined}
      now={now}
      cancelled={count("cancelled")}
      onCancelled={() => setOpened(opened === "cancelled" ? "" : "cancelled")}
    />
  );
  const filter = (query: string, days: number) => onFilter(query, days, wait);
  const chooseWait = (kind: string) => {
    const next = wait === kind ? "" : kind;
    setWait(next);
    onFilter(search, days, next);
  };
  const machine = work.machines.length > 0;
  return (
    <div className="waiting-board">
      <Button
        type="button"
        className="board-filters-toggle"
        aria-expanded={filtering}
        aria-controls="board-filters"
        onClick={() => setFiltering(!filtering)}
      >
        Search and filters
      </Button>
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
      <nav className="wait-strip" aria-label="Filter tasks by what they wait on">
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
      {wait === "" && (
        <nav className="stage-tabs" aria-label="Task stages">
          {PHONE_STAGES.map((stage) => (
            <button
              key={stage}
              type="button"
              aria-current={
                (opened === "" && selected === stage) ||
                (opened === "cancelled" && stage === "done")
                  ? "page"
                  : undefined
              }
              onClick={() => {
                setSelected(stage);
                save(STAGE_KEY, stage);
                setOpened("");
              }}
            >
              {statusLabel(stage)} <i>{meta(stage)?.ordinary ?? count(stage)}</i>
              {(meta(stage)?.blocked ?? 0) > 0 && <b>+{meta(stage)?.blocked}</b>}
            </button>
          ))}
          {machine &&
            ["build", "test"].map((stage) => (
              <button
                key={stage}
                type="button"
                aria-current={opened === stage ? "page" : undefined}
                onClick={() => setOpened(stage)}
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
      <div
        ref={surface}
        className={`kanban waiting-columns${wait !== "" || (!phone && opened !== "") ? " wait-list" : ""}`}
      >
        {wait !== "" ? (
          column("waiting")
        ) : phone ? (
          column(opened || selected)
        ) : opened !== "" ? (
          column(opened)
        ) : (
          <>
            <button
              type="button"
              className="backlog-rail"
              data-open-stage="backlog"
              aria-expanded={false}
              onClick={() => setOpened("backlog")}
            >
              Backlog {count("backlog")}
            </button>
            {STAGES.map((stage) => (
              <div className="stage-slot" key={stage}>
                {column(stage)}
              </div>
            )).flatMap((element, index) =>
              index === 3
                ? [
                    element,
                    <div className="machine-rail" key="machine">
                      {machine ? (
                        <>
                          <button
                            type="button"
                            data-open-stage="build"
                            onClick={() => setOpened("build")}
                          >
                            Build {count("build")}
                          </button>
                          <button
                            type="button"
                            data-open-stage="test"
                            onClick={() => setOpened("test")}
                          >
                            Test {count("test")}
                          </button>
                        </>
                      ) : (
                        <UnreportedRail connected={connected} />
                      )}
                    </div>,
                  ]
                : [element],
            )}
          </>
        )}
      </div>
    </div>
  );
}
