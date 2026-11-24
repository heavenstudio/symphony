// A task's page (`/task/SLUG`): its head (slug, title, state, who filed it, the owner's priority and
// cancel, its facts, whose it is where its board names an owner (owner.tsx's line, a chunk of its
// own), its roles and their sessions (session.tsx's control, a chunk of its own, for a holder whose
// session it names), its builds and tests), its gates, its subtasks, its timeline,
// its brief, its changes, its execution attempts where it has some (attempts.tsx, a chunk of their
// own), its thread and the composer under it. Each part is drawn from its own read (task-live.ts's
// parts, in the page's store) as soon as that read is there, and is its own mock-up until then:
// the head and the brief at the height this browser last drew them, the gates' outline, the
// timeline's box, the thread's and the composer's ghosts, so a part that comes moves none of the
// others. Live, it is task-live.ts's `LiveTask`, under the board's notices.

import {
  type ComponentType,
  type CSSProperties,
  memo,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";
import { AttachControls, useAttach } from "./attach";
import type { AttemptsProps } from "./attempts";
import { BlocksView } from "./blocks";
import { Button } from "./button";
import { FRESH, type Freshness, shown } from "./cache";
import { chunkFailed, useChunk } from "./chunk";
import { useFrame } from "./frame";
import { TaskGatesPanel } from "./gates";
import { taskKind } from "./kind";
import { PageNotices } from "./live";
import { useMount } from "./mount";
import type { NextActionsProps } from "./nextactions";
import type { OwnerLineProps } from "./owner";
import { PartsContext, type Shown, usePart } from "./parts";
import { href, scoped } from "./project";
import { getJson, post, said } from "./request";
import { currentRoute } from "./route";
import { SelectField } from "./select";
import type { GoToSessionProps } from "./session";
import { field, STALE, useStore } from "./shared";
import { load, save } from "./storage";
import { TaskDiffPanel } from "./task-diff";
import { LiveTask, offerPart, type Thread } from "./task-live";
import { readBrief } from "./task-read";
import { Textarea } from "./textarea";
import { TimelineSlot } from "./timeline-slot";
import { failed as failedRead, ready, Trouble } from "./trouble";
import { drawnHeight, drawnStyle, NearViewport, rememberDrawn, Settle, Skeleton } from "./viewport";
import type {
  Block,
  TaskPage as Page,
  Role,
  SessionGo,
  TaskBrief,
  TaskEntry,
  TaskOffer,
} from "./wire";
import { TASK_ACTION_ROUTE, TASK_HAND_ROUTE } from "./wire";
import { CHECKING } from "./words";
import { statusWords, WorkPhases } from "./work-phases";

/** The performance mark set once the board is first drawn (the phone test times it). */
const READY_MARK = "board-ready";

/** `path` under the route's checkout, as the page's posts and reads address it. */
function url(path: string): string {
  return scoped(currentRoute().scope, path);
}

/** A bone's width, `--w`, as the ghosts' style sheet reads it. */
function bone(width: string): CSSProperties {
  return { "--w": width } as CSSProperties;
}

/** A task's priorities, as its field offers them. */
const PRIORITIES: [string, string][] = [0, 1, 2, 3, 4].map((value) => [`${value}`, `p${value}`]);

/** Whose line a task's message is, as its thread draws it: a person's (the owner's, or a person's
 * a board named), the board's own, or an agent's. */
export function voice(message: TaskEntry["message"]): "owner" | "system" | "agent" {
  if (message.sender === "owner" || message.person) return "owner";
  return message.kind === "system" ? "system" : "agent";
}

/** A message of a task's thread: its time, who sent it to whom, and its text or blocks. */
export const TaskMessage = memo(function TaskMessage({ entry }: { entry: TaskEntry }) {
  const { message } = entry;
  const who = voice(message);
  return (
    <div className={`entry ${who}`}>
      <time>{entry.stamp}</time>{" "}
      <span className="from">
        {message.sender}
        {message.recipient !== null && ` → ${message.recipient}`}
      </span>
      {message.kind === "verdict" && <b>verdict </b>}
      {/* An owner message is literal text, unless it names an upload: the server then sends its
          blocks, the text kept as typed and the files drawn. */}
      {who === "agent" || (who === "owner" && entry.blocks.length > 0) ? (
        <div className="entry-text">
          <BlocksView blocks={entry.blocks} />
        </div>
      ) : (
        <div className="entry-text literal">{message.text}</div>
      )}
    </div>
  );
});

/** A message of a task's thread while it loads: a bar for its time and sender, and two lines. */
function GhostEntry() {
  return (
    <div className="ghost-entry">
      <span className="bone" />
      <Skeleton lines={2} />
    </div>
  );
}

/** The composer while its task's page loads: its field, its Attach button and its recipient and
 * Send controls. */
export function GhostComposer() {
  return (
    <div className="ghost-composer">
      <div>
        <span className="bone" style={bone("7ch")} />
      </div>
      <div className="ghost-field ghost-textarea" />
      <div>
        <span className="ghost-field" style={{ width: "6rem" }} />
        <span className="bone" style={bone("22ch")} />
      </div>
      <div>
        <span className="bone" style={bone("2ch")} />
        <span className="ghost-field" style={{ width: "9rem" }} />
        <span className="ghost-field" style={{ width: "4.5rem" }} />
      </div>
    </div>
  );
}

/** A task's thread and composer while the thread loads: messages, then the composer. */
export function GhostThread({ quiet = false }: { quiet?: boolean }) {
  return (
    <>
      <span className="status-text" role={quiet ? undefined : "status"}>
        Loading the thread…
      </span>
      <div className="ghost-history">
        <GhostEntry />
        <GhostEntry />
        <GhostEntry />
      </div>
      <GhostComposer />
    </>
  );
}

/** A kept title's and facts' lines, as many as either is likely to take: each part is as tall as
 * kept and shows the lines its height holds. */
const TITLE_LINES = ["72%", "88%", "64%", "80%", "56%", "76%"];
const FACT_LINES = ["30%", "42%", "36%", "46%", "34%", "40%"];

/** A part's height as kept, for its ghost's box. */
function keptLines(height: number): CSSProperties {
  return { height: `${height}px` };
}

export interface GhostHeadProps {
  height: number | null;
  title: number | null;
  facts: number | null;
}

/**
 * A task's head card before its read, `height` px tall and its title and facts as tall as kept: a
 * bar for its slug, one for each line of title, its state and priority pills, who filed it and the
 * priority and cancel controls, a bar for each fact, and its role chip. A title not kept is one line
 * wide and two on a phone, facts not kept one line.
 */
export function GhostHead({ height, title, facts }: GhostHeadProps) {
  return (
    <div className="card ghost-head" style={drawnStyle(height)}>
      <div>
        <span className="bone" style={bone("18ch")} />
      </div>
      {title !== null ? (
        <div className="ghost-title kept" style={keptLines(title)}>
          {TITLE_LINES.map((width) => (
            <span key={width} className="bone" style={bone(width)} />
          ))}
        </div>
      ) : (
        <div className="ghost-title">
          <span className="bone" style={bone("72%")} />
          <span className="bone" style={bone("45%")} />
        </div>
      )}
      {/* As the real row: the state's pill, who filed it, then the owner's controls apart. */}
      <div className="row ghost-meta">
        <span className="ghost-pill">
          <span className="bone" style={bone("7ch")} />
        </span>
        <span className="bone" style={bone("13ch")} />
        <div className="row ghost-controls">
          <span className="bone" style={bone("6ch")} />
          <span className="ghost-field" style={{ width: "4rem" }} />
          <span className="ghost-field" style={{ width: "7rem" }} />
        </div>
      </div>
      {facts !== null ? (
        <div className="ghost-meta ghost-facts" style={keptLines(facts)}>
          {FACT_LINES.map((width) => (
            <div key={width}>
              <span className="bone" style={bone(width)} />
            </div>
          ))}
        </div>
      ) : (
        <div className="ghost-meta">
          <span className="bone" style={bone("40%")} />
        </div>
      )}
      <span className="ghost-pill">
        <span className="bone" style={bone("14ch")} />
      </span>
    </div>
  );
}

/** A brief's ghost's paragraphs, of four and three bars. */
const BRIEF_LINES: [number, number] = [4, 3];
/** In tenths of a px as board.css draws them in a `.card`: a bar `.8em` tall, a line's leading
 * above each bar of a paragraph (its margins collapse to `.5lh - .4em`), a paragraph's gap of `1em`
 * above the next, and the card's border and padding (1 px and 16 px each side). */
const BAR = 128;
const LEADING = 60;
const GAP = 160;
const FRAME = 340;

/**
 * The bars of each of a brief's ghost's two paragraphs in a box `height` px tall (border and padding
 * in), when one was kept: only the bars it shows whole, those whose bottom stays inside its content,
 * so none is cut off at its edge; all of them with no height kept.
 */
export function briefBars(height: number | null): [number, number] {
  if (height === null) return BRIEF_LINES;
  const room = Math.max(height * 10 - FRAME, 0);
  const bars: [number, number] = [0, 0];
  let bottom = 0;
  for (const [paragraph, lines] of BRIEF_LINES.entries()) {
    for (let line = 0; line < lines; line += 1) {
      const above = line === 0 && paragraph > 0 ? GAP : LEADING;
      if (bottom + above + BAR > room) return bars;
      bottom += above + BAR;
      bars[paragraph] = (bars[paragraph] ?? 0) + 1;
    }
  }
  return bars;
}

/** A brief's card before its read: two paragraphs of bars, in a box `height` px tall when kept. */
export function GhostBrief({ height }: { height: number | null }) {
  const [first, second] = briefBars(height);
  return (
    <div className="card ghost-brief" style={drawnStyle(height)}>
      {first > 0 && <Skeleton lines={first} />}
      {second > 0 && <Skeleton lines={second} />}
    </div>
  );
}

/** The task's role chips, each with its holder's Go to session control, under `scope`'s board. */
export function RoleChips({
  roles,
  sessions,
  local,
  scope,
}: {
  roles: Role[];
  sessions: Record<string, SessionGo>;
  local: boolean;
  scope: string;
}) {
  return (
    <span className="chips">
      {roles.map((role) => (
        <span
          key={`${role.role}:${role.handle}`}
          className={`chip health-${role.health}`}
          title={`${role.role} is ${role.health}`}
        >
          {`${role.role} ${role.handle}`}
          <SessionPart
            handle={role.handle}
            go={sessions[role.handle] ?? null}
            local={local}
            scope={scope}
          />
        </span>
      ))}
    </span>
  );
}

export interface OwnerControlsProps {
  slug: string;
  priority: number;
  status: string;
}

/**
 * The owner's controls of a task: its priority, and its cancel behind a confirmation while it is
 * open. While the page may show what is out of date, the task may have moved since: each waits,
 * saying so.
 */
export function OwnerControls({ slug, priority, status }: OwnerControlsProps) {
  const mount = useMount();
  const [selected, setSelected] = useState(`${priority}`);
  const [confirming, setConfirming] = useState(false);
  const [cancelled, setCancelled] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState("");
  const working = useRef(false);
  const current = useRef(priority);
  current.current = priority;
  useEffect(() => setSelected(`${priority}`), [priority]);
  const terminal = cancelled || status === "done" || status === "cancelled";
  useEffect(() => {
    if (terminal) setConfirming(false);
  }, [terminal]);
  const stale = useStore(STALE);
  const begin = (): boolean => {
    if (working.current || STALE.get()) return false;
    working.current = true;
    setBusy(true);
    setResult("");
    return true;
  };
  const end = (said: string) => {
    working.current = false;
    setBusy(false);
    setResult(said);
  };
  const choose = async (value: string) => {
    if (working.current || STALE.get()) return;
    setSelected(value);
    begin();
    try {
      await post(url("/api/task-priority"), { slug, priority: value });
      if (mount.aborted) return;
      end("Priority saved");
    } catch (error) {
      if (mount.aborted) return;
      setSelected(`${current.current}`);
      end(said(error));
    }
  };
  const cancel = async () => {
    if (!begin()) return;
    try {
      await post(url("/api/task-cancel"), { slug });
      if (mount.aborted) return;
      setCancelled(true);
      setConfirming(false);
      end("Task cancelled");
    } catch (error) {
      if (mount.aborted) return;
      end(said(error));
    }
  };
  return (
    <div className="task-controls row">
      <label className="inline-control" htmlFor="task-priority">
        {"Priority "}
        <SelectField
          id="task-priority"
          label="Priority"
          value={selected}
          disabled={busy || stale}
          onChange={(value) => void choose(value)}
          options={PRIORITIES}
        />
      </label>
      {!terminal && !confirming && (
        <Button type="button" disabled={busy || stale} onClick={() => setConfirming(true)}>
          {stale ? CHECKING : "Cancel task"}
        </Button>
      )}
      {!terminal && confirming && (
        // biome-ignore lint/a11y/useSemanticElements: a row of the head's controls, as the Dioxus page drew it, not a form's fieldset
        <div className="row" role="group" aria-label="Cancel task confirmation">
          <span>Cancel this task?</span>
          <Button type="button" disabled={busy || stale} onClick={() => void cancel()}>
            {stale ? CHECKING : "Confirm cancellation"}
          </Button>
          <Button type="button" disabled={busy} onClick={() => setConfirming(false)}>
            Keep task
          </Button>
        </div>
      )}
      {result !== "" && <p role="status">{result}</p>}
    </div>
  );
}

export interface ComposerProps {
  slug: string;
  roles: Role[];
  approval: TaskOffer | null;
}

/** Whom a message may go to: everyone on the task, a role, or a holder of one. */
export function recipients(roles: readonly Role[]): [string, string][] {
  return [
    ["", "everyone on it"],
    ...["@executor", "@reviewer", "@orchestrator"].map((role): [string, string] => [role, role]),
    ...roles.map((role): [string, string] => [role.handle, role.handle]),
  ];
}

/**
 * The thread's composer: a draft kept on this device as it is typed, its files, whom it goes to,
 * and the landing the task offers. A sent draft is cleared only on success, and only if the owner
 * has not edited it while the post ran. The message and the landing wait while the page may show
 * what is out of date; the draft stays editable.
 */
export function Composer({ slug, roles, approval }: ComposerProps) {
  const mount = useMount();
  const draftKey = `task-draft:${slug}`;
  const toKey = `task-to:${slug}`;
  const [draft, setDraft] = useState(() => load(draftKey) ?? "");
  const typedNow = useRef(draft);
  typedNow.current = draft;
  useEffect(() => save(draftKey, draft), [draftKey, draft]);
  const [recipient, setRecipient] = useState(() => load(toKey) ?? "");
  const [busy, setBusy] = useState(false);
  const working = useRef(false);
  const [result, setResult] = useState("");
  const attach = useAttach(`task-draft:${slug}`);
  const stale = useStore(STALE);
  const send = async () => {
    if (
      working.current ||
      STALE.get() ||
      attach.busy ||
      (typedNow.current.trim() === "" && attach.empty)
    ) {
      return;
    }
    working.current = true;
    setBusy(true);
    const typed = typedNow.current;
    const text = attach.compose(typed);
    let told: string;
    try {
      await post(url("/api/task-message"), { slug, text, to: recipient });
      if (mount.aborted) return;
      // The files went with it; no new one can arrive while it is sent.
      attach.clear();
      if (typedNow.current === typed) {
        save(draftKey, "");
        setDraft("");
        told = "Sent";
      } else told = "";
    } catch (error) {
      if (mount.aborted) return;
      told = said(error);
    }
    setResult(told);
    working.current = false;
    setBusy(false);
  };
  const land = async (offer: TaskOffer, payload: string) => {
    if (working.current || STALE.get()) return;
    working.current = true;
    setBusy(true);
    let told: string;
    try {
      const done = await post(url(TASK_ACTION_ROUTE), {
        slug,
        action: offer.action,
        payload,
      });
      if (mount.aborted) return;
      const words = field(done, "said");
      told = typeof words === "string" ? words : "";
    } catch (error) {
      if (mount.aborted) return;
      told = said(error);
    }
    setResult(told);
    working.current = false;
    setBusy(false);
  };
  const payload = approval?.payload ?? null;
  return (
    <form
      id="task-send"
      className="verdict"
      data-posting={busy}
      data-attach={attach.zone}
      onSubmit={(event) => {
        event.preventDefault();
        void send();
      }}
    >
      <label htmlFor="task-message">Message</label>
      <Textarea
        id="task-message"
        value={draft}
        placeholder="Message (⌘/Ctrl+Enter sends)"
        onChange={(event) => {
          const text = event.target.value;
          save(draftKey, text);
          setDraft(text);
          if (result === "Sent") setResult("");
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
            event.preventDefault();
            void send();
          }
        }}
      />
      <AttachControls attach={attach} disabled={busy} />
      <div className="row">
        <label className="inline-control" htmlFor="task-recipient">
          {"to "}
          <SelectField
            id="task-recipient"
            label="Message recipient"
            value={recipient}
            onChange={(value) => {
              save(toKey, value);
              setRecipient(value);
            }}
            options={recipients(roles)}
          />
        </label>
        <Button type="submit" disabled={busy || attach.busy || stale}>
          {stale ? CHECKING : "Send"}
        </Button>
        {approval !== null && payload !== null && (
          <>
            <code className="approval-branch">{approval.branch}</code>
            <Button
              type="button"
              disabled={busy || stale}
              onClick={() => void land(approval, payload)}
            >
              {stale ? CHECKING : `✓ ${approval.label}`}
            </Button>
          </>
        )}
      </div>
      <p role="status">{result}</p>
    </form>
  );
}

