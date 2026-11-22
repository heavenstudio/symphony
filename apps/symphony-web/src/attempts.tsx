// A task's execution attempts on its page (docs/engineering/EXECUTION_ATTEMPTS.md, "On a task's page"): each
// attempt the board kept for it, newest first, past ones and what was bound to them included after
// their handle left (`/api/task-attempts`), and what may be asked of each here: a cancel, a stop in
// one of the modes its provider pinned, a resume of one paused or ended with something to resume
// from, and a view of its resource. A control posts the attempt's revision as the page read it
// (`/api/attempt-control`), refused as stale once the attempt moved, and draws the history it
// answers. A view is resolved on demand (`/api/attempt-view`) into a window opened as it is asked,
// and its address is kept nowhere: not in the page's state, on the device or in a log. An attempt
// runs on its provider and has no pane: a session's focus and screen stay its handle's local
// session's (session.tsx). A chunk of its own, which the page imports for a task with attempts
// alone; its style (attempts.css) it puts in the page.

import { useEffect, useRef, useState } from "react";
import STYLE from "./attempts.css?inline";
import { Button } from "./button";
import { useMount } from "./mount";
import { scoped } from "./project";
import { getJson, post, said } from "./request";
import { field, STALE, useStore } from "./shared";
import {
  ATTEMPT_CONTROL_ROUTE,
  ATTEMPT_VIEW_ROUTE,
  type AttemptControl,
  type AttemptControls,
  type AttemptView,
  TASK_ATTEMPTS_PATH,
} from "./wire";
import { CHECKING } from "./words";

const isText = (value: unknown): value is string => typeof value === "string";
/** As serde reads an `Option`: absent and `null` are none. */
const absent = (value: unknown) => value === undefined || value === null;
/** A whole number as serde reads an `i64`. */
const isWhole = (value: unknown): value is number =>
  typeof value === "number" && Number.isSafeInteger(value);
/** A `Vec<String>` serde defaults when absent: none for anything else. */
function texts(value: unknown): string[] | null {
  if (value === undefined) return [];
  return Array.isArray(value) && value.every(isText) ? value : null;
}

/** What may be asked of an attempt read with none: nothing. */
export const NONE: AttemptControls = {
  cancel: false,
  stop: [],
  resume: [],
  view: false,
  unserved: null,
};

/** An attempt's controls as serde reads them, or none: absent, they are NONE. */
export function readControls(value: unknown): AttemptControls | null {
  if (value === undefined) return NONE;
  const stop = texts(field(value, "stop"));
  const resume = texts(field(value, "resume"));
  // A `bool` serde defaults when absent, and refuses as `null`.
  const said = field(value, "view");
  const view = said === undefined ? false : said;
  const unserved = field(value, "unserved");
  return typeof field(value, "cancel") === "boolean" &&
    stop !== null &&
    resume !== null &&
    typeof view === "boolean" &&
    (absent(unserved) || isText(unserved))
    ? {
        cancel: field(value, "cancel") as boolean,
        stop,
        resume,
        view,
        unserved: (unserved as string | undefined) ?? null,
      }
    : null;
}

/** An attempt's words, each text. */
const WORDS = [
  "id",
  "lane",
  "role",
  "configuration",
  "dispatch",
  "cancellation",
  "collection",
  "cleanup",
  "initiator",
] as const;
/** What an attempt may have, each text or none. */
const MAYBE = [
  "handle",
  "predecessor",
  "relation",
  "successor",
  "ending",
  "held",
  "resource",
  "generation",
  "conversation",
] as const;

/** An attempt as serde reads it, or none: a word a newer server adds is kept as it is said. */
export function readAttempt(value: unknown): AttemptView | null {
  const controls = readControls(field(value, "controls"));
  const [created, revision, current] = ["created", "revision", "current"].map((key) =>
    field(value, key),
  );
  if (
    controls === null ||
    !isWhole(created) ||
    !isWhole(revision) ||
    typeof current !== "boolean" ||
    !WORDS.every((key) => isText(field(value, key))) ||
    !MAYBE.every((key) => absent(field(value, key)) || isText(field(value, key)))
  ) {
    return null;
  }
  const read = { created, revision, current, controls } as AttemptView;
  for (const key of WORDS) read[key] = field(value, key) as string;
  for (const key of MAYBE) read[key] = (field(value, key) as string | undefined) ?? null;
  return read;
}

/** A task's attempts as serde reads them, or none when one of them is not read. */
export function readAttempts(value: unknown): AttemptView[] | null {
  const attempts = field(value, "attempts");
  if (!Array.isArray(attempts)) return null;
  const read = attempts.map(readAttempt);
  return read.every((attempt) => attempt !== null) ? read : null;
}

