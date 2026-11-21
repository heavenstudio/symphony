// The guards of a task a custom workflow holds, on its page (docs/WORKFLOW.md, "The app"): under
// each action a guard holds, a row for each guard its edge carries, and above the actions the
// guards the task was let into its place by, each saying whether it still stands. A row is a gate's
// row in shape: the guard's name, its state in a word, a line saying why, and its authority with
// what the board accepted from it. The name is the row's link: it opens, under the row, what the
// board holds for the guard. What the authority itself points at (the decisions a pass rests on,
// its reasons for a refusal) is drawn as links by the rule the gates link by (`links.ts`), only
// from an attestation that verified; its ledger's page, where the project names one, is on its
// name. The page shows and links: nothing in it approves, grants or sends an attestation, and what
// it knows of a refusal is the owner's own take on this page, kept until the task moves.
//
// It reads `/api/workflow-guards` beside the actions, and the guard that refused a take from the
// refusal's own answer (`guard`, crates/symphony-api's `GuardRefusal`), by its word and never by
// its text. Part of the task page's workflow chunk (nextactions.tsx), so a task the board's own
// writers keep loads none of it.

import { type ReactNode, useState } from "react";
import { appPath, taskSlug, webAddress } from "./links";
import { href } from "./project";
import { field } from "./shared";
import type {
  WorkflowAttestee,
  WorkflowGuardAuthority,
  WorkflowGuardEntry,
  WorkflowGuardMet,
  WorkflowGuardRecord,
  WorkflowGuardRefusal,
  WorkflowPair,
  WorkflowTaskGuards,
} from "./wire";

const isText = (value: unknown): value is string => typeof value === "string";
const isNumber = (value: unknown): value is number => typeof value === "number";
const texts = (value: unknown): value is string[] => Array.isArray(value) && value.every(isText);
const isPair = (value: unknown): value is WorkflowPair =>
  isText(field(value, "phase")) && isText(field(value, "state"));
const isWho = (value: unknown): value is WorkflowAttestee =>
  isText(field(value, "kind")) && isText(field(value, "subject"));
const absent = (value: unknown) => value === undefined || value === null;

/** What an attestation said, as the page draws it, or none for one it cannot read. */
function readRecord(value: unknown): WorkflowGuardRecord | null {
  const revisions = field(value, "input_revisions");
  const read =
    isText(field(value, "result")) &&
    texts(field(value, "facts")) &&
    texts(field(value, "reasons")) &&
    isText(field(value, "epoch")) &&
    isNumber(field(value, "generation")) &&
    isNumber(field(value, "issued_at")) &&
    isNumber(field(value, "expires_at")) &&
    isNumber(field(value, "task_revision")) &&
    isPair(field(value, "at")) &&
    typeof revisions === "object" &&
    revisions !== null &&
    Object.values(revisions).every(isText) &&
    isWho(field(value, "principal"));
  return read ? (value as WorkflowGuardRecord) : null;
}

/** A record that may be absent: `undefined` for one that is there and cannot be read. */
function readOptionalRecord(value: unknown): WorkflowGuardRecord | null | undefined {
  if (absent(value)) return null;
  return readRecord(value) ?? undefined;
}

/**
 * The guard that refused a take, as the refusal's answer names it beside its words, or none: for an
 * answer that names none, and for one the page cannot read. A word it does not know is kept, and
 * said as a refusal in the answer's own words.
 */
export function readRefusal(value: unknown): WorkflowGuardRefusal | null {
  const record = readOptionalRecord(field(value, "record"));
  if (
    !isText(field(value, "guard")) ||
    !isText(field(value, "adapter")) ||
    !isText(field(value, "why")) ||
    record === undefined
  ) {
    return null;
  }
  return { ...(value as WorkflowGuardRefusal), record };
}