/** What the owner has read of a long brief past the part its page sent, for one revision of the
 * brief: the chunks, the byte to read from next, the bytes still left and the last failure. */
export interface BriefRead {
  revision: string;
  blocks: Block[];
  next: number;
  omitted: number;
  failed: string;
}

export const NOTHING_READ: BriefRead = {
  revision: "",
  blocks: [],
  next: 0,
  omitted: 0,
  failed: "",
};

/** What was read of the brief at `revision`; none when what is held was read of another revision,
 * which the brief's page no longer shows. */
export function briefOf(read: BriefRead, revision: string): BriefRead | null {
  return read.revision === revision ? read : null;
}

/**
 * `answer` (a chunk, or why it failed), asked of the brief at `revision` (whose page stopped at
 * `next` with `omitted` left), added after what was read of it. A chunk cut from another revision is
 * not added: the brief changed after its page was sent, and the page reloads with the new one.
 */
export function briefAdd(
  read: BriefRead,
  revision: string,
  next: number,
  omitted: number,
  answer: TaskBrief | string,
): BriefRead {
  const held = briefOf(read, revision) ?? { ...NOTHING_READ, revision, next, omitted };
  if (typeof answer === "string") return { ...held, failed: answer };
  if (answer.revision !== revision) {
    return {
      ...held,
      failed: "The brief changed since this page was sent: reload to read the new one.",
    };
  }
  return {
    revision,
    blocks: [...held.blocks, ...answer.blocks],
    next: answer.next,
    omitted: answer.omitted,
    failed: "",
  };
}

