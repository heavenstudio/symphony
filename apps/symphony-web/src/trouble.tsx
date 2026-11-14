// The one block a page shows in place of what it could not show: a title, a plain sentence, the
// way back and a retry. A page tells the two cases apart: the server has no such thing (a 404), or
// the read did not finish: the server could not be reached, or it answered with an error of its
// own, which the block says with its status. The page's notices leave it out while it stands.

import { Button } from "./button";
import { href } from "./project";

/** What a read fails with when no answer came at all: the fetch itself failed. */
export const UNREACHABLE = "The server could not be reached";

/**
 * What a read fails with when the server answered `status`: its own `error`, when it gave one,
 * then the status, so a page says what the server said rather than blaming the connection.
 */
export function answered(status: number, error?: string): string {
  const own = error?.trim().replace(/\.+$/, "") ?? "";
  return own === "" ? `The server answered ${status}` : `${own} (the server answered ${status})`;
}

/**
 * The sentence for a read that did not finish, from the reason the transport gave, said once.
 * Only a server that could not be reached sends the reader to the connection.
 */
export function failed(why: string): string {
  const reason = why.trim().replace(/\.+$/, "");
  if (reason === "") return "The read did not finish. Try again.";
  if (reason === UNREACHABLE) return `${UNREACHABLE}. Check the connection and try again.`;
  return `${reason}.`;
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
  /** Reads again; a page that retries on its own still offers it. */
  retry?: () => void;
}

/** What a page shows when its read did not bring what it names. */
export function Trouble({ title, text, back, retry }: TroubleProps) {
  return (
    <section className="trouble card" role="status">
      <h1>{title}</h1>
      <p>{text}</p>
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
