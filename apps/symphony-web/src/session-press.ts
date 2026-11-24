// What a press of an agent's Go to session control does (session.tsx), a chunk of its own: imported
// once a control is first pressed, so a page whose owner presses none carries none of it. On the
// server's machine it has the Herdr app show the pane; anywhere else it reads the pane's screen. It
// imports nothing of a kind's own: session.tsx is the agents page's and a task's.

import { scoped } from "./project";
import { getJson, post, said } from "./request";
import type { Going } from "./session";
import type { SessionControl } from "./session-control";
import { field } from "./shared";
import { AGENT_FOCUS_PATH, AGENT_SCREEN_PATH, type AgentScreen } from "./wire";

/** A screen as the server answers one, or none for a value without what it needs. */
export function readScreen(value: unknown): AgentScreen | null {
  const handle = field(value, "handle");
  const pane = field(value, "pane");
  const text = field(value, "text");
  const readAt = field(value, "read_at");
  return typeof handle === "string" &&
    typeof pane === "string" &&
    typeof text === "string" &&
    typeof readAt === "number"
    ? { handle, pane, text, read_at: readAt }
    : null;
}

/** Have the server's machine show `handle`'s session in the Herdr app: what it said it did. */
export async function focus(handle: string, scope: string): Promise<string> {
  try {
    const answer = await post(scoped(scope, AGENT_FOCUS_PATH), { handle });
    const told = field(answer, "said");
    return typeof told === "string" ? told : "missing field `said`";
  } catch (failure) {
    return said(failure);
  }
}

/** The screen of `handle`'s session's pane, read now, or what failed; given up once `signal` ends. */
export async function screen(
  handle: string,
  scope: string,
  signal: AbortSignal,
): Promise<AgentScreen | string> {
  try {
    const path = `${AGENT_SCREEN_PATH}?handle=${encodeURIComponent(handle)}`;
    return readScreen(await getJson(scoped(scope, path), signal)) ?? "The screen is not readable";
  } catch (failure) {
    return said(failure);
  }
}

/**
 * A press of `control`'s button for `handle`'s session: nothing while one is on its way (`busy`) or
 * for a link, which the browser follows. Else a focus, saying what the server did or what failed;
 * or the screen read, shown, or what failed said beside the one shown before. Nothing more once
 * `mount` ended while the server answered.
 */
export async function press(
  control: SessionControl,
  handle: string,
  scope: string,
  busy: boolean,
  mount: AbortSignal,
  going: Going,
): Promise<void> {
  if (busy || control.kind === "link") return;
  going.busy(true);
  going.said("");
  if (control.kind === "focus") {
    const told = await focus(handle, scope);
    if (mount.aborted) return;
    going.said(told);
  } else {
    const read = await screen(handle, scope, mount);
    if (mount.aborted) return;
    if (typeof read === "string") going.said(read);
    else going.screen(read);
  }
  going.busy(false);
}