export interface BriefProps {
  slug: string;
  revision: string;
  blocks: Block[];
  omitted: number;
  next: number;
}

/**
 * The brief, which the page sends cut when it is very long; a button reads the rest, a chunk at a
 * time, after the part already shown. What was read belongs to one revision of the brief: an edit
 * starts again from the part the new page sent.
 */
export function Brief({ slug, revision, blocks, omitted, next }: BriefProps) {
  const mount = useMount();
  const [read, setRead] = useState(NOTHING_READ);
  const [busy, setBusy] = useState(false);
  const held = briefOf(read, revision);
  const more = held?.blocks ?? null;
  const from = held?.next ?? next;
  const remaining = held?.omitted ?? omitted;
  const failedSaid = held?.failed ?? "";
  const drawn = useMemo(() => (more === null ? blocks : [...blocks, ...more]), [blocks, more]);
  const loadMore = async () => {
    if (busy) return;
    setBusy(true);
    let answer: TaskBrief | string;
    try {
      const path = `/api/task-brief?slug=${encodeURIComponent(slug)}&from=${from}`;
      answer = readBrief(await getJson(url(path), mount)) ?? "The brief's part is not readable";
    } catch (error) {
      answer = said(error);
    }
    if (mount.aborted) return;
    setRead((before) => briefAdd(before, revision, next, omitted, answer));
    setBusy(false);
  };
  return (
    <div className="task-body card">
      <BlocksView blocks={drawn} />
      {remaining > 0 && (
        <>
          <p className="brief-cut">
            {`${remaining} bytes of the brief are not shown yet. `}
            <Button id="brief-more" disabled={busy} onClick={() => void loadMore()}>
              Show more of the brief
            </Button>
          </p>
          {failedSaid !== "" && <p role="alert">{failedSaid}</p>}
        </>
      )}
    </div>
  );
}

