// A card's Later and its Undo, as the queue and a document's card draw them: Later puts the card
// after the rest of its group, or away for three days, and the reader goes on to the card the server
// names next; a document's card says the server the reader saw it, which ends its Later.

import { useEffect, useState } from "react";
import { Button } from "./button";
import { type FrameRefresh, useFrame } from "./frame";
import { useMount } from "./mount";
import { here, project, scoped } from "./project";
import { post, said } from "./request";
import { field, STALE, useStore } from "./shared";
import { CHECKING } from "./words";

/** A card waiting on the owner, as Later and its Undo name it. */
export interface Postponed {
  path: string;
  slug: string;
  postponed: boolean;
  /** The checkout whose card it is (`""`, the main one's, when none is named). */
  scope?: string;
}

/**
 * Later, or its Undo, on `card`: once the server takes it, the frame reads again and, after Later,
 * the reader goes (`go`) to the card the server names next. Nothing of that once `mount` is
 * aborted, the page gone while the server answered. What failed, said; none when it went well.
 */
export async function postpone(
  card: Postponed,
  mount: AbortSignal,
  frame: FrameRefresh,
  go: (address: string) => void,
): Promise<string | null> {
  try {
    const reply = await post(scoped(card.scope ?? "", "/api/postpone"), {
      path: card.path,
      slug: card.slug,
      action: card.postponed ? "undo" : "later",
    });
    if (mount.aborted) return null;
    frame.refresh();
    const next = field(reply, "next");
    if (!card.postponed && typeof next === "string") go(here(project(), next));
    return null;
  } catch (failure) {
    return said(failure);
  }
}

/**
 * Say the reader saw `card`, so its Later ends: the frame reads again when the server says that
 * changed anything. What failed, said; none when it went well or `signal` ended meanwhile.
 */
export async function seen(
  card: Postponed,
  signal: AbortSignal,
  frame: FrameRefresh,
): Promise<string | null> {
  try {
    const reply = await post(scoped(card.scope ?? "", "/api/postpone"), {
      path: card.path,
      slug: card.slug,
      action: "seen",
    });
    if (!signal.aborted && field(reply, "changed") === true) frame.refresh();
    return null;
  } catch (failure) {
    return signal.aborted ? null : said(failure);
  }
}

export interface PostponeProps extends Postponed {
  /**
   * What the reader is doing with the card on its document's page (the address's anchor and
   * whether its editor is open) while they are with it: each new one says the card was seen.
   */
  seenAs?: string | null;
}

/** The Later and Undo controls of a card waiting on the owner. */
export function PostponeControls({
  path,
  slug,
  postponed,
  scope = "",
  seenAs = null,
}: PostponeProps) {
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const frame = useFrame();
  const mount = useMount();
  // Each saying runs to its answer, as long as the page's kind is mounted: a later one never
  // drops the frame's read an earlier one asked for.
  useEffect(() => {
    if (seenAs === null) return;
    void seen({ path, slug, postponed: false, scope }, mount, frame).then((failure) => {
      if (failure !== null) setError(failure);
    });
  }, [seenAs, path, slug, scope, frame, mount]);
  // While the page may show what is out of date, the card may have moved since: wait for it.
  const checking = useStore(STALE);
  const label = pending ? "Saving…" : checking ? CHECKING : postponed ? "Undo Later" : "Later";
  const act = async () => {
    if (pending || STALE.get()) return;
    setPending(true);
    setError(null);
    const failure = await postpone({ path, slug, postponed, scope }, mount, frame, (address) => {
      location.href = address;
    });
    if (mount.aborted) return;
    setError(failure);
    setPending(false);
  };
  return (
    <span className="postpone-controls">
      {postponed && <small className="postponed">postponed</small>}
      <Button
        type="button"
        disabled={pending || checking}
        title="Return after the rest of this group, or in 3 days"
        onClick={() => void act()}
      >
        {label}
      </Button>
      {error !== null && <span role="alert">{error}</span>}
    </span>
  );
}
