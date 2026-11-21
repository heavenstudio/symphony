// A task a custom workflow holds, on its page (docs/WORKFLOW.md, "The app"): where it stands, by
// its phase's and state's labels, with the place a block resumes at and the person a recovery by
// hand waits on, and the local owner's next actions on it as the server offers them
// (`/api/workflow-actions`): each its edge's label and destination, and one unavailable with why
// and whom it needs. An action posts its edge's ID with the snapshot it was offered under
// (`/api/workflow-apply`) and a key kept until the server has answered, so a post sent again after
// no answer is the same request, which the board takes once; a refusal that the task or its
// workflow moved since prompts a refresh of the actions. Where the workflow declares guards, the
// task's are read beside its actions (`/api/workflow-guards`) and drawn by guards.tsx: under each
// action a guard holds, the guards it needs, and above the actions those that let the task in. An
// action its guards alone hold is the owner's to take when this host asks each guard's authority,
// and a refusal that names its guard is kept under that action until the task moves. A chunk of
// its own, which the page imports
// for a task a workflow holds alone, so a task the board's own writers keep loads none of it; its
// style is flow.tsx's, which it puts in the page.

import { useEffect, useRef, useState } from "react";
import { Button } from "./button";
import { name, putStyle, readDefinition } from "./flow";
import {
  asksAll,
  clock,
  entryRows,
  footWords,
  GuardRows,
  needRows,
  readGuards,
  readRefusal,
  type Tried,
} from "./guards";
import { useMount } from "./mount";
import { scoped } from "./project";
import { Failure, getJson, post, said } from "./request";
import { field, STALE, useStore } from "./shared";
import { tryAgain } from "./state";
import {
  WORKFLOW_ACTIONS_PATH,
  WORKFLOW_APPLY_ROUTE,
  WORKFLOW_GUARDS_PATH,
  WORKFLOW_PATH,
  type WorkflowAuthority,
  type WorkflowGuardRefusal,
  type WorkflowManualRecovery,
  type WorkflowOperationRequest,
  type WorkflowPair,
  type WorkflowPlace,
  type WorkflowTaskGuards,
} from "./wire";
import { CHECKING } from "./words";

const isText = (value: unknown): value is string => typeof value === "string";
const isNumber = (value: unknown): value is number => typeof value === "number";
const isPair = (value: unknown): value is WorkflowPair =>
  isText(field(value, "phase")) && isText(field(value, "state"));
const texts = (value: unknown): value is string[] => Array.isArray(value) && value.every(isText);

/** An action the server offers on the task, as the page draws and posts it. */
export interface Action {
  edge: string;
  label: string;
  destination: WorkflowPair;
  /** Whom it is open to: said beside an action that is unavailable. */
  responsible: WorkflowAuthority[];
  /** The inputs it needs, each a text the owner writes. */
  inputs: string[];
  /** Why it cannot be taken now, by refusal code; none when it can. */
  unavailable: string[];
}

/** The actions the server offers, with the snapshot they were offered under, sent back as it came. */
export interface Offer {
  snapshot: WorkflowOperationRequest["expected"];
  actions: Action[];
}

function readAuthority(value: unknown): value is WorkflowAuthority {
  const kind = field(value, "kind");
  return kind === "task_role" ? isText(field(value, "role")) : isText(field(value, "capability"));
}

function readAction(value: unknown): Action | null {
  const responsible = field(value, "responsible");
  const inputs = field(value, "required_inputs");
  const unavailable = field(value, "unavailable");
  const destination = field(value, "destination");
  const edge = field(value, "edge");
  const label = field(value, "label");
  if (
    !isText(edge) ||
    !isText(label) ||
    !isPair(destination) ||
    !Array.isArray(responsible) ||
    !responsible.every(readAuthority) ||
    !texts(inputs) ||
    !texts(unavailable)
  ) {
    return null;
  }
  return { edge, label, destination, responsible, inputs, unavailable };
}

/**
 * The next actions a read (`/api/workflow-actions`, or an apply's `result.next`) offers, or none
 * for one that is not what the page draws. The snapshot is checked as far as the page reads it
 * (where it was taken) and is otherwise sent back whole.
 */
export function readOffer(value: unknown): Offer | null {
  const snapshot = field(value, "snapshot");
  const actions = field(value, "actions");
  if (!isPair(field(snapshot, "at")) || !isNumber(field(snapshot, "task_revision"))) return null;
  if (!Array.isArray(actions)) return null;
  const read = actions.map(readAction);
  if (!read.every((action): action is Action => action !== null)) return null;
  return { snapshot: snapshot as Offer["snapshot"], actions: read };
}

