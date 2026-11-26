// Recorded metadata only: no attribution from today's role holders, and no message bodies.
import { field } from "./shared";
import type { History, MessageEndpoint, TaskEntry, TimelineMessage } from "./wire";

const number = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);
const text = (v: unknown): v is string => typeof v === "string";
const optional = (v: unknown) => v === null || number(v);
const list = (v: unknown, check: (v: unknown) => boolean) => Array.isArray(v) && v.every(check);
const candidate = (v: unknown) => ["role", "by", "source"].every((k) => text(field(v, k)));
const endpoint = (v: unknown) => text(field(v, "state")) && list(field(v, "candidates"), candidate);

/** Fail unreadable metadata as a read, preserve future endpoint/source states as unknown. */
export function readHistory(value: unknown): History | null {
  if (
    typeof field(value, "sources_complete") !== "boolean" ||
    !list(
      field(value, "sources"),
      (v) =>
        text(field(v, "slug")) &&
        text(field(v, "state")) &&
        ["first_id", "last_id", "before"].every((k) => optional(field(v, k))),
    ) ||
    !list(
      field(value, "roles"),
      (v) =>
        candidate(v) &&
        number(field(v, "from")) &&
        number(field(v, "from_id")) &&
        optional(field(v, "to")) &&
        optional(field(v, "to_id")) &&
        typeof field(v, "uncertain") === "boolean",
    ) ||
    !list(
      field(value, "messages"),
      (v) =>
        number(field(v, "id")) &&
        number(field(v, "at")) &&
        text(field(v, "sender")) &&
        (field(v, "recipient") === null || text(field(v, "recipient"))) &&
        text(field(v, "kind")) &&
        endpoint(field(v, "from")) &&
        endpoint(field(v, "to")) &&
        (field(v, "after_close") === null || typeof field(v, "after_close") === "boolean"),
    )
  )
    return null;
  return value as History;
}

/** Earlier thread entries have no historical resolution unless the projection names them. */
export function messagesOf(
  history: History | null | undefined,
  entries: readonly TaskEntry[] = [],
): TimelineMessage[] {
  const messages = new Map((history?.messages ?? []).map((m) => [m.id, m]));
  for (const { message: m } of entries) {
    if (m.kind === "system" || messages.has(m.id)) continue;
    messages.set(m.id, {
      id: m.id,
      at: m.ts,
      sender: m.sender,
      recipient: m.recipient,
      kind: m.kind,
      from: { state: "unknown", candidates: [] },
      to: { state: m.recipient === null ? "everyone" : "unknown", candidates: [] },
      after_close: null,
    });
  }
  return [...messages.values()].sort((a, b) => a.at - b.at || a.id - b.id);
}

export function address(message: TimelineMessage): string {
  return `${message.sender} → ${message.recipient ?? "Everyone on task"}`;
}

/** A selected message as the hours line says it at its instant, `clock`: its number, its two ends,
 * its time and, for a verdict, that it is one: "#15396 · gilded-gecko-838 → @executor · 11:31 ·
 * verdict". */
export function addressAt(message: TimelineMessage, clock: string): string {
  const kind = message.kind === "verdict" ? " · verdict" : "";
  return `#${message.id} · ${address(message)} · ${clock}${kind}`;
}

/** Only a unique known role gets a role lane; ambiguity stays on Other. */
export function endpointLane(endpoint: MessageEndpoint, recorded: string | null): string {
  if (recorded !== null && ["@orchestrator", "@executor", "@reviewer"].includes(recorded)) {
    return recorded.slice(1);
  }
  const roles = [...new Set(endpoint.candidates.map((c) => c.role))];
  return endpoint.state === "known" &&
    roles.length === 1 &&
    ["orchestrator", "executor", "reviewer"].includes(roles[0] ?? "")
    ? (roles[0] as string)
    : "other";
}

export function endpointWords(endpoint: MessageEndpoint): string {
  const candidates = endpoint.candidates.map((c) => `${c.by}, ${c.role} on ${c.source}`).join("; ");
  switch (endpoint.state) {
    case "known":
      return `Holder at send time: ${candidates || "unknown"}.`;
    case "ambiguous":
      return `Ambiguous endpoint${candidates ? `: ${candidates}` : ""}.`;
    case "everyone":
      return "Everyone on task.";
    case "outside":
      return "Outside the recorded task roles.";
    default:
      return `Individual endpoint unknown${candidates ? `; possible: ${candidates}` : ""}.`;
  }
}

/** What the role history lacks, said over the messages; none for a whole one. */
export function historyWords(history: History | null | undefined): string | null {
  if (!history) return "Role and address history unavailable.";
  const incomplete = history.sources.filter((s) => s.state !== "complete");
  if (!history.sources_complete || incomplete.length > 0) {
    return `Incomplete role history; individual endpoints may be unknown.${incomplete.map((s) => ` ${s.slug}: ${s.state === "partial" ? "partial" : "unavailable"}.`).join("")}`;
  }
  return null;
}

/** Exact numeric time remains available even outside JavaScript's calendar range. */
export function isoTime(at: number): string | undefined {
  const date = new Date(at * 1000);
  return Number.isNaN(date.valueOf()) ? undefined : date.toISOString();
}