/** Whose a task is, its line's chunk once it has come: a task drawn after draws it at once. */
let ownerLine: ComponentType<OwnerLineProps> | null = null;

/** Import whose a task is, its line's chunk, kept once it has come. */
export const loadOwnerLine = () =>
  import("./owner-part").then((chunk) => {
    ownerLine = chunk.OwnerLine;
    return ownerLine;
  });

/**
 * Whose a task is and whom it may be handed to, a chunk of its own (owner.tsx's line, which a
 * document's cards draw too), imported for a task whose board names its owner alone (chunk.tsx).
 * Until it has come, its place is held: a bar for its words and, for one who may hand it on, the
 * select's box.
 */
export function OwnerPart(props: OwnerLineProps) {
  const [Line, failed, retry] = useChunk(loadOwnerLine, () => ownerLine);
  if (Line) return <Line {...props} />;
  if (failed) return chunkFailed("The task's ownership", retry);
  return (
    <span className="row">
      <span className="bone" style={bone("24ch")} />
      {(props.owner.hand_to ?? []).length > 0 && (
        <span className="ghost-field" style={{ width: "7rem" }} />
      )}
    </span>
  );
}

/** A holder's Go to session control, its chunk once it has come: a chip drawn after draws it at once. */
let goToSession: ComponentType<GoToSessionProps> | null = null;

