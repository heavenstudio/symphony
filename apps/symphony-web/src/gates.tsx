// The task page's gates: every stamp or check the task needs to land, each met, missing, waived,
// held or unknown, with who, when and why, drawn the same way for every project from
// `GET /api/task-gates` (docs/workflow/TASKS.md#gates), as the Dioxus client drew them (its
// `task_gates.rs`, gone with its task page).
//
// The section is a part of its page (task-live.ts's `gatesPart`, in the page's store): its answer
// is asked as the page mounts, behind neither the page's read nor the thread's, and the head, the
// brief and the message box never wait on it. Until it has an answer it is its own outline, the
// card with a shadow line for each gate it expects (`Room`): as many as the server last answered
// for the task, else as this browser last drew for it, else for the last task it drew, else four,
// and none when it knows there are none; so the page below does not move when the gates arrive.
// The part reads itself again for a feed event newer than its answer that may change them (the
// task changed, one of its runs started or ended, `namesGates`), the last answer shown meanwhile.
// A task nothing gates shows no section, and a read that failed says so in its place, tried again
// by itself, its Retry saying it reads while a read is out, while the rest of the page works. Its
// head's pill says what the task waits on, by its status (`waitsOn`): the gates only in review.
//
// Wide, a gate is one row of bounded columns, a part wrapping within its own when its words need
// more room, so every word shows without a hover; on a phone, three lines (its name and state, its
// detail, who and when). A link is drawn only for a path of the app or a web address (`linkable`),
// written under the project's base (`href`), on the words of the detail that are its evidence
// (`evidence`). Plugin text keeps its own direction. Its style is gates.css; what it knows without a
// page is gates-model.ts.

import { type CSSProperties, Fragment, useContext, useEffect, useMemo } from "react";
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
  recall,
  seenOf,
  type Waiter,
  WORDS,
  type Words,
  waitsOn,
} from "./gates-model";
import { linkable } from "./links";
import { PartsContext, type Shown, usePart } from "./parts";
import { href } from "./project";
import { said } from "./request";
import { load, save } from "./storage";
import { gatesPart } from "./task-live";
import { windowWidth } from "./viewport";
import type { Gate, GatesSeen, TaskGates } from "./wire";

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

/** Once an answer is drawn: the room it took in this window, kept for the next visit. */
export function drawn(slug: string, answer: TaskGates): void {
  remember(slug, seenOf(answer));
}

/** What the section has of the task's gates, from its part: the server's 404 is a task it has no
 * gates for, and the last answer stays shown while a later read fails. */
export function gatesShown(shown: Shown<TaskGates>): Gates {
  if (shown.value !== undefined) return { kind: "read", answer: shown.value };
  if (shown.missing) return gatesOf({ found: null });
  return shown.error === null ? READING : gatesOf({ failed: said(shown.error) });
}

/**
 * The room the section holds for the task `slug` until its answer: what the server last answered
 * for it (`seen`, which its page brings) with what this browser kept for it; before the page has
 * come, what this browser kept for it, else for the last task it drew in a window as wide, else
 * four gates.
 */
export function roomFor(slug: string, seen: GatesSeen | null): Room {
  const lines = load(KEPT) ?? "";
  const own = recall(lines, slug);
  return Room.of(seen, seen === null ? (own ?? latest(lines, width())) : own);
}

export interface TaskGatesPanelProps {
  slug: string;
  /** The checkout the route names (`""`, the main one). */
  scope: string;
  /** The task as its page has it, once it has: its status says what the pill names (`waitsOn`). */
  task: Waiter | null;
  /** What the server last answered for the task (`TaskPage.gates`): the room held until its answer. */
  seen?: GatesSeen | null;
  /** Whether the page already says it loads (`GatesViewProps.quiet`). */
  quiet?: boolean;
}

/** The gates of the task `slug`, a part of its page: its outline until its answer, then the gates. */
export function TaskGatesPanel({
  slug,
  scope,
  task,
  seen = null,
  quiet = false,
}: TaskGatesPanelProps) {
  const part = gatesPart(scope, slug);
  const parts = useContext(PartsContext);
  const shown = usePart(part);
  const gates = gatesShown(shown);
  const said = seen === null ? "" : `${seen.count} ${seen.waiting} ${seen.unsigned}`;
  // biome-ignore lint/correctness/useExhaustiveDependencies: the room follows what the server said, by its counts
  const room = useMemo(() => roomFor(slug, seen), [slug, said]);
  // Once an answer is drawn, the room it took in this window, for the next visit.
  useEffect(() => {
    if (gates.kind === "read") drawn(slug, gates.answer);
  }, [gates, slug]);
  return (
    <div id={`gates-${slug}`} className="task-gates-slot">
      <GatesView
        gates={gates}
        task={task}
        now={Date.now() / 1000}
        room={room}
        width={width()}
        quiet={quiet}
        retrying={shown.retrying}
        onRetry={() => void parts?.read(part)}
      />
    </div>
  );
}

/** A shadow bar's style: its width, `--w`. */
const bar = (wide: string) => ({ "--w": wide }) as CSSProperties;

export interface GatesViewProps {
  gates: Gates;
  /** The task the gates are of, which says what the pill names (`waitsOn`); none on its ghost. */
  task: Waiter | null;
  /** Now, in seconds since the epoch: what the gates' times are told against. */
  now: number;
  /** What the section holds until it has an answer. */
  room: Room;
  /** The window's width, in px, which the height `room` holds was drawn at. */
  width: number | null;
  /** Whether the page already says it loads: the section's reading is then not said again. */
  quiet?: boolean;
  /** A read is out after the last failed: the retry says so, and asks for none more. */
  retrying?: boolean;
  onRetry: () => void;
}

/**
 * The section for `gates`, its times told against `now`: nothing for a task nothing gates. Until it
 * has an answer it holds `room`, at the height drawn before in a window `width` px wide.
 */
export function GatesView({
  gates,
  task,
  now,
  room,
  width,
  quiet = false,
  retrying = false,
  onRetry,
}: GatesViewProps) {
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
      <Button type="button" disabled={retrying} onClick={() => onRetry()}>
        {retrying ? WORDS.retrying : WORDS.retry}
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
          <p className="gates-said" role={quiet ? undefined : "status"}>
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
      const waits = task === null ? null : waitsOn(answer, task);
      return (
        <section className="task-gates card" data-gates="read" aria-labelledby="task-gates-title">
          <div className="gates-head">
            {head}
            {/* Before the task's page says who it waits on: the pill's outline, where the room
                kept says one was drawn, so the page's coming moves nothing under the section. */}
            {task === null && waiting > 0 && (
              <span className="pill gates-waits gate-shadow">
                <span className="gate-bar" style={bar("9em")} />
              </span>
            )}
            {waits !== null && (
              <span className={`pill ${waits.tone} gates-waits`}>
                {WORDS.waitsOn}
                {waits.names.map((name, at, names) => (
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
