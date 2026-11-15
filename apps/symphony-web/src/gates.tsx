// The task page's gates: every stamp or check the task needs to land, each met, missing, waived,
// held or unknown, with who, when and why, drawn the same way for every project from
// `GET /api/task-gates` (docs/TASKS.md#gates), as the Dioxus client drew them
// (apps/symphony-ui/src/task_gates.rs).
//
// The section reads its answer when it nears the screen, and the head, the brief and the message
// box never wait on it: a project's answer may start its landing tool. Until it has an answer it is
// its own outline, the card with a shadow line for each gate it expects (`Room`): as many as the
// server last answered for the task, else as this browser last drew, else four, and none when it
// knows there are none; so the page below does not move when the gates arrive. It reads them again
// when the task changes, one of its runs starts or ends (`workOf`), or the page's feed counts
// another event that may change them (`nudged`), the last answer shown meanwhile, and only the
// latest read is drawn: a newer one aborts the one before. A task nothing gates shows no section,
// and a read that failed says so in its place while the rest of the page works.
//
// Wide, a gate is one row of bounded columns, a part wrapping within its own when its words need
// more room, so every word shows without a hover; on a phone, three lines (its name and state, its
// detail, who and when). A link is drawn only for a path of the app or a web address (`linkable`),
// written under the project's base (`href`), on the words of the detail that are its evidence
// (`evidence`). Plugin text keeps its own direction. Its style is gates.css; what it knows without a
// page is gates-model.ts.

import { type CSSProperties, Fragment, useEffect, useRef, useState } from "react";
import { Button } from "./button";
import {
  byline,
  counts,
  evidence,
  failed,
  type Gates,
  gatesOf,
  KEPT,
  keep,
  latest,
  localMinutes,
  pieces,
  READING,
  Room,
  readTaskGates,
  recall,
  seenOf,
  WORDS,
  type Words,
  waitsOn,
} from "./gates-model";
import { linkable } from "./links";
import { useMount } from "./mount";
import { href, scoped } from "./project";
import { Failure, getJson, said } from "./request";
import { currentRoute } from "./route";
import { load, save } from "./storage";
import { useVisibility, windowWidth } from "./viewport";
import { type Gate, type GatesSeen, TASK_GATES_PATH, type TaskGates } from "./wire";

/** The window's width in px; none where there is no window (the unit tests' Node). */
export function width(): number | null {
  return typeof innerWidth === "number" ? windowWidth() : null;
}

/** The room this browser kept for the task `slug`. */
export function kept(slug: string): Room | null {
  const lines = load(KEPT);
  return lines === null ? null : recall(lines, slug);
}

/**
 * Keep the room the task `slug`'s gates now take: `seen`, at the height its section was drawn in a
 * window this wide (none where it drew nothing).
 */
export function remember(slug: string, seen: GatesSeen): void {
  const section =
    typeof document === "undefined"
      ? null
      : document.getElementById(`gates-${slug}`)?.querySelector("section.task-gates");
  const wide = width();
  const drawn: [number, number] | null =
    section && wide !== null ? [wide, Math.round(section.getBoundingClientRect().height)] : null;
  save(KEPT, keep(load(KEPT) ?? "", slug, { seen, drawn }));
}

/**
 * Once an answer is drawn: the room it took in this window, kept for the next visit, and the branch
 * it was answered for told to `onBranch` with the slug, which the page matches moved branches
 * against.
 */
export function drawn(
  slug: string,
  answer: TaskGates,
  onBranch?: (slug: string, branch: string) => void,
): void {
  remember(slug, seenOf(answer));
  if (onBranch !== undefined && answer.branch !== "") onBranch(slug, answer.branch);
}

/**
 * Read the gates of `slug`, under the page's checkout, given up with its request once `signal`
 * ends: the server's 404 is a task it has no gates for.
 */
export async function readGates(slug: string, signal: AbortSignal): Promise<Gates> {
  const path = `${TASK_GATES_PATH}?slug=${encodeURIComponent(slug)}`;
  try {
    const answer = readTaskGates(await getJson(scoped(currentRoute().scope, path), signal));
    return gatesOf(answer === null ? { failed: WORDS.unreadable } : { found: answer });
  } catch (failure) {
    if (failure instanceof Failure && failure.kind === "missing") return gatesOf({ found: null });
    return gatesOf({ failed: said(failure) });
  }
}

/**
 * The section's reads of the task `slug`: each read drops the one still running, aborting it, so
 * only the latest is drawn (`show`), and none is once the section stops or the kind's `mount` ends.
 */
export class GatesReader {
  private running: AbortController | null = null;

  constructor(
    private readonly slug: string,
    private readonly show: (gates: Gates) => void,
    private readonly mount: AbortSignal,
  ) {}

