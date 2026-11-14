// The task board's view (`/tasks`): its filters, its columns (the owner's sign-off, the pipeline in
// its own scroller, the columns off it), each column's rows mounted near the screen, its task
// groups, and the ghost it draws before its first read, in the shape and with the cards it had.
// What it reads and when is boardlive.tsx's.

import { type CSSProperties, Fragment, useEffect, useRef, useState } from "react";
import { Button } from "./button";
import { type Layout, layoutAt, rowHeight } from "./cardheight";
import { sleep } from "./feed";
import { useMount } from "./mount";
import { href, scoped } from "./project";
import { getJson, said } from "./request";
import { SelectField } from "./select";
import { load, save } from "./storage";
import { byPriority, byRecent, isTask, readListing } from "./task";
import { statusWords, TaskCard } from "./taskcard";
import { failed } from "./trouble";
import { KEEP_MS, NearViewport, Skeleton, waitsNear, windowWidth } from "./viewport";
import type { MachineWork, Task, TaskListing } from "./wire";
import { freshnessWord, PIPELINE, phaseLabels, taskStage } from "./work";

/**
 * What a column waiting for its page reserves each of the page's rows, whose tasks it does not
 * know yet: a typical card, its slug, a title of two lines, a role chip and a link chip
 * (`rowHeight`), near the median of a live board's cards (142 px).
 */
export const ROW = 139;
/** The rows of a column's page, as the server pages it, and the height of Load more under it. */
export const PAGE = 30;
export const MORE = 40;
/** The space under a card. */
export const GAP = 8;
/** What a card's box holds besides its title and chips: its border and padding, its slug's line,
 * the space over its title and its two chip rows' margins, in px. */
const FRAME = 52;
/** A row of chips. */
const CHIP_ROW = 21;
/** A line of a card's title. */
const TITLE_LINE = 20;

/** The columns off the pipeline. */
export const OFFLINE = ["backlog", "blocked", "cancelled"];
/** Where this browser keeps the cards of each column of the board as it opens, as drawn last,
 * newest first, as lines of `WIDTH STATUS PITCH...`, a pitch followed by a `c` for each row of
 * chips its card carries. */
const CARDS = "board-cards";
/** How many columns' cards a browser keeps, at every width. */
export const CARDS_MAX = 32;
/** Where this browser keeps the shape of the project's board as last read (`boardShape`), as
 * lines of `STATUS COUNT`. */
export const SHAPE = "board-shape";

/** A card as its column keeps it: its pitch (its height with the space under it) and how many
 * rows of chips it carries (its build and test phases, its linked documents, its roles). */
export type Kept = [number, number];

const width = (w: string) => ({ "--w": w }) as CSSProperties;
const range = (count: number) => Array.from({ length: Math.max(count, 0) }, (_, at) => at);

/** A status as the Status filter names it, a sentence's first word: `in_progress` reads "In
 * progress". */
export function statusLabel(status: string): string {
  const words = statusWords(status);
  const [first = ""] = words;
  return first === "" ? "" : `${first.toUpperCase()}${words.slice(first.length)}`;
}

/** `statuses` with Build and Test where the board draws their columns: after In progress, Build
 * before Test. One already named is moved there; with no In progress they go last. */
export function withMachineStages(statuses: string[]): string[] {
  const rest = statuses.filter((status) => status !== "build" && status !== "test");
  const progress = rest.indexOf("in_progress");
  const at = progress < 0 ? rest.length : progress + 1;
  return [...rest.slice(0, at), "build", "test", ...rest.slice(at)];
}

/** Whether the tasks of the column `status` wait for an agent to take them up, so a card guessed
 * for them draws no role chip. */
function unstaffed(status: string): boolean {
  return ["concept", "design", "ready", "backlog"].includes(status);
}

/** The card in the place `slot` of the column `status`: as `kept` has it, else as tall as the
 * typical card ROW stands for, with its row of linked documents and, unless no agent has taken
 * the column's tasks up, its row of roles. */