/** Import a holder's Go to session control, its chunk, kept once it has come. */
export const loadGoToSession = () =>
  import("./session-part").then((chunk) => {
    goToSession = chunk.GoToSession;
    return goToSession;
  });

/**
 * A role's holder's Go to session control, a chunk of its own (session.tsx's control, which the
 * agents page draws too), imported for a holder whose session the page names alone (chunk.tsx):
 * none, and nothing imported, for one it names none of.
 */
export function SessionPart(props: GoToSessionProps) {
  return props.go === null ? null : <SessionChunk {...props} />;
}

/**
 * The control of a holder whose session the page names, its place held until its chunk comes: a
 * control's box, as tall as its button, which a link in its place, where the session has an
 * address, is not.
 */
function SessionChunk(props: GoToSessionProps) {
  const [Go, failed, retry] = useChunk(loadGoToSession, () => goToSession);
  if (Go) return <Go {...props} />;
  if (failed) return chunkFailed("The session's control", retry);
  return <span className="ghost-field" style={{ width: "7rem" }} />;
}

/** A workflow's next actions, imported for a task a custom workflow holds alone. */
const nextActions = () => import("./nextactions").then((chunk) => chunk.NextActions);

/**
 * A task a custom workflow holds: its place and the owner's next actions on it, a chunk of their
 * own (nextactions.tsx), imported for such a task alone (chunk.tsx).
 */