/** A view's address, when it is one a window may be sent to: http or https alone, so an address
 * the provider answered never runs as script in the page's own origin. */
export function viewAddress(value: unknown): string | null {
  const url = field(value, "url");
  if (!isText(url) || !isWhole(field(value, "valid_for_ms"))) return null;
  try {
    return ["https:", "http:"].includes(new URL(url).protocol) ? url : null;
  } catch {
    return null;
  }
}

/** A board word as said: its underscores spaces (`hard_stop`, "hard stop"). */
export function words(word: string): string {
  return word.replaceAll("_", " ");
}

/** An attempt's id as the page names it: its first eight digits. */
export function short(id: string): string {
  return id.slice(0, 8);
}

/** When an attempt was admitted, in UTC to the minute. */
export function when(ms: number): string {
  const date = new Date(ms);
  return Number.isNaN(date.getTime())
    ? ""
    : `${date.toISOString().slice(0, 16).replace("T", " ")} UTC`;
}

/** Where an attempt stands: its dispatch, how it ended, a cancel asked or not confirmed. */
export function stateWords(attempt: AttemptView): string {
  const at = attempt.dispatch === "terminal" ? (attempt.ending ?? "ended") : attempt.dispatch;
  const cancel = attempt.cancellation === "none" ? "" : `, cancel ${attempt.cancellation}`;
  return `${words(at)}${cancel}`;
}

/** What an attempt follows and what followed it. */
export function lineage(attempt: AttemptView): string {
  const follows = attempt.predecessor
    ? `${words(attempt.relation ?? "follows")} of ${short(attempt.predecessor)}`
    : "";
  const followed = attempt.successor ? `followed by ${short(attempt.successor)}` : "";
  return [follows, followed].filter((said) => said !== "").join(" · ");
}

/** What was bound to an attempt, as references: its resource and generation and its
 * conversation, never an address. */
export function bound(attempt: AttemptView): string {
  const resource = attempt.resource
    ? `resource ${attempt.resource}${attempt.generation ? ` (${attempt.generation})` : ""}`
    : "";
  const conversation = attempt.conversation ? `conversation ${attempt.conversation}` : "";
  return [resource, conversation].filter((said) => said !== "").join(" · ");
}

/** A control asked of an attempt: its act, and its mode where it takes one. */
export interface Asked {
  act: "cancel" | "stop" | "resume";
  mode: string | null;
}

/** How a control is said on its button and in its confirmation. */
export function askedWords({ act, mode }: Asked): string {
  const said = { cancel: "Cancel", stop: "Stop", resume: "Resume" }[act];
  return mode === null ? said : `${said}: ${words(mode)}`;
}