/** What the board's workflow read gives the task's page: the project a request names, and the
 * labels of the phases, states and places of its definition (flow.tsx's `name`). */
export interface Board {
  project: string;
  names: Map<string, string>;
  /** Whether the definition declares a guard: a task's guards are read only where it does. */
  guarded: boolean;
}

/** The board's workflow read (`/api/workflow`) as the page reads it, or none for one it cannot. */
export function readBoard(value: unknown): Board | null {
  const project = field(value, "project");
  const definition = readDefinition(value);
  if (!isText(project) || !definition) return null;
  const names = new Map<string, string>();
  name(definition, names);
  const guards = field(field(field(value, "workflow"), "definition"), "guards");
  return { project, names, guarded: Array.isArray(guards) && guards.length > 0 };
}

/** An ID as words, where its definition names it no label. */
export function words(id: string): string {
  return id.replaceAll("_", " ");
}

/** A place by the labels `names` gives it: its own, else its phase's and state's, else its IDs. */
export function placeWords(names: Map<string, string>, pair: WorkflowPair): string {
  const { phase, state } = pair;
  return (
    names.get(`${phase}/${state}`) ??
    `${names.get(`phase:${phase}`) ?? words(phase)} · ${names.get(`state:${state}`) ?? words(state)}`
  );
}

/** Whom `authority` names: a role on the task, or a capability. */
export function whose(authority: WorkflowAuthority[]): string {
  return authority
    .map((one) =>
      one.kind === "task_role" ? `the ${one.role}` : `the ${words(one.capability)} capability`,
    )
    .join(" or ");
}

/** Why an action cannot be taken now, by its refusal code. */
const WHY: Record<string, string> = {
  stale_task: "the task moved since",
  stale_workflow: "the workflow changed since",
  stale_activation: "another workflow is active",
  stale_input: "an input changed since",
  unknown_id: "not in this workflow",
  removed_id: "no longer in this workflow",
  unauthorized: "not the owner's to take",
  dependency: "waits on what it depends on",
  capacity: "no slot is free",
  human_wait: "waits on a person",
  guard_unavailable: "waits on its guard",
  explicit_action_required: "needs an explicit action",
};

/** The refusal a preview lists an edge a guard holds by: it is given no attestation to decide on. */
const GUARD = "guard_unavailable";

/** Whether its guards are all that holds `action`. */
export function guardsAlone(action: Action): boolean {
  return action.unavailable.length > 0 && action.unavailable.every((code) => code === GUARD);
}

/**
 * Why `action` is unavailable, and whom it needs, as its line says it; empty while it is not.
 * `rows` says its guards are drawn under it, each saying its own why, so the line leaves them out.
 */
export function unavailableWords(action: Action, rows = false): string {
  const codes = action.unavailable.filter((code) => !(rows && code === GUARD));
  if (codes.length === 0) return "";
  const why = codes.map((code) => WHY[code] ?? "not supported here");
  const needs = action.responsible.length > 0 ? `: needs ${whose(action.responsible)}` : "";
  return `${[...new Set(why)].join("; ")}${needs}`;
}

/** The recovery by hand a task waits on, as its line says it. */
export function recoveryWords(manual: WorkflowManualRecovery): string {
  const exit =
    manual.trigger === "lost_holder"
      ? `${manual.holder}, its ${manual.role}, was lost`
      : `${manual.holder} left its ${words(manual.role)}`;
  return `${exit}: it waits on you to move it on`;
}

/** The values the owner wrote for each action's inputs, by edge and input. */
export type Values = Record<string, Record<string, string>>;

/** The value written for `edge`'s `input`, trimmed. */
function value(values: Values, edge: string, input: string): string {
  return (values[edge]?.[input] ?? "").trim();
}

/** Whether an input `action` needs has no value written. */
export function missing(action: Action, values: Values): boolean {
  return action.inputs.some((input) => value(values, action.edge, input) === "");
}

/** The request taking `action` on `slug`, under the offer's snapshot, but its key. */
export function requestOf(
  project: string,
  slug: string,
  offer: Offer,
  action: Action,
  values: Values,
): Omit<WorkflowOperationRequest, "idempotency_key"> {
  return {
    project,
    task: slug,
    edge: action.edge,
    expected: offer.snapshot,
    input_revisions: {},
    inputs: Object.fromEntries(
      action.inputs.map((input) => [
        input,
        { kind: "text", value: value(values, action.edge, input) },
      ]),
    ),
  };
}