export function WorkflowPart(props: NextActionsProps) {
  const [Part, failed, retry] = useChunk(nextActions);
  if (Part) return <Part {...props} />;
  return failed ? chunkFailed("The workflow's actions", retry) : null;
}

/** A task's attempts, imported for a task with some alone. */
const attempts = () => import("./attempts").then((chunk) => chunk.Attempts);

/**
 * A task's execution attempts, a chunk of their own (attempts.tsx), imported for a task with some
 * alone (chunk.tsx).
 */
export function AttemptsPart(props: AttemptsProps) {
  const [Part, failed, retry] = useChunk(attempts);
  if (Part) return <Part {...props} />;
  return failed ? chunkFailed("The task's attempts", retry) : null;
}

export interface TaskHeadProps {
  page: Page;
  /** The checkout the route names (`""`, the main one), whose sessions the role chips go to. */
  scope: string;
  /** Whether the feed is open and nothing shown came from this device: the machines' own work is
   * read as disconnected otherwise. */
  connected: boolean;
}

/** A task's head card: its slug, title, state, who filed it, the owner's controls, its facts, its
 * roles and its builds and tests. */
export function TaskHead({ page, scope, connected }: TaskHeadProps) {
  const { task } = page;
  const { slug } = task;
  return (
    <div className="task-head card">
      <p className="eyebrow">
        <code>{slug}</code>
      </p>
      <h1>{task.title}</h1>
      {/* The state, who filed it, then the owner's controls apart from them: the priority is said
          once, by its own control. */}
      <div className="task-state row">
        <span className={`state pill st-${task.status}`}>{statusWords(task.status)}</span>
        <span className="by">{`filed by ${task.created_by}`}</span>
        <OwnerControls slug={slug} priority={task.priority} status={task.status} />
      </div>
      {task.workflow && (
        <WorkflowPart scope={scope} slug={slug} place={task.workflow} updated={task.updated} />
      )}
      <ul className="facts">
        {taskKind(task.fields) !== null && (
          <li className="kind">{`kind: ${taskKind(task.fields)}`}</li>
        )}
        {page.owner && (
          <li>
            <OwnerPart
              owner={page.owner}
              id="task-hand"
              hand={(to) => post(url(TASK_HAND_ROUTE), { slug, to })}
            />
          </li>
        )}
        {task.parent !== null && (
          <li>
            {"part of "}
            <a href={href(`/task/${task.parent}`)}>{task.parent}</a>
          </li>
        )}
        {task.depends.map((dependency) => (
          <li key={`after ${dependency}`}>
            {"after "}
            <a href={href(`/task/${dependency}`)}>{dependency}</a>
          </li>
        ))}
        {task.links.map((link) => (
          <li key={`implements ${link}`}>
            {"implements "}
            <a href={href(`/${link}`)}>{link}</a>
          </li>
        ))}
      </ul>
      <RoleChips roles={task.roles} sessions={page.sessions} local={page.local} scope={scope} />
      <WorkPhases work={page.machine_work} slug={slug} connected={connected} />
    </div>
  );
}

export interface TaskDetailProps {
  /** The route's task. */
  slug: string;
  /** The checkout the route names (`""`, the main one). */
  scope: string;
  /** The task's page, once its read or this device gave it: its head and its brief are their
   * mock-ups before. */
  page: Page | null;
  /** Why the page's read failed, while there is none to draw: said in the head's place. */
  pageFailed: string;
  connected: boolean;
  /** The thread as its part stands. */
  thread: Shown<Thread>;
  /** Whether the page says a failure: the thread waits under its ghost no longer. */
  failed: boolean;
  onRetry: () => void;
  onHistory: (older: boolean) => undefined | Promise<string | null>;
  onChildren: (more: boolean) => void;
}