/** A request key: 128 random bits in hex, which a page served over plain HTTP has too. */
function newKey(): string {
  return Array.from(crypto.getRandomValues(new Uint8Array(16)), (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
}

/** The post of `asked` on `attempt` of `slug`: the attempt's revision as read, and a resume's
 * request key, `key`. Who asks is never a field: the server knows. */
export function controlOf(
  slug: string,
  attempt: AttemptView,
  asked: Asked,
  key: string,
): AttemptControl {
  return {
    slug,
    attempt: attempt.id,
    revision: attempt.revision,
    act: asked.act,
    mode: asked.mode,
    key: asked.act === "resume" ? key : null,
  };
}

/** Put the chunk's style in the page, once. */
function putStyle(): void {
  if (document.getElementById("attempts-style")) return;
  const element = document.createElement("style");
  element.id = "attempts-style";
  element.textContent = STYLE;
  document.head.insertBefore(element, document.head.querySelector("[data-symphony-theme]"));
}

/**
 * A view of `attempt`'s resource, resolved now and sent to `opened`, the window the click opened
 * (none where the browser refused one): what the page says of it, empty once the window has it.
 * The address goes to the window alone; a window that cannot have it is closed, and with none the
 * provider is asked for nothing.
 */
export async function view(
  scope: string,
  slug: string,
  attempt: AttemptView,
  opened: Window | null,
): Promise<string> {
  if (opened === null) return "No view: the browser opened no window; allow this page's pop-ups";
  try {
    const answer = await post(scoped(scope, ATTEMPT_VIEW_ROUTE), { slug, attempt: attempt.id });
    const address = viewAddress(answer);
    if (address === null) throw new Error("the server's answer is no address to open");
    opened.location.replace(address);
    return "";
  } catch (error) {
    opened.close();
    return `No view of ${short(attempt.id)}: ${said(error)}`;
  }
}

/** What a read of the history came to: the attempts (none where it was not read) and, when it was
 * not, why. */
export interface Read {
  attempts: AttemptView[] | null;
  refused: string;
}

/** `slug`'s attempts under `scope`; none once `signal` ends. */
export async function readHistory(
  scope: string,
  slug: string,
  signal: AbortSignal,
): Promise<Read | null> {
  const path = `${TASK_ATTEMPTS_PATH}?slug=${encodeURIComponent(slug)}`;
  try {
    const attempts = readAttempts(await getJson(scoped(scope, path), signal));
    if (signal.aborted) return null;
    return { attempts, refused: attempts === null ? "the server's answer could not be read" : "" };
  } catch (error) {
    return signal.aborted ? null : { attempts: null, refused: said(error) };
  }
}

/** What a control came to: the history it answered (none where it was refused, or its answer is
 * unread, the history then read again) and what the page says of it. */
export interface Controlled {
  attempts: AttemptView[] | null;
  said: string;
}

/**
 * Post `asked` on `attempt` of `slug` under `scope`. A resume goes with the key `keys` keeps for
 * the attempt at the revision read, so one sent again after no answer is the same request.
 */
export async function control(
  scope: string,
  slug: string,
  attempt: AttemptView,
  asked: Asked,
  keys: Map<string, string>,
): Promise<Controlled> {
  const at = `${attempt.id}@${attempt.revision}`;
  const key = keys.get(at) ?? newKey();
  keys.set(at, key);
  const what = `${askedWords(asked)} of ${short(attempt.id)}`;
  try {
    const answer = await post(
      scoped(scope, ATTEMPT_CONTROL_ROUTE),
      controlOf(slug, attempt, asked, key),
    );
    return { attempts: readAttempts(answer), said: `${what}: asked` };
  } catch (error) {
    return { attempts: null, said: `${what} was not taken: ${said(error)}` };
  }
}

export interface AttemptsProps {
  /** The checkout the route names (`""`, the main one). */
  scope: string;
  slug: string;
  /** How many attempts the task's page counts: read again when it changes. */
  count: number;
  /** The task's last change: read again after each. */
  updated: number;
}

/** A task's attempts, read as the part mounts, again on each change of the task's page, on Refresh
 * and after a control whose answer it could not draw, and else from each control's answer. */
export function Attempts({ scope, slug, count, updated }: AttemptsProps) {
  const mount = useMount();
  const checking = useStore(STALE);
  const [read, setRead] = useState<Read>({ attempts: null, refused: "" });
  const [round, setRound] = useState(0);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState("");
  const [confirming, setConfirming] = useState<{ id: string; asked: Asked } | null>(null);
  const keys = useRef(new Map<string, string>());
  // biome-ignore lint/correctness/useExhaustiveDependencies: read again on each change of the task's page and each refresh asked, whose values the read does not use
  useEffect(() => {
    putStyle();
    const reading = new AbortController();
    void readHistory(scope, slug, reading.signal).then(
      (answer) =>
        answer &&
        setRead((was) => (answer.attempts === null ? { ...was, refused: answer.refused } : answer)),
    );
    return () => reading.abort();
  }, [scope, slug, count, updated, round]);
  const ask = async (attempt: AttemptView, asked: Asked) => {
    if (busy || STALE.get()) return;
    setBusy(true);
    setResult("");
    const done = await control(scope, slug, attempt, asked, keys.current);
    if (mount.aborted) return;
    const { attempts } = done;
    if (attempts === null) setRound((was) => was + 1);
    else setRead({ attempts, refused: "" });
    setResult(done.said);
    setConfirming(null);
    setBusy(false);
  };
  const open = (attempt: AttemptView) => {
    // Opened within the click, as a browser lets a page open a window, and told nothing of the
    // page; its address comes once the server has resolved it.
    const opened = window.open("", "_blank");
    if (opened !== null) opened.opener = null;
    setResult("");
    void view(scope, slug, attempt, opened).then((said) => mount.aborted || setResult(said));
  };
  return (
    <AttemptsView
      attempts={read.attempts}
      refused={read.refused}
      busy={busy}
      checking={checking}
      result={result}
      confirming={confirming}
      onAsk={(attempt, asked) => setConfirming({ id: attempt.id, asked })}
      onConfirm={(attempt, asked) => void ask(attempt, asked)}
      onKeep={() => setConfirming(null)}
      onView={open}
      onRefresh={() => setRound((was) => was + 1)}
    />
  );
}

export interface AttemptsViewProps {
  /** The attempts read, oldest first; none before the first read answers. */
  attempts: AttemptView[] | null;
  /** Why the last read was not drawn; empty for none. */
  refused: string;
  /** Whether a control's post is out. */
  busy: boolean;
  /** Whether the page may show what is out of date: the controls wait meanwhile. */
  checking: boolean;
  /** What the last control or view came to; empty for none. */
  result: string;
  /** The control waiting on its confirmation, and its attempt's id. */
  confirming: { id: string; asked: Asked } | null;
  onAsk: (attempt: AttemptView, asked: Asked) => void;
  onConfirm: (attempt: AttemptView, asked: Asked) => void;
  onKeep: () => void;
  onView: (attempt: AttemptView) => void;
  onRefresh: () => void;
}

/** A task's attempts as drawn: newest first, each its state, lineage, references and controls. */
export function AttemptsView(props: AttemptsViewProps) {
  const { attempts, refused, result } = props;
  return (
    <section className="attempts" aria-labelledby="attempts-title" aria-busy={attempts === null}>
      <div className="row attempts-head">
        <h2 id="attempts-title">
          {"Attempts "}
          {attempts !== null && <span className="count">{attempts.length}</span>}
        </h2>
        <Button type="button" onClick={props.onRefresh}>
          Refresh attempts
        </Button>
      </div>
      {attempts === null && refused === "" && <p className="attempts-reading">Reading…</p>}
      {refused !== "" && <p role="alert">{`The attempts could not be read: ${refused}`}</p>}
      {attempts !== null && (
        <ol className="attempts-list">
          {[...attempts].reverse().map((attempt) => (
            <AttemptRow key={attempt.id} attempt={attempt} {...props} />
          ))}
        </ol>
      )}
      {result !== "" && <p role="status">{result}</p>}
    </section>
  );
}

/** One attempt: what it is, where it stands, what it follows and holds, and its controls. */
export function AttemptRow({
  attempt,
  busy,
  checking,
  confirming,
  onAsk,
  onConfirm,
  onKeep,
  onView,
}: AttemptsViewProps & { attempt: AttemptView }) {
  const { controls } = attempt;
  const asked: Asked[] = [
    ...(controls.cancel ? [{ act: "cancel", mode: null } as const] : []),
    ...controls.stop.map((mode) => ({ act: "stop", mode }) as const),
    ...controls.resume.map((mode) => ({ act: "resume", mode }) as const),
  ];
  const waiting = confirming?.id === attempt.id ? confirming.asked : null;
  const said = [lineage(attempt), bound(attempt)].filter((line) => line !== "");
  return (
    <li className="attempt card" data-attempt={attempt.id} data-current={attempt.current}>
      <p className="attempt-head">
        <code>{short(attempt.id)}</code>
        <span className={`pill attempt-state at-${attempt.dispatch}`}>{stateWords(attempt)}</span>
        {attempt.current && <span className="attempt-holds">holds its lane</span>}
      </p>
      <p className="attempt-meta">
        {`${attempt.role}${attempt.handle ? ` ${attempt.handle}` : ""} on ${attempt.configuration}`}
        {` · ${when(attempt.created)} · asked by ${attempt.initiator}`}
      </p>
      {said.length > 0 && <p className="attempt-meta">{said.join(" · ")}</p>}
      {attempt.held && <p className="attempt-meta">{`Waits on a person: ${attempt.held}`}</p>}
      {waiting === null ? (
        (asked.length > 0 || controls.view) && (
          <div className="row attempt-controls">
            {asked.map((ask) => (
              <Button
                key={askedWords(ask)}
                type="button"
                disabled={busy || checking}
                onClick={() => onAsk(attempt, ask)}
              >
                {checking ? CHECKING : askedWords(ask)}
              </Button>
            ))}
            {controls.view && (
              <Button type="button" onClick={() => onView(attempt)}>
                View
              </Button>
            )}
          </div>
        )
      ) : (
        // biome-ignore lint/a11y/useSemanticElements: a row of one attempt's confirmation, as the task's own cancel draws it, not a form's fieldset
        <div className="row attempt-controls" role="group" aria-label="Confirm the control">
          <span>{`${askedWords(waiting)} attempt ${short(attempt.id)}?`}</span>
          <Button
            type="button"
            disabled={busy || checking}
            onClick={() => onConfirm(attempt, waiting)}
          >
            {busy ? "Sending…" : checking ? CHECKING : "Confirm"}
          </Button>
          <Button type="button" disabled={busy} onClick={onKeep}>
            Keep it
          </Button>
        </div>
      )}
      {controls.unserved && <p className="attempt-meta">{controls.unserved}</p>}
    </li>
  );
}