export function card(kept: Kept[], slot: number, status: string): Kept {
  return kept[slot] ?? [ROW, unstaffed(status) ? 1 : 2];
}

/** What a column of `count` tasks takes before its first page is in: a card for each row of the
 * page (PAGE at most), and the Load more button when they are more than a page. */
export function firstPageHeight(count: number, kept: Kept[]): number {
  const cards = range(Math.min(count, PAGE)).reduce(
    (sum, slot) => sum + (kept[slot]?.[0] ?? ROW),
    0,
  );
  return count > PAGE ? cards + MORE : cards;
}

/** The lines of title `card`'s box has room for, one to five: what its pitch leaves of its frame
 * and chips, with a few px given for a height kept rounded. */
export function titleLines([pitch, chips]: Kept): number {
  const room = Math.max(pitch - (GAP + FRAME + chips * CHIP_ROW), 0);
  return Math.min(Math.max(Math.floor((room + 4) / TITLE_LINE), 1), 5);
}

/** The cards `kept` (as CARDS holds them) gives the column `status` in a window `width` px wide. */
export function recallCards(kept: string, status: string, width: number): Kept[] {
  for (const line of kept.split("\n")) {
    const [drawnAt, named, ...pitches] = line.split(" ");
    if (drawnAt !== String(width) || named !== status) continue;
    const cards: Kept[] = [];
    for (const pitch of pitches) {
      const height = pitch.replace(/c+$/, "");
      const chips = pitch.length - height.length;
      if (!/^\+?\d+$/.test(height) || chips > 255) break;
      cards.push([Number(height), chips]);
    }
    return cards;
  }
  return [];
}

/** `kept` with `cards` as the column `status`'s in a window `width` px wide, first, its line
 * before and the oldest past CARDS_MAX dropped. */
export function keepCards(kept: string, status: string, width: number, cards: Kept[]): string {
  const line = [
    `${width} ${status}`,
    ...cards.map(([pitch, chips]) => `${pitch}${"c".repeat(chips)}`),
  ].join(" ");
  const own = `${width} ${status} `;
  const others = kept === "" ? [] : kept.split("\n").filter((old) => !`${old} `.startsWith(own));
  return [line, ...others].slice(0, CARDS_MAX).join("\n");
}

/** What a column keeps of the rows of its page as they stand, having kept `saved`; none when that
 * is what it kept. */
export function keepRows(rows: Kept[], saved: Kept[]): Kept[] | null {
  return rows.length > 0 && JSON.stringify(rows) !== JSON.stringify(saved) ? rows : null;
}

/** Whether a column's keep watches on, `waited` ms in: until every row has drawn its card, or no
 * row still waiting lies within the sentinel's reach of the screen, or KEEP_MS has passed. */
export function keepsWatching(drawn: boolean, near: boolean, waited: number): boolean {
  return !drawn && near && waited < KEEP_MS;
}

/** The cards this browser kept for the column `status` in a window as wide as this one. */
function keptCards(status: string): Kept[] {
  return recallCards(load(CARDS) ?? "", status, windowWidth());
}

/** The card a row of `task` waits for, in `layout`, with the build and test phases `work` gives
 * it: as tall as it will draw (`rowHeight`), and its rows of chips. */
export function rowCard(task: Task, work: MachineWork, connected: boolean, layout: Layout): Kept {
  const phases = phaseLabels(work, task.slug, connected);
  const rows = [phases.length > 0, task.links.length > 0, task.roles.length > 0];
  return [rowHeight(task, phases, layout), rows.filter(Boolean).length];
}

/**
 * The columns the board draws with no filter, in its order, each with its count: Waiting for your
 * sign-off when a task waits for it, the pipeline's (`build-test`, one rail, for Build and Test when
 * no machine `reported` them), then backlog, blocked and cancelled.
 */
