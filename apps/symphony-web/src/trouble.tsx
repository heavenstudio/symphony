// The one block a page shows in place of what it could not show: a title, a plain sentence, the
// way back and a retry. A page tells the two cases apart: the server has no such thing (a 404), or
// the read did not finish: the server could not be reached, or it answered with an error of its
// own, which the block says with its status. The page's notices leave it out while it stands.

import type { ReactNode } from "react";
import { Button } from "./button";
import { href } from "./project";

/** What a read fails with when no answer came at all: the fetch itself failed. */
export const UNREACHABLE = "The server could not be reached";
/** What a read fails with when the server did not answer within the limit (request.ts). Nothing
 * was sent to act on: a write that got no answer says its own words. */
export const NOT_IN_TIME = "The server did not answer in time";
/** What a page that tries a failed read again by itself says after the reason. */
export const TRYING = "Trying again…";

/**
 * What a read fails with when the server answered `status`: its own `error`, when it gave one,
 * then the status, so a page says what the server said rather than blaming the connection.
 */
export function answered(status: number, error?: string): string {
  const own = error?.trim().replace(/\.+$/, "") ?? "";
  return own === "" ? `The server answered ${status}` : `${own} (the server answered ${status})`;
}

/**
 * The sentence for a read that did not finish, from the reason the transport gave, said once:
 * every page's failed read says it here. A page that tries the read again by itself (`trying`)
 * says so and asks nothing of the reader; one that does not asks them to try again. Only a
 * server that could not be reached sends the reader to the connection.
 */
export function failed(why: string, trying = false): string {
  // A reason that says it goes on by itself (the board's own notice, a sentence said here before)
  // says so once.
  if (trying && why.trimEnd().endsWith("…")) return why.trim();
  const reason = why.trim().replace(/[.…]+$/, "");
  if (reason === "") return `The read did not finish. ${trying ? TRYING : "Try again."}`;
  if (reason === UNREACHABLE) {
    return trying
      ? `${UNREACHABLE}; check the connection. ${TRYING}`
      : `${UNREACHABLE}. Check the connection and try again.`;
  }
  return trying ? `${reason}. ${TRYING}` : `${reason}.`;
}

/**
 * A region's `data-ready` while it has nothing of its own to show: `missing` when the server has
 * no such thing, `failed` when its read did not bring it, else `loading`. The first two are final.
 */
export function ready(missing: boolean, failing: boolean): string {
  if (missing) return "missing";
  return failing ? "failed" : "loading";
}

export interface TroubleProps {
  title: string;
  text: string;
  /** Where to go instead, as a path of the site and its label. */
  back?: [string, string];
  /** Reads again at once; a page that tries again on its own still offers it. */
  retry?: () => void;
  /** What the page offers after its sentence, before the retry and the way back. */
  children?: ReactNode;
}

/** What a page shows when its read did not bring what it names. */
export function Trouble({ title, text, back, retry, children }: TroubleProps) {
  return (
    <section className="trouble card" role="status">
      <h1>{title}</h1>
      <p>{text}</p>
      {children}
      <p className="trouble-actions">
        {retry && (
          <Button type="button" onClick={retry}>
            Retry
          </Button>
        )}
        {back && (
          <a className="trouble-back" href={href(back[0])}>
            <span aria-hidden="true">← </span>
            {back[1]}
          </a>
        )}
      </p>
    </section>
  );
}
