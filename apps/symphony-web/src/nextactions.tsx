// A task a custom workflow holds, on its page (docs/workflow/WORKFLOW.md, "The app"): where it stands, by
// its phase's and state's labels, with the place a block resumes at and the person a recovery by
// hand waits on, and the local owner's next actions on it as the server offers them
// (`/api/workflow-actions`): each its edge's label and destination, and one unavailable with why
// and whom it needs, with the accepted output standing for each task it depends on where an edge
// takes them. An action posts its edge's ID with the snapshot it was offered under, and those
// outputs where its edge depends on them
// (`/api/workflow-apply`) and a key kept until the server has answered, so a post sent again after
// no answer is the same request, which the board takes once; a refusal that the task or its
// workflow moved since prompts a refresh of the actions. A chunk of its own, which the page imports
// for a task a workflow holds alone, so a task the board's own writers keep loads none of it; its
// style is flow.tsx's, which it puts in the page.

import { useEffect, useRef, useState } from "react";
import { Button } from "./button";
import { name, putStyle, readDefinition } from "./flow";
import { useMount } from "./mount";
import { scoped } from "./project";
import { Failure, getJson, post, said } from "./request";
import { field, list, STALE, useStore } from "./shared";
import { tryAgain } from "./state";
import {
  WORKFLOW_ACTIONS_PATH,
  WORKFLOW_APPLY_ROUTE,
  WORKFLOW_PATH,
  type WorkflowAuthority,
  type WorkflowManualRecovery,
  type WorkflowOperationRequest,
  type WorkflowPair,
  type WorkflowPlace,
  type WorkflowTakenOutput,
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
  /** What its edge does: a land's or an adapter's is taken by them, never by the page. */
  operation: string;
  label: string;
  destination: WorkflowPair;
  /** Whom it is open to: said beside an action that is unavailable. */
  responsible: WorkflowAuthority[];
  /** The inputs it needs, each a text the owner writes. */
  inputs: string[];
  /** Why it cannot be taken now, by refusal code; none when it can. */
  unavailable: string[];
}

/** The actions the server offers, with the snapshot they were offered under, sent back as it came,
 * and the accepted output standing for each task the task depends on where an edge leaving its
 * place takes them (none in an apply's answer, which the page reads again after). */
export interface Offer {
  snapshot: WorkflowOperationRequest["expected"];
  actions: Action[];
  takes: WorkflowTakenOutput[];
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
  const operation = field(value, "operation");
  const label = field(value, "label");
  if (
    !isText(edge) ||
    !isText(operation) ||
    !isText(label) ||
    !isPair(destination) ||
    !Array.isArray(responsible) ||
    !responsible.every(readAuthority) ||
    !texts(inputs) ||
    !texts(unavailable)
  ) {
    return null;
  }
  return { edge, operation, label, destination, responsible, inputs, unavailable };
}

function readTaken(value: unknown): value is WorkflowTakenOutput {
  const output = field(value, "output");
  return isText(field(value, "task")) && (output === null || isText(output));
}

/**
 * The next actions a read (`/api/workflow-actions`, or an apply's `result.next`) offers, or none
 * for one that is not what the page draws. The snapshot is checked as far as the page reads it
 * (where it was taken) and is otherwise sent back whole.
 */
export function readOffer(value: unknown): Offer | null {
  const snapshot = field(value, "snapshot");
  const actions = field(value, "actions");
  const takes = field(value, "takes") ?? [];
  if (!isPair(field(snapshot, "at")) || !isNumber(field(snapshot, "task_revision"))) return null;
  if (!Array.isArray(actions) || !Array.isArray(takes) || !takes.every(readTaken)) return null;
  const read = actions.map(readAction);
  if (!read.every((action): action is Action => action !== null)) return null;
  return {
    snapshot: snapshot as Offer["snapshot"],
    actions: read,
    takes: takes.map(({ task, output }) => ({ task, output })),
  };
}

/** What the board's workflow read gives the task's page: the project a request names, the labels
 * of the phases, states and places of its definition (flow.tsx's `name`), and the edges that take
 * the accepted outputs of what a task depends on. */
export interface Board {
  project: string;
  names: Map<string, string>;
  taking: Set<string>;
}