export function boardShape(
  counts: [string, number][],
  statuses: string[],
  signoff: number,
  reported: boolean,
): [string, number][] {
  const known = (statuses.length === 0 ? counts.map(([status]) => status) : statuses).filter(
    (status) => reported || (status !== "build" && status !== "test"),
  );
  const count = (status: string): [string, number] => [
    status,
    counts.find(([one]) => one === status)?.[1] ?? 0,
  ];
  const shape: [string, number][] = signoff > 0 ? [["signoff", signoff]] : [];
  for (const stage of PIPELINE) {
    if (!reported && stage === "build") shape.push(["build-test", 0]);
    if (known.includes(stage) || (reported && (stage === "build" || stage === "test"))) {
      shape.push(count(stage));
    }
  }
  shape.push(...known.filter((status) => OFFLINE.includes(status)).map(count));
  return shape;
}

/** The board's shape for a browser that kept none: the pipeline and the columns off it, a task in
 * each. */
function defaultShape(): [string, number][] {
  return [...PIPELINE, ...OFFLINE].map((status) => [status, 1]);
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
export function GhostCard({ card }: { card: Kept }) {
  const [pitch, chips] = card;
  const lines = titleLines(card);
  return (
    <div className="ghost-card" style={{ height: `${Math.max(pitch - GAP, 0)}px` }}>
      <span className="bone" />
      <div>
        {range(lines).map((line) => (
          <span
            key={line}
            className="bone"
            style={line > 0 && line + 1 === lines ? width("62%") : undefined}
          />
        ))}
      </div>
      {range(chips).map((row) => (
        <span key={row} className="ghost-pill">
          <span className="bone" style={width(row % 2 === 0 ? "8ch" : "12ch")} />
        </span>
      ))}
    </div>
  );
}

/** A column of the board's ghost: a bar for its heading, a card for each of its `count` tasks (a
 * page's at most, as `cards` kept them) and Load more's room past a page; Build and Test's lines
 * of text where they hold none. */
function GhostColumn({ status, count, cards }: { status: string; count: number; cards: Kept[] }) {
  // The rail's say of why it is one, then its link; Build's Machine context; the line an empty
  // Build or Test column shows.
  const lines =
    status === "build-test"
      ? 5
      : status === "build"
        ? 1 + Number(count === 0)
        : status === "test"
          ? Number(count === 0)
          : 0;
  return (
    <div className="ghost-column" data-for={status}>
      <div>
        <span className="bone" style={width("9ch")} />
      </div>
      {lines > 0 && <Skeleton lines={lines} />}
      {status === "build-test" && (
        <div>
          <span className="bone" style={width("12ch")} />
        </div>
      )}
      <div>
        {range(Math.min(count, PAGE)).map((slot) => (
          <GhostCard key={slot} card={card(cards, slot, status)} />
        ))}
        {count > PAGE && <div style={{ height: `${MORE}px` }} />}
      </div>
    </div>
  );
}

/** The task board before its first read: its filters and, in the `shape` it last had, its columns
 * with a card for each task they held, each as this browser last drew it. */
export function GhostBoard({ shape }: { shape: [string, number][] }) {
  const column = ([status, count]: [string, number]) => (
    <GhostColumn key={status} status={status} count={count} cards={keptCards(status)} />
  );
  return (
    <>
      <div className="row filters">
        <div>
          Search tasks and messages
          <span className="ghost-field" style={{ width: "10.5rem" }} />
        </div>
        <div>
          Status
          <span className="ghost-field" style={{ width: "7.5rem" }}>
            <span className="bone" style={width("10ch")} />
          </span>
        </div>
        <div>
          Finished tasks
          <span className="ghost-field" style={{ width: "8.125rem" }}>
            <span className="bone" style={width("11ch")} />
          </span>
        </div>
      </div>
      {/* Its own class, laid out as `.kanban` is: a wait for the board never finds its ghost. */}
      <div className="ghost-kanban">
        {shape.filter(([status]) => status === "signoff").map(column)}
        {/* Two bars that wrap as the hint's words do: one line wide, two on a phone. */}
        <div className="ghost-hint">
          <span className="bone" style={width("38ch")} />{" "}
          <span className="bone" style={width("26ch")} />
        </div>
        {/* It scrolls as the pipeline's scroller does, its scroll bar in the same place; no stop
            for the keyboard. */}
        <div className="ghost-pipeline" tabIndex={-1}>
          {shape
            .filter(([status]) => status !== "signoff" && !OFFLINE.includes(status))
            .map(column)}
        </div>
        <div className="ghost-offline">
          {shape.filter(([status]) => OFFLINE.includes(status)).map(column)}
        </div>
      </div>
    </>
  );
}

/** The task board whose first read failed, with nothing drawn from this device: why, in the
 * read's own words, and a retry, in place of the ghost, which would wait for ever. */
export function UnreadBoard({ why, retry }: { why: string; retry: () => void }) {
  return (
    <section className="trouble card unread-board" role="status">
      <h2>The task board could not be read</h2>
      <p>{failed(why)}</p>
      <p className="trouble-actions">
        <Button type="button" onClick={retry}>
          Retry
        </Button>
      </p>
    </section>
  );
}

/** Build and Test of a project no machine reports for: one narrow rail where their columns go. */
function UnreportedRail({ connected }: { connected: boolean }) {
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

/** What a part of the task groups shows: its tasks, how many there are, and why a read failed. */
export interface Tree {
  rows: Task[];
  total: number;
  trouble: string;
}

/**
 * A page of the task groups `parent` holds (the roots for none, older tasks among them), from
 * `cursor` on: its listing, or why it could not be read.
 */
export async function readTree(
  scope: string,
  parent: string | null,
  cursor: string | null,
): Promise<TaskListing | string> {
  const after = cursor === null ? "" : `&cursor=${encodeURIComponent(cursor)}`;
  const selection = parent === null ? "roots=1" : `parent=${encodeURIComponent(parent)}`;
  try {
    const list = readListing(
      await getJson(scoped(scope, `/api/task-board?${selection}&older=1${after}`)),
    );
    return list ?? "The task board's answer is not a listing";
  } catch (failure) {
    return said(failure);
  }
}

/** `shown` with the page `read`: in its place for the first page, else after it, each task once;
 * a read that failed keeps what it showed and says why. */
export function grownTree(shown: Tree, read: TaskListing | string, more: boolean): Tree {
  if (typeof read === "string") return { ...shown, trouble: read };
  const rows = more ? [...shown.rows] : [];
  for (const task of read.tasks) {
    if (!rows.some((row) => row.slug === task.slug)) rows.push(task);
  }
  return { rows, total: read.counts.reduce((sum, [, count]) => sum + count, 0), trouble: "" };
}

/**
 * The task groups `parent` holds (the roots for none), read a page at a time while the part that
 * shows them is drawn: the first page again (`more` false), or the next one.
 */
function useTree(scope: string, parent: string | null): [Tree, (more: boolean) => void] {
  const [tree, setTree] = useState<Tree>({ rows: [], total: 0, trouble: "" });
  const cursor = useRef<string | null>(null);
  const alive = useRef<AbortSignal | null>(null);
  const mount = useMount();
  useEffect(() => {
    const controller = new AbortController();
    alive.current = controller.signal;
    return () => controller.abort();
  }, []);
  const load = (more: boolean) => {
    const signal = alive.current;
    void readTree(scope, parent, more ? cursor.current : null).then((read) => {
      if (signal?.aborted !== false || mount.aborted) return;
      if (typeof read !== "string") cursor.current = read.next ?? null;
      setTree((shown) => grownTree(shown, read, more));
    });
  };
  return [tree, load];
}

/** The task groups' tree: the top tasks, each opening the tasks under it. */
function TreePanel({ known, scope }: { known: Task[]; scope: string }) {
  const [tree, load] = useTree(scope, null);
  const [started] = useState({ done: false });
  useEffect(() => {
    if (started.done) return;
    started.done = true;
    load(false);
  });
  return (
    <div id="task-groups">
      {tree.trouble !== "" && (
        <section className="trouble card" role="status">
          <h3>The task groups could not be loaded</h3>
          <p>{failed(tree.trouble)}</p>
          <p className="trouble-actions">
            <Button type="button" onClick={() => load(false)}>
              Retry
            </Button>
          </p>
        </section>
      )}
      <ul>
        {tree.rows.map((task) => (
          <TreeNode key={task.slug} task={task} known={known} scope={scope} />
        ))}
      </ul>
      {tree.rows.length < tree.total && (
        <Button onClick={() => load(true)}>Load more task groups</Button>
      )}
    </div>
  );
}

/** A task of the tree, as the board shows it when it holds it, its subtasks opened on demand. */
function TreeNode({ task, known, scope }: { task: Task; known: Task[]; scope: string }) {
  const shown = known.find((one) => one.slug === task.slug) ?? task;
  const [open, setOpen] = useState(false);
  const [children, load] = useTree(scope, shown.slug);
  return (
    <NearViewport id={`tree-${shown.slug}`} height={40} listItem>
      <a href={href(`/task/${shown.slug}`)}>{shown.title}</a>{" "}
      <span className="pill">{statusWords(shown.status)}</span>
      <Button
        aria-expanded={open}
        onClick={() => {
          if (!open) load(false);
          setOpen(!open);
        }}
      >
        Subtasks
      </Button>
      {open && (
        <>
          {children.trouble !== "" && (
            <p role="status">{`Its subtasks could not be loaded: ${children.trouble}`}</p>
          )}
          <ul>
            {children.rows.map((child) => (
              <TreeNode key={child.slug} task={child} known={known} scope={scope} />
            ))}
          </ul>
          {children.rows.length < children.total && (
            <Button onClick={() => load(true)}>Load more subtasks</Button>
          )}
        </>
      )}
    </NearViewport>
  );
}

/** The rows of `column`'s page as they stand now, each its pitch and its rows of chips: a drawn
 * card's, else the place the row holds and its ghost's chips; and whether every row has drawn. */
export function drawnRows(column: Element): [Kept[], boolean] {
  let drawn = true;
  const height = (node: Element) => Math.round(node.getBoundingClientRect().height);
  const rows = [...column.querySelectorAll('[id^="row-"]')].slice(0, PAGE).map((row): Kept => {
    const found = row.querySelector(".taskcard");
    if (found) {
      const chips = [...found.querySelectorAll(".chips")].filter(
        (chips) => chips.firstElementChild !== null,
      ).length;
      return [height(found) + GAP, chips];
    }
    drawn = false;
    return [height(row), row.querySelectorAll(".ghost-pill").length];
  });
  return [rows, drawn];
}

/** Keep the rows of the column `status` as they draw, its page's at most: every 100 ms, each row's
 * pitch, again whenever one changed, while `keepsWatching`, until `signal` ends. */
export async function rememberCards(status: string, signal: AbortSignal): Promise<void> {
  let saved: Kept[] = [];
  let waited = 0;
  for (;;) {
    await sleep(100, signal);
    if (signal.aborted) return;
    waited += 100;
    const column = document.querySelector(`.kanban .column[data-status="${status}"]`);
    if (!column) return;
    const [rows, drawn] = drawnRows(column);
    const cards = keepRows(rows, saved);
    if (cards !== null) {
      save(CARDS, keepCards(load(CARDS) ?? "", status, windowWidth(), cards));
      saved = cards;
    }
    if (!keepsWatching(drawn, waitsNear(column, ".board-scroll"), waited)) return;
  }
}

/** What a column asks of the page: a page of `status` (the next one when `more`), under the
 * search `query` and the window of `days`. */
export type OnLoad = (status: string, more: boolean, query: string, days: number) => void;

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
}

/** A column of the board: its heading and count, its rows near the screen, and Load more. */
export function TaskColumn({
  status,
  work,
  connected,
  tasks,
  count,
  query,
  days,
  onLoad,
  refreshed,
}: ColumnProps) {
  const [visible, setVisible] = useState(false);
  // The layout a row's reserve is worked out for and the cards kept, read once: a window resized
  // later only misjudges rows still waiting.
  const [layout] = useState(() => layoutAt(innerWidth));
  const [kept] = useState(() => keptCards(status));
  // Complete totals make empty-column reads unnecessary. Retained rows still refresh so a
  // reconnect can remove stale cards, and a newly populated column must request its page.
  const needsPage = count > 0 || tasks.length > 0;
  const load = useRef(onLoad);
  load.current = onLoad;
  useEffect(() => {
    void refreshed;
    if (visible && needsPage) load.current(status, false, query, days);
  }, [visible, needsPage, query, days, refreshed, status]);
  const rows =
    query === "" && (status === "done" || status === "cancelled")
      ? [...tasks].sort(byRecent)
      : tasks;
  // Keep unloaded columns in place while their first page is fetched: a card's box for each row of
  // the page, each as tall as this browser last drew the row in its place, and the Load more
  // button a column of more than a page will show.
  const reserved = firstPageHeight(count, kept);
  const reading = count > 0 && tasks.length === 0;
  // Once its cards are drawn on the board as it opens, their heights for the next visit.
  const shown = tasks.length;
  const keeps = useRef({ query, days });
  keeps.current = { query, days };
  useEffect(() => {
    if (shown === 0 || keeps.current.query !== "" || keeps.current.days !== 1) return;
    const controller = new AbortController();
    void rememberCards(status, controller.signal);
    return () => controller.abort();
  }, [shown, status]);
  const ghosts = range(Math.min(count, PAGE)).map((slot) => (
    <GhostCard key={slot} card={card(kept, slot, status)} />
  ));
  return (
    <section
      className={status === "signoff" ? "column signoff" : "column"}
      data-status={status}
      aria-busy={reading}
    >
      <h2>
        {status === "signoff" ? "Waiting for your sign-off" : statusWords(status)}{" "}
        <span className="count">{count}</span>
      </h2>
      {status === "build" && <BoardContext work={work} connected={connected} />}
      {count === 0 && (status === "build" || status === "test") && (
        <p className="muted work-empty">{`No tasks with fresh ${status} activity.`}</p>
      )}
      <NearViewport
        id={`column-${status}`}
        height={reserved}
        onEnter={() => setVisible(true)}
        ghost={ghosts}
      >
        {reading && (
          <div style={{ minHeight: `${reserved}px` }} data-pending="true">
            {ghosts}
          </div>
        )}
        {/* A row waiting to be drawn holds what its own card will take (`rowHeight`). */}
        {rows.map((task) => {
          const reserve = rowCard(task, work, connected, layout);
          return (
            <NearViewport
              key={task.slug}
              id={`row-${task.slug}`}
              height={reserve[0]}
              ghost={<GhostCard card={reserve} />}
            >
              <TaskCard task={task} work={work} connected={connected} />
            </NearViewport>
          );
        })}
        {/* Only once the first page is in: while it is read, there is nothing more to load. */}
        {tasks.length > 0 && count > tasks.length && (
          <Button onClick={() => onLoad(status, true, query, days)}>Load more tasks</Button>
        )}
      </NearViewport>
    </section>
  );
}

export interface TaskBoardProps {
  scope: string;
  tasks: Task[];
  work: MachineWork;
  connected: boolean;
  signoff: string[];
  counts: [string, number][];
  statuses: string[];
  refreshed: number;
  onFilter: (query: string, days: number) => void;
  onLoad: OnLoad;
}

/** The board: its filters, its columns and its task groups. */
export function TaskBoard({
  scope,
  tasks,
  work,
  connected,
  signoff,
  counts,
  statuses,
  refreshed,
  onFilter,
  onLoad,
}: TaskBoardProps) {
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState("");
  const [days, setDays] = useState(1);
  const [subtree, setSubtree] = useState(false);
  const named = statuses.length === 0 ? counts.map(([status]) => status) : statuses;
  // With no machine reporting this project's work, nothing can place a task in Build or Test: one
  // narrow rail says so once, where two empty columns would each repeat it.
  const unreported = work.machines.length === 0;
  const machine = (status: string) => status === "build" || status === "test";
  const listed = unreported ? named.filter((status) => !machine(status)) : withMachineStages(named);
  const filtered = tasks.filter(
    (task) =>
      (isTask(task) || search !== "") &&
      (selected === "" || taskStage(work, task, connected) === selected),
  );
  if (search === "") filtered.sort(byPriority);
  const stage = (task: Task) => taskStage(work, task, connected);
  const column = (status: string) => (
    <TaskColumn
      key={status}
      status={status}
      work={work}
      connected={connected}
      tasks={filtered.filter((task) => stage(task) === status && !signoff.includes(task.slug))}
      count={counts.find(([one]) => one === status)?.[1] ?? 0}
      query={search}
      days={days}
      onLoad={onLoad}
      refreshed={refreshed}
    />
  );
  return (
    <>
      <div className="row filters">
        <label>
          {"Search tasks and messages "}
          <input
            className="dx-input"
            type="search"
            placeholder="Slug or words"
            defaultValue=""
            onInput={(event) => {
              const query = event.currentTarget.value;
              setSearch(query);
              onFilter(query, days);
            }}
          />
        </label>
        {/* biome-ignore lint/a11y/noLabelWithoutControl: the field's trigger, a button, is the control it labels */}
        <label>
          {"Status "}
          <SelectField
            label="Status"
            value={selected}
            options={[
              ["", "All statuses"],
              ...listed.map((status): [string, string] => [status, statusLabel(status)]),
            ]}
            onChange={setSelected}
          />
        </label>
        {/* biome-ignore lint/a11y/noLabelWithoutControl: the field's trigger, a button, is the control it labels */}
        <label>
          {"Finished tasks "}
          <SelectField
            label="Finished tasks"
            value={String(days)}
            options={[
              ["1", "Last 24 hours"],
              ["7", "Last week"],
            ]}
            onChange={(value) => {
              const window = value === "7" ? 7 : 1;
              setDays(window);
              onFilter(search, window);
            }}
          />
        </label>
      </div>
      <div className="kanban">
        {signoff.length > 0 && (
          <TaskColumn
            status="signoff"
            work={work}
            connected={connected}
            tasks={filtered.filter((task) => signoff.includes(task.slug))}
            count={signoff.length}
            query={search}
            days={days}
            onLoad={onLoad}
            refreshed={refreshed}
          />
        )}
        {selected === "" && (
          <p id="board-scroll-hint" className="board-scroll-hint muted">
            Columns continue right → · Swipe, scroll, or focus the board and use arrow keys.
          </p>
        )}
        {(selected === "" || (PIPELINE as readonly string[]).includes(selected)) && (
          // biome-ignore lint/a11y/useSemanticElements: the scroller is a region of its own, as the Dioxus page draws it
          <div
            className="board-scroll"
            // biome-ignore lint/a11y/noNoninteractiveTabindex: the keyboard scrolls the columns from it
            tabIndex={0}
            role="region"
            aria-label="Task pipeline columns"
            aria-describedby={selected === "" ? "board-scroll-hint" : ""}
          >
            <div className="pipeline">
              {PIPELINE.map((stage) => (
                <Fragment key={stage}>
                  {unreported && stage === "build" && (selected === "" || machine(selected)) && (
                    <UnreportedRail connected={connected} />
                  )}
                  {listed.includes(stage) &&
                    (selected === "" || selected === stage) &&
                    column(stage)}
                </Fragment>
              ))}
            </div>
          </div>
        )}
        <div className="offline">
          {listed
            .filter((status) => OFFLINE.includes(status))
            .map((status) => (selected === "" || selected === status ? column(status) : null))}
        </div>
      </div>
      <section className="task-groups" aria-labelledby="task-groups-title">
        <h2 id="task-groups-title">Task groups</h2>
        <Button
          aria-expanded={subtree}
          aria-controls="task-groups"
          onClick={() => setSubtree(!subtree)}
        >
          {`${subtree ? "Hide" : "Show"} task groups`}
        </Button>
        {subtree && <TreePanel known={tasks} scope={scope} />}
      </section>
    </>
  );
}