/** A task's page, each part as its own read stands: its data, or its mock-up until that comes. */
export function TaskDetail({
  slug,
  scope,
  page,
  pageFailed,
  connected,
  thread,
  failed,
  onRetry,
  onHistory,
  onChildren,
}: TaskDetailProps) {
  const offer = usePart(offerPart(scope, slug)).value;
  const approval = offer !== undefined && offer.payload !== null ? offer : null;
  // The heights this browser last drew the head and the brief at, for their mock-ups: the task's
  // own in a window as wide, else those of the last task it drew there.
  const [heights] = useState(() => ({
    head: drawnHeight(`head ${slug}`),
    title: drawnHeight(`title ${slug}`),
    facts: drawnHeight(`facts ${slug}`),
    brief: drawnHeight(`brief ${slug}`),
  }));
  const drawn = page !== null;
  // The page says once that the task loads: its parts' own mock-ups do not say theirs beside it.
  const waiting = !drawn && pageFailed === "";
  // The head's and the brief's heights, for their mock-ups on the next visit.
  useEffect(() => {
    if (!drawn) return;
    const controller = new AbortController();
    const at = `article.task[data-slug="${slug}"] > `;
    void rememberDrawn(
      [
        [`head ${slug}`, `${at}.task-head`],
        [`title ${slug}`, `${at}.task-head h1`],
        [`facts ${slug}`, `${at}.task-head .facts`],
        [`brief ${slug}`, `${at}.task-body`],
      ],
      100,
      controller.signal,
    );
    return () => controller.abort();
  }, [slug, drawn]);
  const entries = thread.value?.entries ?? [];
  const loaded = thread.value !== undefined;
  const older = (thread.value?.before ?? null) !== null;
  const tasks = page?.children ?? [];
  return (
    <article className="task" data-slug={slug} aria-busy={drawn ? undefined : "true"}>
      {page !== null ? (
        <TaskHead page={page} scope={scope} connected={connected} />
      ) : pageFailed !== "" ? (
        <Trouble
          title="The task could not be loaded"
          text={failedRead(pageFailed, true)}
          retry={onRetry}
        />
      ) : (
        <>
          <span className="status-text" role="status">
            Loading task…
          </span>
          <GhostHead height={heights.head} title={heights.title} facts={heights.facts} />
        </>
      )}
      <TaskGatesPanel
        slug={slug}
        scope={scope}
        task={page?.task ?? null}
        seen={page?.gates ?? null}
        quiet={waiting}
      />
      {page !== null && page.children_count > 0 && (
        <NearViewport id={`subtasks-${slug}`} height={60} onEnter={() => onChildren(false)}>
          <h2>
            {"Subtasks "}
            <span className="count">{page.children_count}</span>
          </h2>
          <ul>
            {tasks.map((child) => (
              <NearViewport key={child.slug} listItem id={`child-${child.slug}`} height={32}>
                <a href={href(`/task/${child.slug}`)}>{child.title}</a>
                {` · ${statusWords(child.status)}`}
              </NearViewport>
            ))}
          </ul>
          {tasks.length === 0 ? (
            <Button onClick={() => onChildren(false)}>Load subtasks</Button>
          ) : tasks.length < page.children_count ? (
            <Button onClick={() => onChildren(true)}>Load more subtasks</Button>
          ) : null}
        </NearViewport>
      )}
      <TimelineSlot
        key={slug}
        slug={slug}
        scope={scope}
        thread={{
          entries,
          before: thread.value?.before ?? null,
          pending: (!loaded && thread.error === null) || thread.retrying,
          error: thread.error === null ? null : said(thread.error),
          earlier: async () => (await onHistory(true)) ?? null,
          latest: async () => (await onHistory(false)) ?? null,
          render: (entry) => <TaskMessage entry={entry} />,
        }}
      />
      {page !== null ? (
        <Brief
          slug={slug}
          revision={page.brief_revision}
          blocks={page.blocks}
          omitted={page.brief_omitted}
          next={page.brief_next}
        />
      ) : (
        pageFailed === "" && <GhostBrief height={heights.brief} />
      )}
      <NearViewport key={`diff-${slug}`} id={`changes-${slug}`} height={160}>
        <TaskDiffPanel scope={scope} slug={slug} />
      </NearViewport>
      {!!page?.attempts && (
        <NearViewport key={`attempts-${slug}`} id={`attempts-${slug}`} height={120}>
          <AttemptsPart
            scope={scope}
            slug={slug}
            count={page.attempts}
            updated={page.task.updated}
          />
        </NearViewport>
      )}
      <h2>Thread</h2>
      {/* The thread and the composer under it wait under their ghost until the thread on the first
          screen is in, whatever the head and the brief do meanwhile. */}
      <Settle failed={failed} cover={<GhostThread quiet={waiting} />}>
        <NearViewport id={`thread-region-${slug}`} height={200}>
          <div id="task-thread" data-pending={`${!loaded}`}>
            {older && <Button onClick={() => onHistory(true)}>Load earlier messages</Button>}
            {/* Offered once its read failed with nothing drawn (the store tries it again too), and
                never asked while a read is out: the first is the ghost's to say, a later the
                button's. */}
            {!loaded && thread.error !== null && (
              <Button disabled={thread.retrying} onClick={() => onHistory(false)}>
                {thread.retrying ? "Loading messages…" : "Load messages"}
              </Button>
            )}
            {/* One timeline: every message is an item of the one list its line runs down. */}
            <ul className="history">
              {entries.map((entry) => (
                <NearViewport
                  key={entry.message.id}
                  listItem
                  id={`message-${entry.message.id}`}
                  height={100}
                  ghost={<GhostEntry />}
                >
                  <TaskMessage entry={entry} />
                </NearViewport>
              ))}
            </ul>
            {!loaded && (
              <div className="ghost-history">
                <GhostEntry />
                <GhostEntry />
              </div>
            )}
          </div>
        </NearViewport>
        <NearViewport id={`composer-${slug}`} height={200}>
          {page !== null ? (
            <Composer slug={slug} roles={page.task.roles} approval={approval} />
          ) : (
            <GhostComposer />
          )}
        </NearViewport>
      </Settle>
    </article>
  );
}