function readAuthority(value: unknown): value is WorkflowGuardAuthority {
  const [epoch, generation] = [field(value, "epoch"), field(value, "generation")];
  return (
    isText(field(value, "adapter")) &&
    isText(field(value, "issuer")) &&
    (absent(epoch) || isText(epoch)) &&
    (absent(generation) || isNumber(generation)) &&
    typeof field(value, "conflicted") === "boolean" &&
    isNumber(field(value, "admitted")) &&
    typeof field(value, "asked_here") === "boolean"
  );
}

function readMet(value: unknown): WorkflowGuardMet | null {
  const record = readOptionalRecord(field(value, "record"));
  const read =
    isText(field(value, "guard")) &&
    isText(field(value, "adapter")) &&
    isText(field(value, "issuer")) &&
    isText(field(value, "epoch")) &&
    isNumber(field(value, "generation")) &&
    isText(field(value, "standing")) &&
    record !== undefined;
  return read ? { ...(value as WorkflowGuardMet), record } : null;
}

function readEntry(value: unknown): WorkflowGuardEntry | null | undefined {
  if (absent(value)) return null;
  const guards = field(value, "guards");
  const met = Array.isArray(guards) ? guards.map(readMet) : [null];
  const read =
    isText(field(value, "edge")) &&
    isText(field(value, "label")) &&
    isNumber(field(value, "revision")) &&
    isWho(field(value, "principal")) &&
    isNumber(field(value, "at")) &&
    met.every((one) => one !== null);
  return read ? { ...(value as WorkflowGuardEntry), guards: met as WorkflowGuardMet[] } : undefined;
}

/** A task's guards (`/api/workflow-guards`) as the page draws them, or none for a read it cannot. */
export function readGuards(value: unknown): WorkflowTaskGuards | null {
  const authorities = field(value, "authorities");
  const guards = field(value, "guards");
  const edges = field(value, "edges");
  const entry = readEntry(field(value, "entry"));
  const page = (one: unknown) => absent(field(one, "page")) || isText(field(one, "page"));
  if (
    !Array.isArray(authorities) ||
    !authorities.every(readAuthority) ||
    !Array.isArray(guards) ||
    !guards.every(
      (one) => isText(field(one, "guard")) && isText(field(one, "adapter")) && page(one),
    ) ||
    !Array.isArray(edges) ||
    !edges.every((one) => isText(field(one, "edge")) && texts(field(one, "guards"))) ||
    entry === undefined
  ) {
    return null;
  }
  return { ...(value as WorkflowTaskGuards), entry };
}

/** A piece of a row's words: text, a command to copy, or a link, one that leaves the app marked by
 * the host it goes to. */
export type Part = string | { code: string } | { link: string; text: string; out?: string };