  /** Ask for the gates, dropping the read still running; settles once this read is done. */
  async read(): Promise<void> {
    this.running?.abort();
    const reading = new AbortController();
    this.running = reading;
    const answer = await readGates(this.slug, reading.signal);
    if (reading.signal.aborted || this.mount.aborted) return;
    this.running = null;
    this.show(answer);
  }

  /** Drop the read still running: the section is gone. */
  stop(): void {
    this.running?.abort();
    this.running = null;
  }
}

export interface TaskGatesPanelProps {
  slug: string;
  /** The task's `updated`: a change reads the gates again. */
  revision: number;
  /** What the server last answered for the task (`TaskPage.gates`): the room held until its answer. */
  seen?: GatesSeen | null;
  /** The task's machine work (`workOf`): a change reads the gates again. */
  work?: number;
  /** Bumped by the page's feed for an event that may change the gates: each reads them again. */
  nudged?: number;
  /** Told each answer's branch, once drawn, with the slug. */
  onBranch?: (slug: string, branch: string) => void;
}

/**
 * The gates of the task `slug`, read when the section nears the screen and again when the task
 * changes (`revision`), when its machine work does (`work`) and when the page's feed counts another
 * event that may change them (`nudged`), the last answer shown meanwhile.
 */
export function TaskGatesPanel({
  slug,
  revision,
  seen = null,
  work = 0,
  nudged = 0,
  onBranch,
}: TaskGatesPanelProps) {
  const [gates, setGates] = useState<Gates>(READING);
  // How many times the reader asked to read again.
  const [retried, setRetried] = useState(0);
  const mount = useMount();
  const [reader] = useState(() => new GatesReader(slug, setGates, mount));
  const [room] = useState(() => Room.of(seen, kept(slug)));
  const [near, watch] = useVisibility(false);
  const told = useRef(onBranch);
  told.current = onBranch;
  useEffect(() => {
    void [revision, work, nudged, retried];
    if (near) void reader.read();
  }, [near, reader, revision, work, nudged, retried]);
  useEffect(() => () => reader.stop(), [reader]);
  // Once an answer is drawn, the room it took in this window, for the next visit, and the branch it
  // was answered for, for the page's feed.
  useEffect(() => {
    if (gates.kind === "read") drawn(slug, gates.answer, told.current);
  }, [gates, slug]);
  return (
    <div id={`gates-${slug}`} className="task-gates-slot">
      {!near && <div className="viewport-sentinel" aria-hidden="true" ref={watch} />}
      <GatesView
        gates={gates}
        now={Date.now() / 1000}
        room={room}
        width={width()}
        onRetry={() => {
          setGates(READING);
          setRetried((count) => count + 1);
        }}
      />
    </div>
  );
}

/**
 * The section on the task's ghost, before its page is read: the outline the section itself draws
 * until its answer, for the room this browser kept for the task `slug`, else the room of the last
 * task it kept one for (`latest`), else four gates (`Room.of` with nothing from the server yet), so
 * the brief under it stays put as the page comes in its place.
 */
export function GhostGates({ slug }: { slug: string }) {
  const [room] = useState(() => {
    const lines = load(KEPT) ?? "";
    return Room.of(null, recall(lines, slug) ?? latest(lines, width()));
  });
  return <GatesView gates={READING} now={0} room={room} width={width()} onRetry={() => {}} />;
}

/** A shadow bar's style: its width, `--w`. */
const bar = (wide: string) => ({ "--w": wide }) as CSSProperties;

export interface GatesViewProps {
  gates: Gates;
  /** Now, in seconds since the epoch: what the gates' times are told against. */
  now: number;
  /** What the section holds until it has an answer. */
  room: Room;
  /** The window's width, in px, which the height `room` holds was drawn at. */
  width: number | null;
  onRetry: () => void;
}

/**
 * The section for `gates`, its times told against `now`: nothing for a task nothing gates. Until it
 * has an answer it holds `room`, at the height drawn before in a window `width` px wide.
 */