/** The board's workflow read (`/api/workflow`) as the page reads it, or none for one it cannot. */
export function readBoard(value: unknown): Board | null {
  const project = field(value, "project");
  const definition = readDefinition(value);
  if (!isText(project) || !definition) return null;
  const names = new Map<string, string>();
  name(definition, names);
  const edges = list(field(field(field(value, "workflow"), "definition"), "edges"));
  const taking = new Set(
    edges
      .filter((edge) => field(edge, "dependencies") === "accepted_output")
      .map((edge) => field(edge, "id"))
      .filter(isText),
  );
  return { project, names, taking };
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
  guard_unavailable: "its check does not answer here",
  explicit_action_required: "needs an explicit action",
};

/** Who takes an edge whose operation is a land's or an adapter's, never the page (as `just tasks
 * workflow actions` says it), by the task's `slug`; none for any other. */
export function takerWords(action: Action, slug: string): string | null {
  switch (action.operation) {
    case "prepare_land":
    case "confirm_land":
      return `the land takes it: just land ${slug}`;
    case "prepare_external":
    case "confirm_external":
      return "its adapter takes it";
    default:
      return null;
  }
}

/** Why `action` on `slug` is unavailable, and whom it needs, as its line says it; empty while it
 * is not. An edge a land or an adapter takes says so in place of asking for an explicit action. */
export function unavailableWords(action: Action, slug: string): string {
  if (action.unavailable.length === 0) return "";
  const taker = takerWords(action, slug);
  const explicit = "explicit_action_required";
  const theirs = taker !== null && action.unavailable.includes(explicit);
  const others = action.unavailable
    .filter((code) => !(theirs && code === explicit))
    .map((code) => WHY[code] ?? "not supported here");
  const why = theirs && taker !== null ? [taker, ...others] : others;
  const needs =
    action.responsible.length > 0 && others.length > 0
      ? `: needs ${whose(action.responsible)}`
      : "";
  return `${[...new Set(why)].join("; ")}${needs}`;
}