/** What an app path is a link to, as its words name it. */
function named(path: string): string {
  const slug = taskSlug(path);
  if (slug !== null) return `task ${slug}`;
  const [file = "", anchor = ""] = path.split("#");
  if (/\/DECISIONS\.md$/.test(file) && anchor !== "") return `decision ${anchor}`;
  const run = /[?&]run=([^&#]+)/.exec(path);
  if (/\/runs(\?|$)/.test(file) && run) return `run ${run[1]}`;
  const leaf = file.split("?")[0]?.split("/").pop() ?? "";
  return leaf.endsWith(".md") ? `document ${leaf}` : path;
}

/** The host of a web address, as a link that leaves the app is marked. */
function hostOf(url: string): string {
  return url.replace(/^[a-z]+:\/\//i, "").split(/[/?#]/)[0] ?? "";
}

/**
 * An authority's own reference (a fact a pass rests on, a reason) as the page draws it, by the
 * gates' rule: a path of the app in it is a link named for what it is, a web address at its end
 * puts the link on the words before it, marked as leaving the app, and anything else is its text.
 */
export function referred(text: string): Part[] {
  const words = text.trim().split(/\s+/);
  const last = words[words.length - 1] ?? "";
  if (webAddress(last)) {
    const before = words.slice(0, -1).join(" ");
    return [{ link: last, text: before === "" ? last : before, out: hostOf(last) }];
  }
  const at = words.findIndex((word) => appPath(word) && word.length > 1);
  if (at < 0) return [text];
  const path = words[at] ?? "";
  const [before, after] = [words.slice(0, at).join(" "), words.slice(at + 1).join(" ")];
  return [
    ...(before === "" ? [] : [`${before} `]),
    { link: path, text: named(path) },
    ...(after === "" ? [] : [` ${after}`]),
  ];
}

/** `references`, each drawn by `referred`, a dot between two. */
function references(all: string[]): Part[] {
  return all.flatMap((one, at) => [...(at > 0 ? [" · "] : []), ...referred(one)]);
}

/** A time of the board's, seconds since the epoch, as the page says it: `13:40:07 UTC`. */
export function clock(seconds: number): string {
  const at = new Date(seconds * 1000);
  return Number.isNaN(at.getTime()) ? "an unknown time" : `${at.toISOString().slice(11, 19)} UTC`;
}

/** An ID as words, where its definition names it no label. */
const words = (id: string) => id.replaceAll("_", " ");

/** How a row is toned: its dot and its state's pill. */
export type Tone = "quiet" | "amber" | "red" | "green";

/** A line of a row's record: what it is, and its words. */
export interface Line {
  label: string;
  parts: Part[];
}

/** One guard as a row draws it. */
export interface Row {
  guard: string;
  /** The guard's name, its ID in words. */
  name: string;
  /** Its state, in a word. */
  state: string;
  tone: Tone;
  why: Part[];
  /** Its authority: the issuer it signs as (the adapter where none is committed), its ledger's
   * page where the project names one, its adapter, and what the board accepted from it. */
  authority: { name: string; page: string | null; adapter: string; accepted: string };
  /** What the board holds for it, opened by its name: its heading and its lines. */
  record: { title: string; lines: Line[] };
}

/** What the board accepted from `authority`, as a row's second line says it. */
export function acceptedWords(authority: WorkflowGuardAuthority | undefined): string {
  if (!authority) return "no authority committed";
  if (absent(authority.generation)) return "accepted: none yet";
  const state = `gen ${authority.generation} · ${authority.epoch ?? ""}`;
  return authority.conflicted ? `in conflict at ${state}` : `accepted ${state}`;
}

/** Who an attestation covers, as a record says it. */
function who(principal: WorkflowAttestee): string {
  return principal.kind === "human" ? "you" : `the ${principal.kind} ${principal.subject}`;
}

/** The lines every record of an attestation has: the authority's word, what it rests on, its
 * reasons, its time and what it covers. */
function saidLines(issuer: string, record: WorkflowGuardRecord, names: Names): Line[] {
  const inputs = Object.entries(record.input_revisions).map(([name, at]) => `${name} ${at}`);
  return [
    {
      label: `${issuer}'s word`,
      parts: [`${record.result} · decided at generation ${record.generation} · ${record.epoch}`],
    },
    ...(record.facts.length > 0 ? [{ label: "Rests on", parts: references(record.facts) }] : []),
    ...(record.reasons.length > 0
      ? [{ label: "Its reasons", parts: references(record.reasons) }]
      : []),
    {
      label: "Its time",
      parts: [`issued ${clock(record.issued_at)} · good until ${clock(record.expires_at)}`],
    },
    {
      label: "Covers",
      parts: [
        `${names.place(record.at)}, task revision ${record.task_revision}, taken by ${who(record.principal)}`,
        ...(inputs.length > 0 ? [`, inputs: ${inputs.join(", ")}`] : []),
      ],
    },
  ];
}

/** What the board accepted from an authority, as a record's lines. */
function boardLines(authority: WorkflowGuardAuthority | undefined): Line[] {
  if (!authority) return [{ label: "The board", parts: ["holds no authority for this guard"] }];
  return [
    { label: `From ${authority.issuer}`, parts: [acceptedWords(authority)] },
    {
      label: "Admitted on it",
      parts: [`${authority.admitted} ${authority.admitted === 1 ? "operation" : "operations"}`],
    },
    {
      label: "Asked from here",
      parts: [
        authority.asked_here ? "yes, when you take the action" : "no: this app cannot ask it",
      ],
    },
  ];
}

/** What a row needs of the page around it: the labels of places. */
export interface Names {
  place: (pair: WorkflowPair) => string;
}

/** A take a guard refused, as the page keeps it until the task moves: the guard, when, and the
 * refusal's own words. */
export interface Tried {
  refusal: WorkflowGuardRefusal;
  /** When the page heard it, in milliseconds. */
  at: number;
  /** The refusal as the server said it. */
  said: string;
}

/** An action a guard holds, as its rows are drawn from it. */
export interface Guarded {
  slug: string;
  edge: string;
  label: string;
  /** The task's workflow revision the action was offered at. */
  revision: number;
}

/** The state and why of a guard a take refused, by the refusal's word. */
function triedWords(
  tried: Tried,
  issuer: string,
  action: Guarded,
  authority: WorkflowGuardAuthority | undefined,
): { state: string; tone: Tone; why: Part[] } {
  const { refusal } = tried;
  const record = refusal.record ?? null;
  const yours = `Your ${action.label} at ${clock(tried.at / 1000)}`;
  const now = absent(authority?.generation) ? "another" : `generation ${authority?.generation}`;
  switch (refusal.why) {
    case "missing":
      return {
        state: "missing",
        tone: "amber",
        why: [`${yours} went with no attestation from ${issuer}.`],
      };
    case "unanswered":
      return {
        state: "unanswered",
        tone: "amber",
        why: [`${yours} asked ${issuer}, which gave no attestation: `, tried.said],
      };
    case "unverified":
      return {
        state: "unverified",
        tone: "red",
        why: [
          `The attestation does not verify by ${issuer}'s committed key, so nothing it says is shown or linked.`,
        ],
      };
    case "refused":
      return {
        state: "refused",
        tone: "red",
        why: [`${yours} was refused. ${issuer} says: `, ...references(record?.reasons ?? [])],
      };
    case "unknown":
      return {
        state: "unknown",
        tone: "quiet",
        why: [`${issuer} cannot say: `, ...references(record?.reasons ?? [])],
      };
    case "unfounded":
      return {
        state: "unfounded",
        tone: "amber",
        why: [`${issuer} passed it on no decision of its own, which the board admits nothing on.`],
      };
    case "expired":
      return {
        state: "expired",
        tone: "amber",
        why: [
          `The attestation ${issuer} gave expired at ${clock(record?.expires_at ?? Number.NaN)}. ${action.label} again asks for a fresh one.`,
        ],
      };
    case "early":
      return {
        state: "early",
        tone: "amber",
        why: [
          `The attestation ${issuer} gave says it was issued at ${clock(record?.issued_at ?? Number.NaN)}, ahead of this machine's clock.`,
        ],
      };
    case "overlong":
      return {
        state: "held too long",
        tone: "amber",
        why: [`The attestation ${issuer} gave holds for longer than an attestation may.`],
      };
    case "stale":
      return {
        state: "stale",
        tone: "amber",
        why: [
          `The attestation ${issuer} gave covers another operation (task revision ${record?.task_revision ?? "?"}); the task is at revision ${action.revision}.`,
        ],
      };
    case "decided_again":
      return {
        state: "decided again",
        tone: "amber",
        why: [
          `${issuer} attested at generation ${record?.generation ?? "?"} and has decided again since (${now} now).`,
        ],
      };
    case "not_reported":
      return {
        state: "not reported",
        tone: "amber",
        why: [
          `${issuer} attested at generation ${record?.generation ?? "?"}, which it has not reported to the board yet.`,
        ],
      };
    case "not_yours":
      return {
        state: "not yours",
        tone: "red",
        why: [
          `The attestation ${issuer} gave covers ${record ? who(record.principal) : "another principal"}, not you.`,
        ],
      };
    default:
      // The board's records refused it (unreported, conflict, uncommitted), or a word this page
      // does not know: the refusal's own words.
      return { state: "refused", tone: "red", why: [tried.said] };
  }
}

/** The state and why of a guard nothing was tried on, from the board's records. */
function untriedWords(
  issuer: string,
  action: Guarded,
  authority: WorkflowGuardAuthority | undefined,
): { state: string; tone: Tone; why: Part[] } {
  if (!authority) {
    return {
      state: "uncommitted",
      tone: "red",
      why: ["The active workflow commits no authority for this guard, so nothing meets it."],
    };
  }
  if (absent(authority.generation)) {
    return {
      state: "unreported",
      tone: "amber",
      why: [
        `${issuer} has reported no state to the board, so nothing it attests is admitted until it does: `,
        { code: "just tasks workflow observe FILE" },
      ],
    };
  }
  if (authority.conflicted) {
    return {
      state: "conflict",
      tone: "red",
      why: [
        `${issuer} reported two decisions at generation ${authority.generation}: nothing it attests is admitted until it reports the state after them.`,
      ],
    };
  }
  if (!authority.asked_here) {
    return {
      state: "not from here",
      tone: "quiet",
      why: [
        `This app cannot ask ${issuer}. With ${issuer}'s attestation file, from the command line: `,
        {
          code: `just tasks workflow apply ${action.slug} ${action.edge} --revision ${action.revision} --attestation FILE`,
        },
      ],
    };
  }
  return {
    state: "no word yet",
    tone: "quiet",
    why: [`${action.label} asks ${issuer} for its word when you take it.`],
  };
}

/** The authority of `adapter` among those the read holds, and the page of `guard`. */
function authorityOf(guards: WorkflowTaskGuards, guard: string) {
  const need = guards.guards.find((one) => one.guard === guard);
  const authority = guards.authorities.find((one) => one.adapter === need?.adapter);
  const adapter = need?.adapter ?? "";
  return {
    authority,
    shown: {
      name: authority?.issuer ?? adapter,
      page: need?.page ?? null,
      adapter,
      accepted: acceptedWords(authority),
    },
  };
}

/**
 * The rows of the guards `action`'s edge carries: each from the board's records, or, for the guard
 * the owner's last take of the action was refused by, from that refusal. None for an action no
 * guard holds.
 */
export function needRows(
  action: Guarded,
  guards: WorkflowTaskGuards,
  tried: Tried | null,
  names: Names,
): Row[] {
  const edge = guards.edges.find((one) => one.edge === action.edge);
  return (edge?.guards ?? []).map((guard) => {
    const { authority, shown } = authorityOf(guards, guard);
    const mine = tried !== null && tried.refusal.guard === guard ? tried : null;
    const said = mine
      ? triedWords(mine, shown.name, action, authority)
      : untriedWords(shown.name, action, authority);
    const record = mine?.refusal.record ?? null;
    return {
      guard,
      name: words(guard),
      ...said,
      authority: shown,
      record: mine
        ? {
            title: `What the board read from your ${action.label} at ${clock(mine.at / 1000)} · kept on this page until the task moves or the page reloads`,
            lines: [
              ...(record
                ? [
                    ...saidLines(shown.name, record, names),
                    {
                      label: "Signature",
                      parts: [`verifies by the key the project commits for ${shown.name}`],
                    },
                  ]
                : [{ label: "The board said", parts: [mine.said] }]),
              ...boardLines(authority),
            ],
          }
        : { title: "What the board holds for this guard", lines: boardLines(authority) },
    };
  });
}

/** Why a guard an admission met no longer stands, by its standing's word. */
function standingWords(met: WorkflowGuardMet, authority: WorkflowGuardAuthority | undefined) {
  const issuer = met.issuer;
  const none: { state: string; tone: Tone; after: string } = {
    state: "met",
    tone: "green",
    after: `, which the board still accepts`,
  };
  const gone = "so it no longer stands: no land is sent from here on it";
  switch (met.standing) {
    case "met":
      return none;
    case "decided_again":
      return {
        state: "decided again",
        tone: "amber" as Tone,
        after: `. ${issuer} has decided again since (${absent(authority?.generation) ? "another generation" : `generation ${authority?.generation}`} now), ${gone}`,
      };
    case "conflict":
      return {
        state: "conflict",
        tone: "red" as Tone,
        after: `. ${issuer} is in conflict now, ${gone}`,
      };
    case "unreported":
      return {
        state: "unreported",
        tone: "amber" as Tone,
        after: `. The board holds no state from ${issuer} now, ${gone}`,
      };
    case "uncommitted":
      return {
        state: "uncommitted",
        tone: "red" as Tone,
        after: `. The active workflow no longer commits ${met.adapter}, ${gone}`,
      };
    case "unverified":
      return {
        state: "unverified",
        tone: "red" as Tone,
        after: `. Its attestation no longer verifies by the key committed for ${issuer}, ${gone}`,
      };
    default:
      return { state: "not standing", tone: "amber" as Tone, after: `, and ${gone}` };
  }
}

/** The rows of the guards the task was let into its place by, each standing or not. */
export function entryRows(guards: WorkflowTaskGuards, names: Names): Row[] {
  const entry = guards.entry;
  if (!entry) return [];
  return entry.guards.map((met) => {
    const { authority, shown } = authorityOf(guards, met.guard);
    const said = standingWords(met, authority);
    const record = met.record ?? null;
    const rests = record && record.facts.length > 0 ? references(record.facts) : null;
    return {
      guard: met.guard,
      name: words(met.guard),
      state: said.state,
      tone: said.tone,
      why: [
        `Met at generation ${met.generation}`,
        ...(rests ? [", resting on ", ...rests] : []),
        `${said.after}.`,
      ],
      authority: { ...shown, name: met.issuer },
      record: {
        title: "What the board recorded when it let the task in",
        lines: [
          {
            label: "Let in by",
            parts: [
              `${entry.label} · revision ${entry.revision} · ${clock(entry.at)}, taken by ${who(entry.principal)}`,
            ],
          },
          ...(record
            ? saidLines(met.issuer, record, names)
            : [{ label: `${met.issuer}'s word`, parts: ["its attestation is not shown"] }]),
          ...boardLines(authority),
        ],
      },
    };
  });
}

/** Whether the owner's take of an action only its guards hold can go from this app: each guard's
 * authority is one this host asks, so the take brings its attestation. */
export function asksAll(edge: string, guards: WorkflowTaskGuards | null): boolean {
  const carried = guards?.edges.find((one) => one.edge === edge)?.guards ?? [];
  return (
    carried.length > 0 &&
    carried.every((guard) => authorityOf(guards as WorkflowTaskGuards, guard).authority?.asked_here)
  );
}

/** `parts` as drawn: a link under the project's base, one that leaves the app in a tab of its own
 * with its host beside it; a command as code. */
function Parts({ parts }: { parts: Part[] }): ReactNode {
  return parts.map((part, at) => {
    // biome-ignore lint/suspicious/noArrayIndexKey: a text's parts keep their order
    if (typeof part === "string") return <span key={at}>{part}</span>;
    // biome-ignore lint/suspicious/noArrayIndexKey: a text's parts keep their order
    if ("code" in part) return <code key={at}>{part.code}</code>;
    return part.out === undefined ? (
      // biome-ignore lint/suspicious/noArrayIndexKey: a text's parts keep their order
      <a key={at} href={href(part.link)}>
        {part.text}
      </a>
    ) : (
      // biome-ignore lint/suspicious/noArrayIndexKey: a text's parts keep their order
      <span key={at}>
        <a className="guard-out" href={part.link} target="_blank" rel="noreferrer">
          {part.text}
        </a>
        <span className="guard-host">{` ${part.out}`}</span>
      </span>
    );
  });
}

/** An authority's name: a link to its ledger's page for the guard where the project names one. */
function Authority({ row }: { row: Row }) {
  const { name, page, adapter, accepted } = row.authority;
  const linked =
    page === null ? null : webAddress(page) ? (
      <a className="guard-out" href={page} target="_blank" rel="noreferrer">
        {name}
      </a>
    ) : appPath(page) ? (
      <a href={href(page)}>{name}</a>
    ) : null;
  return (
    <span className="guard-by">
      <span>
        {linked ?? name}
        {adapter !== name && ` · ${adapter}`}
      </span>
      <span className="guard-accepted">{accepted}</span>
    </span>
  );
}

export interface GuardRowsProps {
  /** What the rows are of, said to a screen reader and named by the browser tests. */
  label: string;
  /** The box's heading. */
  heading: ReactNode;
  rows: Row[];
  /** Who decides, under the rows. */
  foot?: string;
  /** The rows' key on the page: an edge's ID, or `entry`. */
  of: string;
}

/**
 * A box of guards' rows. A row's name is a button that opens, under the row, what the board holds
 * for the guard, and closes it again.
 */
export function GuardRows(props: GuardRowsProps) {
  const [open, setOpen] = useState("");
  return <GuardRowsView {...props} open={open} onOpen={setOpen} />;
}

export interface GuardRowsViewProps extends GuardRowsProps {
  /** The guard whose record is open; empty for none. */
  open: string;
  onOpen: (guard: string) => void;
}

/** The box as drawn, with the row whose record is open. */
export function GuardRowsView({
  label,
  heading,
  rows,
  foot,
  of,
  open,
  onOpen,
}: GuardRowsViewProps) {
  if (rows.length === 0) return null;
  return (
    <section className="guard-box" aria-label={label} data-guards={of}>
      <p className="guard-head">{heading}</p>
      <ul className="guards">
        {rows.map((row) => {
          const id = `guard-record-${of}-${row.guard}`;
          const shown = open === row.guard;
          return (
            <li
              key={row.guard}
              className={`guard guard-${row.tone}`}
              data-guard={row.guard}
              data-state={row.state}
            >
              <span className="guard-name">
                <button
                  type="button"
                  className="guard-open"
                  aria-expanded={shown}
                  aria-controls={id}
                  title="The board's record of this guard"
                  onClick={() => onOpen(shown ? "" : row.guard)}
                >
                  {row.name}
                </button>
              </span>
              <span className="pill guard-state">{row.state}</span>
              <span className="guard-why" dir="auto">
                <Parts parts={row.why} />
              </span>
              <Authority row={row} />
              {shown && (
                <div className="guard-record" id={id}>
                  <p className="guard-record-title">{row.record.title}</p>
                  <dl>
                    {row.record.lines.map((line) => (
                      <div key={line.label}>
                        <dt>{line.label}</dt>
                        <dd dir="auto">
                          <Parts parts={line.parts} />
                        </dd>
                      </div>
                    ))}
                  </dl>
                </div>
              )}
            </li>
          );
        })}
      </ul>
      {foot !== undefined && <p className="guard-foot">{foot}</p>}
    </section>
  );
}

/** Who decides the guards of `rows`, and that nothing here does. */
export function footWords(rows: Row[], action: string): string {
  const names = [...new Set(rows.map((row) => row.authority.name))].join(" and ");
  const [decide, its] =
    rows.length > 1 && names.includes(" and ") ? ["decide", "their"] : ["decides", "its"];
  return `${names} ${decide} this in the project's own ledger. ${action} passes on ${its} signed word, which the board checks; nothing on this page approves.`;
}