/** The route's task as `live` stands: its parts, each its data or its mock-up, or that the board
 * has no such task. */
export function LiveTaskPage({ live, connected }: { live: LiveTask; connected: boolean }) {
  const { slug } = live;
  const page = live.page();
  const missing = live.missing();
  const pageError = live.parts.shown(live.pagePart).error;
  const pageFailed = page === null && pageError !== null ? said(pageError) : "";
  return (
    <div
      className="task-page"
      data-ready={page !== null ? "task" : ready(missing, pageFailed !== "")}
    >
      {missing ? (
        <Trouble
          title={`No task ${slug}`}
          text="The board has no task by this name."
          back={["/tasks", "All tasks"]}
          retry={() => live.again()}
        />
      ) : (
        <PartsContext.Provider value={live.parts}>
          <TaskDetail
            key={slug}
            slug={slug}
            scope={live.scope}
            page={page}
            pageFailed={pageFailed}
            connected={connected}
            thread={live.thread()}
            failed={pageFailed !== "" || live.error() !== ""}
            onRetry={() => live.again()}
            onHistory={(more) => live.history(more)}
            onChildren={(more) => void live.children(more)}
          />
        </PartsContext.Provider>
      )}
    </div>
  );
}

export interface TaskPageViewProps {
  /** The task kept live, once the page started it. */
  live: LiveTask | null;
  stale: boolean;
  freshness: Freshness;
}

/**
 * A task's page under the board's notices: its parts from the moment the page starts, each behind
 * its own read alone, the board's among them; why the board could not be read, only while no part
 * of the task is drawn.
 */
export function TaskPageView({ live, stale, freshness }: TaskPageViewProps) {
  const device = shown(freshness);
  const drawn = live?.drawn() ?? false;
  const onPage = drawn || (live?.boardShown ?? false);
  const trouble = live?.trouble ?? null;
  useEffect(() => {
    if (live?.boardShown) performance.mark(READY_MARK);
  }, [live?.boardShown]);
  // A task with nothing yet to show says in its head's place what is missing or failed; once
  // drawn, the page's error is drawn in its notice stack, out of the page's flow.
  const error = drawn && live !== null ? live.error() : "";
  return (
    <main
      data-ready={onPage ? "board" : ready(false, trouble !== null)}
      data-feed={stale || device !== null ? "stale" : "live"}
      data-cache={device !== null ? "device" : undefined}
    >
      {live === null ? (
        <Skeleton lines={8} label="Loading the board…" />
      ) : trouble !== null && !onPage ? (
        <Trouble
          title="The board could not be loaded"
          text={failedRead(trouble, true)}
          retry={() => {
            live.retry.current += 1;
          }}
        />
      ) : (
        // What the device drew is not the machines' work now: shown as out of touch until read.
        <LiveTaskPage live={live} connected={!stale && device === null} />
      )}
      <PageNotices reconnecting={stale && onPage} error={error} device={device} composer />
    </main>
  );
}

/** The route's task, kept live: one live task for the page's checkout, which follows the route
 * from task to task within it. */
export function TaskPage({ scope, slug }: { scope: string; slug: string }) {
  const frame = useFrame();
  const [, redraw] = useReducer((count: number) => count + 1, 0);
  const [stale, setStale] = useState(false);
  const [freshness, setFreshness] = useState<Freshness>(FRESH);
  const live = useRef<LiveTask | null>(null);
  const route = useRef(slug);
  route.current = slug;
  useEffect(() => {
    const controller = new AbortController();
    const task = new LiveTask(
      scope,
      route.current,
      { redraw, freshness: setFreshness, stale: setStale },
      controller.signal,
    );
    live.current = task;
    setFreshness(FRESH);
    redraw();
    void task.start(frame);
    return () => controller.abort();
  }, [scope, frame]);
  useEffect(() => {
    live.current?.moved(slug);
  }, [slug]);
  return <TaskPageView live={live.current} stale={stale} freshness={freshness} />;
}