/** The accepted output `taken` names, as its line says it. */
export function takenWords(taken: WorkflowTakenOutput): string {
  return taken.output === null
    ? `Takes ${taken.task}'s accepted output: none stands`
    : `Takes ${taken.task}'s accepted output ${taken.output}`;
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

/** The request taking `action` on `slug` on the `board`'s project, under the offer's snapshot, but
 * its key: an edge that depends on accepted outputs names each one the offer read standing. */
export function requestOf(
  board: Board,
  slug: string,
  offer: Offer,
  action: Action,
  values: Values,
): Omit<WorkflowOperationRequest, "idempotency_key"> {
  const taken = board.taking.has(action.edge) ? offer.takes : [];
  return {
    project: board.project,
    task: slug,
    edge: action.edge,
    expected: offer.snapshot,
    input_revisions: Object.fromEntries(
      taken.flatMap(({ task, output }) => (output === null ? [] : [[task, output]])),
    ),
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

/** What the page read: the board's workflow (none where it could not read it) and the actions
 * (none where the server refused them, with why). */
export interface Read {
  board: Board | null;
  offer: Offer | null;
  refused: string;
}

/**
 * The board's workflow and the actions on `slug` under `scope`, each read again on the feed's waits
 * until it has answered; none once `signal` ends. A refusal of the actions (a hosted visitor is no owner)
 * is said, and a board's read the page cannot read leaves the IDs to say themselves. `wait` is
 * the wait before each read again (`tryAgain`'s).
 */
export async function readActions(
  scope: string,
  slug: string,
  signal: AbortSignal,
  wait?: Parameters<typeof tryAgain>[3],
): Promise<Read | null> {
  const got: { board?: Board | null; offer?: Offer | null; refused?: string } = {};
  const ask = async () => {
    if (!("board" in got)) {
      try {
        got.board = readBoard(await getJson(scoped(scope, WORKFLOW_PATH), signal));
      } catch (error) {
        if (!unknownOutcome(error)) got.board = null;
      }
    }
    if ("offer" in got || signal.aborted) return;
    try {
      const path = `${WORKFLOW_ACTIONS_PATH}?slug=${encodeURIComponent(slug)}`;
      got.offer = readOffer(await getJson(scoped(scope, path), signal));
      got.refused = got.offer === null ? "the server's answer could not be read" : "";
    } catch (error) {
      if (!unknownOutcome(error)) {
        got.offer = null;
        got.refused = said(error);
      }
    }
  };
  await ask();
  await tryAgain(() => !("board" in got && "offer" in got), ask, signal, wait);
  if (signal.aborted) return null;
  return { board: got.board ?? null, offer: got.offer ?? null, refused: got.refused ?? "" };
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
}

/**
 * Take `action` on `slug` under the offer it came in, with the inputs written: its edge's ID and
 * the snapshot it was offered under, with the key `sent` kept when it is that same request.
 */
export async function take(
  scope: string,
  slug: string,
  board: Board,
  offer: Offer,
  action: Action,
  values: Values,
  sent: Sent | null,
): Promise<Taken> {
  const request = requestOf(board, slug, offer, action, values);
  const body = JSON.stringify(request);
  const key = keyFor(body, sent);
  try {
    const answer = await post(scoped(scope, WORKFLOW_APPLY_ROUTE), {
      slug,
      request: { ...request, idempotency_key: key },
    });
    const next = readOffer(field(field(answer, "result"), "next"));
    return { done: true, next, said: `${action.label}: done`, moved: false, sent: null };
  } catch (error) {
    return {
      done: false,
      next: null,
      said: `${action.label} was not taken: ${said(error)}`,
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
    const board = read?.board;
    if (busy !== "" || STALE.get() || !offer || !board) return;
    setBusy(action.edge);
    setResult("");
    const taken = await take(scope, slug, board, offer, action, values, sent.current);
    if (mount.aborted) return;
    sent.current = taken.sent;
    setBusy("");
    setResult(taken.said);
    setMoved(taken.moved);
    const { next } = taken;
    if (taken.done) setValues({});
    if (next) setRead((was) => was && { ...was, offer: next });
  };
  return (
    <NextActionsView
      slug={slug}
      place={place}
      read={read}
      values={values}
      busy={busy}
      checking={checking}
      result={result}
      moved={moved}
      onWrite={(edge, input, text) =>
        setValues((was) => ({ ...was, [edge]: { ...was[edge], [input]: text } }))
      }
      onTake={(action) => void choose(action)}
      onRefresh={() => setRound((was) => was + 1)}
    />
  );
}

export interface NextActionsViewProps {
  slug: string;
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
  onWrite: (edge: string, input: string, text: string) => void;
  onTake: (action: Action) => void;
  onRefresh: () => void;
}

/**
 * The task's place and next actions as drawn: where it stands, a recovery by hand it waits on, then
 * each action its button, its destination, its inputs and, unavailable, why and whom it needs. The
 * actions wait while the page checks, while a post is out and while their snapshot is behind the
 * task's place, whose read is on its way.
 */
export function NextActionsView({
  slug,
  place,
  read,
  values,
  busy,
  checking,
  result,
  moved,
  onWrite,
  onTake,
  onRefresh,
}: NextActionsViewProps) {
  const names = read?.board?.names ?? new Map<string, string>();
  const offer = read?.offer ?? null;
  const behind = offer !== null && offer.snapshot.task_revision !== place.revision;
  return (
    <section className="flow-task" aria-label="Workflow" aria-busy={read === null || behind}>
      <p className="flow-place">
        {"At "}
        <b>{placeWords(names, place)}</b>
        {place.resume && ` · resumes at ${placeWords(names, place.resume)}`}
      </p>
      {place.manual && <p className="flow-manual">{recoveryWords(place.manual)}</p>}
      {offer !== null && offer.takes.length > 0 && (
        <ul className="flow-takes" aria-label="Accepted outputs taken">
          {offer.takes.map((taken) => (
            <li key={taken.task}>{takenWords(taken)}</li>
          ))}
        </ul>
      )}
      {read?.refused && <p className="flow-refused">{`No actions: ${read.refused}`}</p>}
      {offer?.actions.length === 0 && <p className="flow-refused">No action is open now</p>}
      {offer !== null && offer.actions.length > 0 && (
        <ul className="flow-actions" aria-label="Next actions">
          {offer.actions.map((action) => {
            const why = unavailableWords(action, slug);
            return (
              <li key={action.edge} data-edge={action.edge}>
                {action.inputs.map((input) => (
                  <label key={input} className="flow-input">
                    {`${words(input)} `}
                    <input
                      className="dx-input"
                      value={values[action.edge]?.[input] ?? ""}
                      disabled={why !== ""}
                      onInput={(event) => onWrite(action.edge, input, event.currentTarget.value)}
                    />
                  </label>
                ))}
                <Button
                  type="button"
                  disabled={
                    why !== "" || busy !== "" || checking || behind || missing(action, values)
                  }
                  onClick={() => onTake(action)}
                >
                  {checking ? CHECKING : busy === action.edge ? "Sending…" : action.label}
                </Button>
                <span className="flow-to">{`to ${placeWords(names, action.destination)}`}</span>
                {why !== "" && <span className="flow-why">{why}</span>}
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