export function GatesView({ gates, now, room, width, onRetry }: GatesViewProps) {
  const head = (
    <h2 className="eyebrow" id="task-gates-title">
      {WORDS.title}
    </h2>
  );
  const note = (text: string) => (
    <section className="task-gates card" data-gates="failed" aria-labelledby="task-gates-title">
      <div className="gates-head">{head}</div>
      <p className="gates-note" role="status">
        {text}
      </p>
      <Button type="button" onClick={() => onRetry()}>
        {WORDS.retry}
      </Button>
    </section>
  );
  const { count, waiting, unsigned } = room.seen;
  switch (gates.kind) {
    case "reading": {
      if (count === 0) return null;
      // The height drawn before is held exactly, so the outline is never taller than its answer
      // either.
      const hold = Room.held(room, width);
      // The gates with no who and when come last, their third line on a phone empty.
      const signed = Math.max(count - unsigned, 0);
      return (
        <section
          className="task-gates card"
          data-gates="reading"
          aria-labelledby="task-gates-title"
          aria-busy="true"
          style={hold === null ? undefined : { height: `${hold}px` }}
        >
          <div className="gates-head">
            {head}
            {waiting > 0 && (
              <span className="pill gates-waits gate-shadow">
                <span className="gate-bar" style={bar("9em")} />
              </span>
            )}
            <span className="gates-counts">
              <span className="gate-bar" style={bar("11em")} />
            </span>
          </div>
          <p className="gates-said" role="status">
            {WORDS.reading}
          </p>
          <ul className="gates" aria-hidden="true">
            {Array.from({ length: count }, (_, row) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: the shadow rows are alike, and only their count changes
              <li key={row} className="gate gate-shadow">
                <span className="gate-name">
                  <span className="gate-label">
                    <span className="gate-bar" style={bar(`${6 + (row % 3)}em`)} />
                  </span>
                </span>
                <span className="pill gate-state">{" "}</span>
                <span className="gate-detail">
                  <span className="gate-bar" style={bar(`${55 + ((row * 17) % 35)}%`)} />
                </span>
                <span className="gate-by">
                  {row < signed && <span className="gate-bar" style={bar("7em")} />}
                </span>
              </li>
            ))}
          </ul>
        </section>
      );
    }
    case "slow":
      return note(WORDS.slow);
    case "failed":
      return note(failed(gates.why));
    case "read": {
      const { answer } = gates;
      if (answer.gates.length === 0) return null;
      const today = localMinutes(now);
      const waits = waitsOn(answer);
      return (
        <section className="task-gates card" data-gates="read" aria-labelledby="task-gates-title">
          <div className="gates-head">
            {head}
            {waits !== null && (
              <span className={`pill gate-${waits[0]} gates-waits`}>
                {WORDS.waitsOn}
                {waits[1].map((name, at, names) => (
                  // biome-ignore lint/suspicious/noArrayIndexKey: the names are drawn in their order, which a new answer draws whole
                  <Fragment key={at}>
                    {at > 0 && (at + 1 === names.length ? WORDS.and : WORDS.comma)}
                    <span dir="auto">{name}</span>
                  </Fragment>
                ))}
              </span>
            )}
            <span className="gates-counts">{counts(answer)}</span>
          </div>
          <ul className="gates">
            {answer.gates.map((gate) => (
              <GateRow key={gate.name} gate={gate} byline={byline(gate, today)} />
            ))}
          </ul>
        </section>
      );
    }
  }
}

/** `parts` as drawn: each code span a `code`, the rest its words. */
function spoken(parts: Words) {
  return parts.map(([code, part], at) =>
    code ? (
      // biome-ignore lint/suspicious/noArrayIndexKey: a text's parts keep their order
      <code key={at}>{part}</code>
    ) : (
      part
    ),
  );
}

export interface GateRowProps {
  gate: Gate;
  /** Who, when and the commit its words do not say (`byline`). */
  byline: string[];
}

/** One gate: its name (and link), its state, its detail and why, and who and when. */
export function GateRow({ gate, byline }: GateRowProps) {
  const state = gate.state;
  const link = typeof gate.link === "string" && linkable(gate.link) ? gate.link : null;
  const why = typeof gate.why === "string" && gate.why !== "" ? gate.why : null;
  // With no words to put it on, a link goes on the gate's name.
  const [named, linked] = link !== null && gate.detail === "" ? [link, null] : [null, link];
  const [before, inside, after] = pieces(
    gate.detail,
    linked === null ? null : evidence(gate.detail, linked, gate.commit),
  );
  return (
    <li className={`gate gate-${state}`} data-gate={gate.name} data-state={state}>
      <span className="gate-name">
        <span className="gate-label" dir="auto">
          {/* Written under the project's base, so a copy or a new tab opens the page a click does. */}
          {named === null ? gate.name : <a href={href(named)}>{gate.name}</a>}
        </span>
      </span>
      <span className="pill gate-state">{state}</span>
      <span className="gate-detail">
        {gate.detail !== "" && (
          <span dir="auto">
            {spoken(before)}
            {linked !== null && <a href={href(linked)}>{spoken(inside)}</a>}
            {spoken(after)}
          </span>
        )}
        {why !== null && (
          <span className="gate-why">
            {gate.detail !== "" && WORDS.because}
            <span dir="auto">{why}</span>
          </span>
        )}
      </span>
      <span className="gate-by">
        {byline.map((part, at) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: who, when and the commit keep their order
          <Fragment key={at}>
            {at > 0 && WORDS.dot}
            <span dir="auto">{part}</span>
          </Fragment>
        ))}
      </span>
    </li>
  );
}