/** A request's key: 128 random bits in hex, which a page served over plain HTTP has too. */
export function newKey(): string {
  return Array.from(crypto.getRandomValues(new Uint8Array(16)), (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
}

/** A request sent whose answer is not known, by its body: sent again, it keeps its key. */
export interface Sent {
  body: string;
  key: string;
}

/** The key a request whose body is `body` goes with: the one it was sent with while its answer is
 * not known, else a new one. */
export function keyFor(body: string, sent: Sent | null): string {
  return sent !== null && sent.body === body ? sent.key : newKey();
}

/** Whether a failed post says the task or its workflow moved since its actions were read: the
 * server's own words begin with the refusal's code. */
export function isStale(error: unknown): boolean {
  return error instanceof Failure && /^stale_/.test(error.own);
}

/** Whether a failed request may have been taken (no answer, a server error): a post's request
 * keeps its key for the next send, and a read is tried again. */
export function unknownOutcome(error: unknown): boolean {
  return !(error instanceof Failure) || error.kind === "uncertain";
}

/** What the page read: the board's workflow (none where it could not read it), the actions
 * (none where the server refused them, with why) and the task's guards (none where the workflow
 * declares none, or their read was refused). */
export interface Read {
  board: Board | null;
  offer: Offer | null;
  refused: string;
  guards: WorkflowTaskGuards | null;
}

/**
 * The board's workflow and the actions on `slug` under `scope`, each read again on the feed's waits
 * until it has answered; none once `signal` ends. A refusal of the actions (a hosted visitor is no owner)
 * is said, and a board's read the page cannot read leaves the IDs to say themselves. The task's
 * guards are read where the definition declares one, or an action says a guard holds it. `wait` is
 * the wait before each read again (`tryAgain`'s).
 */
export async function readActions(
  scope: string,
  slug: string,
  signal: AbortSignal,
  wait?: Parameters<typeof tryAgain>[3],
): Promise<Read | null> {
  const got: Partial<Read> = {};
  const ask = async () => {
    if (!("board" in got)) {
      try {
        got.board = readBoard(await getJson(scoped(scope, WORKFLOW_PATH), signal));
      } catch (error) {
        if (!unknownOutcome(error)) got.board = null;
      }
    }
    const of = `?slug=${encodeURIComponent(slug)}`;
    if (!("offer" in got) && !signal.aborted) {
      try {
        got.offer = readOffer(await getJson(scoped(scope, WORKFLOW_ACTIONS_PATH + of), signal));
        got.refused = got.offer === null ? "the server's answer could not be read" : "";
      } catch (error) {
        if (!unknownOutcome(error)) {
          got.offer = null;
          got.refused = said(error);
        }
      }
    }
    if (!("board" in got && "offer" in got) || "guards" in got || signal.aborted) return;
    const held = got.offer?.actions.some((action) => action.unavailable.includes(GUARD));
    if (!got.board?.guarded && !held) {
      got.guards = null;
      return;
    }
    try {
      got.guards = readGuards(await getJson(scoped(scope, WORKFLOW_GUARDS_PATH + of), signal));
    } catch (error) {
      if (!unknownOutcome(error)) got.guards = null;
    }
  };
  await ask();
  await tryAgain(() => !("board" in got && "offer" in got && "guards" in got), ask, signal, wait);
  if (signal.aborted) return null;
  return {
    board: got.board ?? null,
    offer: got.offer ?? null,
    refused: got.refused ?? "",
    guards: got.guards ?? null,
  };
}

/** What taking an action came to. */
export interface Taken {
  /** Whether the server took it. */
  done: boolean;
  /** The actions it offers after it; none where it was not taken, or its answer is unread. */
  next: Offer | null;
  /** What the page says of it. */
  said: string;
  /** Whether a refusal says the task or its workflow moved since its actions were read. */
  moved: boolean;
  /** The request, kept while its answer is not known; none once the server has answered. */
  sent: Sent | null;
  /** The guard that refused it, where the refusal names one, and the refusal's own words. */
  guard: { refusal: WorkflowGuardRefusal; said: string } | null;
}

/**
 * Take `action` on `slug` under the offer it came in, with the inputs written: its edge's ID and
 * the snapshot it was offered under, with the key `sent` kept when it is that same request.
 */
export async function take(
  scope: string,
  slug: string,
  project: string,
  offer: Offer,
  action: Action,
  values: Values,
  sent: Sent | null,
): Promise<Taken> {
  const request = requestOf(project, slug, offer, action, values);
  const body = JSON.stringify(request);
  const key = keyFor(body, sent);
  try {
    const answer = await post(scoped(scope, WORKFLOW_APPLY_ROUTE), {
      slug,
      request: { ...request, idempotency_key: key },
    });
    const next = readOffer(field(field(answer, "result"), "next"));
    return {
      done: true,
      next,
      said: `${action.label}: done`,
      moved: false,
      sent: null,
      guard: null,
    };
  } catch (error) {
    const failed = error instanceof Failure ? error : null;
    const refusal = failed && readRefusal(field(failed.answer, "guard"));
    return {
      guard: refusal && { refusal, said: failed?.own ?? "" },
      done: false,
      next: null,
      // A guard's refusal is said in its row, under the action.
      said: `${action.label} was not taken: ${refusal ? `${words(refusal.guard)} holds it` : said(error)}`,
      moved: isStale(error),
      sent: unknownOutcome(error) ? { body, key } : null,
    };
  }
}

export interface NextActionsProps {
  /** The checkout the route names (`""`, the main one). */
  scope: string;
  slug: string;
  /** Where the workflow holds the task, as the page's read and its feed last said. */
  place: WorkflowPlace;
  /** The task's last change: the actions are read again after each. */
  updated: number;
}

/** The owner's take of an edge a guard refused, kept on the page until the task's revision moves. */
export interface Refused {
  edge: string;
  /** The task's workflow revision it was tried at. */
  revision: number;
  tried: Tried;
}

/** The task's place and the owner's next actions on it, read again on each change of the task. */
export function NextActions({ scope, slug, place, updated }: NextActionsProps) {
  const mount = useMount();
  const checking = useStore(STALE);
  const [read, setRead] = useState<Read | null>(null);
  const [round, setRound] = useState(0);
  const [moved, setMoved] = useState(false);
  const [busy, setBusy] = useState("");
  const [result, setResult] = useState("");
  const [values, setValues] = useState<Values>({});
  const [refused, setRefused] = useState<Refused | null>(null);
  const sent = useRef<Sent | null>(null);
  const { revision } = place;
  // biome-ignore lint/correctness/useExhaustiveDependencies: read again on each change of the task and each refresh asked, whose values the read does not use
  useEffect(() => {
    putStyle();
    const reading = new AbortController();
    void readActions(scope, slug, reading.signal).then((answer) => {
      if (answer === null) return;
      setRead(answer);
      setMoved(false);
    });
    return () => reading.abort();
  }, [scope, slug, revision, updated, round]);
  const choose = async (action: Action) => {
    const offer = read?.offer;
    const project = read?.board?.project;
    if (busy !== "" || STALE.get() || !offer || project === undefined) return;
    setBusy(action.edge);
    setResult("");
    const taken = await take(scope, slug, project, offer, action, values, sent.current);
    if (mount.aborted) return;
    sent.current = taken.sent;
    setBusy("");
    setResult(taken.said);
    setMoved(taken.moved);
    const { next, guard } = taken;
    setRefused(
      guard && {
        edge: action.edge,
        revision: offer.snapshot.task_revision,
        tried: { ...guard, at: Date.now() },
      },
    );
    if (taken.done) setValues({});
    if (next) setRead((was) => was && { ...was, offer: next });
  };
  return (
    <NextActionsView
      place={place}
      read={read}
      values={values}
      busy={busy}
      checking={checking}
      result={result}
      moved={moved}
      refused={refused}
      onWrite={(edge, input, text) =>
        setValues((was) => ({ ...was, [edge]: { ...was[edge], [input]: text } }))
      }
      onTake={(action) => void choose(action)}
      onRefresh={() => setRound((was) => was + 1)}
    />
  );
}

export interface NextActionsViewProps {
  place: WorkflowPlace;
  /** What the page read; none before its first read answers. */
  read: Read | null;
  values: Values;
  /** The edge whose post is out; empty for none. */
  busy: string;
  /** Whether the page may show what is out of date: the actions wait meanwhile. */
  checking: boolean;
  /** What the last action taken came to; empty for none. */
  result: string;
  /** Whether a refusal said the task or its workflow moved since: a refresh is offered. */
  moved: boolean;
  /** The owner's last take a guard refused; drawn while the task stands where it was tried. */
  refused?: Refused | null;
  onWrite: (edge: string, input: string, text: string) => void;
  onTake: (action: Action) => void;
  onRefresh: () => void;
}

/**
 * The task's place and next actions as drawn: where it stands, a recovery by hand it waits on, then
 * the guards that let it in, then each action its button, its destination, its inputs and,
 * unavailable, why and whom it needs, with the guards it needs under it. An action its guards
 * alone hold is taken from here when this host asks each one's authority. The
 * actions wait while the page checks, while a post is out and while their snapshot is behind the
 * task's place, whose read is on its way.
 */
export function NextActionsView({
  place,
  read,
  values,
  busy,
  checking,
  result,
  moved,
  refused = null,
  onWrite,
  onTake,
  onRefresh,
}: NextActionsViewProps) {
  const names = read?.board?.names ?? new Map<string, string>();
  const offer = read?.offer ?? null;
  const guards = read?.guards ?? null;
  const behind = offer !== null && offer.snapshot.task_revision !== place.revision;
  const placed = { place: (pair: WorkflowPair) => placeWords(names, pair) };
  const entry = guards?.entry ?? null;
  return (
    <section className="flow-task" aria-label="Workflow" aria-busy={read === null || behind}>
      <p className="flow-place">
        {"At "}
        <b>{placeWords(names, place)}</b>
        {place.resume && ` · resumes at ${placeWords(names, place.resume)}`}
      </p>
      {place.manual && <p className="flow-manual">{recoveryWords(place.manual)}</p>}
      {guards && entry && (
        <GuardRows
          of="entry"
          label="Guards that let it in"
          heading={
            <>
              {"Let in by "}
              <a href="#timeline">{`${entry.label} · revision ${entry.revision} · ${clock(entry.at)}`}</a>
            </>
          }
          rows={entryRows(guards, placed)}
        />
      )}
      {read?.refused && <p className="flow-refused">{`No actions: ${read.refused}`}</p>}
      {offer?.actions.length === 0 && <p className="flow-refused">No action is open now</p>}
      {offer !== null && offer.actions.length > 0 && (
        <ul className="flow-actions" aria-label="Next actions">
          {offer.actions.map((action) => {
            const tried =
              refused !== null &&
              refused.edge === action.edge &&
              refused.revision === place.revision
                ? refused.tried
                : null;
            const rows = guards
              ? needRows(
                  {
                    edge: action.edge,
                    label: action.label,
                    revision: offer.snapshot.task_revision,
                  },
                  guards,
                  tried,
                  placed,
                )
              : [];
            // Its guards alone hold it, and this host asks each one's authority: the take brings
            // their word, so it is the owner's to send.
            const asked = guardsAlone(action) && asksAll(action.edge, guards);
            const why = asked ? "" : unavailableWords(action, rows.length > 0);
            const held = !asked && action.unavailable.length > 0;
            return (
              <li key={action.edge} data-edge={action.edge}>
                {action.inputs.map((input) => (
                  <label key={input} className="flow-input">
                    {`${words(input)} `}
                    <input
                      className="dx-input"
                      value={values[action.edge]?.[input] ?? ""}
                      disabled={held}
                      onInput={(event) => onWrite(action.edge, input, event.currentTarget.value)}
                    />
                  </label>
                ))}
                <Button
                  type="button"
                  disabled={held || busy !== "" || checking || behind || missing(action, values)}
                  onClick={() => onTake(action)}
                >
                  {checking ? CHECKING : busy === action.edge ? "Sending…" : action.label}
                </Button>
                <span className="flow-to">{`to ${placeWords(names, action.destination)}`}</span>
                {why !== "" && <span className="flow-why">{why}</span>}
                <GuardRows
                  of={action.edge}
                  label={`${action.label} needs`}
                  heading={`${action.label} needs`}
                  rows={rows}
                  foot={footWords(rows, action.label)}
                />
              </li>
            );
          })}
        </ul>
      )}
      {result !== "" && <p role="status">{result}</p>}
      {moved && (
        <p className="flow-moved" role="alert">
          {"The task or its workflow moved since these actions were read. "}
          <Button type="button" onClick={onRefresh}>
            Refresh actions
          </Button>
        </p>
      )}
    </section>
  );
}
