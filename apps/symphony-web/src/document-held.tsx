// The comments held for people on a document (docs/product/analysis/users-and-identity.md, "Words
// without agent reach"): a callout or a card's message from a person who may not direct agents,
// kept on the board until someone who may passes it on. The page reads them with itself, again as
// its feed says one was held or passed on (`held`, document-live.ts), and again as its document's
// copy moves while it holds any. It draws each where the server says its pass writes it, after the
// block it was left after or, that block changed, at the end of its section: a callout under the
// section of its key, else under the section that ends on its line, else at the document's end; a
// card's message in its card. Each says who wrote it, its kind, "held" and its words, and for one
// who may pass it on (`passes`) Pass on, which writes it into the file as its author's; the next
// read leaves it out.

import { createContext, useContext, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "./button";
import { api } from "./document-live";
import { useMount } from "./mount";
import { scoped } from "./project";
import { getJson, post, said } from "./request";
import { field, STALE, useStore } from "./shared";
import type { DocumentHeld, HeldComment, Section } from "./wire";
import { DOCUMENT_HELD_PATH, DOCUMENT_PASS_ROUTE } from "./wire";
import { CHECKING } from "./words";

const isText = (value: unknown): value is string => typeof value === "string";
const isCount = (value: unknown): value is number =>
  typeof value === "number" && Number.isSafeInteger(value);
/** An optional field as serde reads one: absent, null or of its type. */
const optional = (value: unknown, is: (value: unknown) => boolean) =>
  value === undefined || value === null || is(value);

/** One held comment as the server sent it, or none where a field the page draws is not one. */
export function readHeldComment(value: unknown): HeldComment | null {
  const [id, author, kind, text] = ["id", "author", "kind", "text"].map((key) => field(value, key));
  const [card, key, line] = ["card", "key", "line"].map((name) => field(value, name));
  if (!isCount(id) || !isText(author) || !isText(kind) || !isText(text)) return null;
  if (!optional(card, isText) || !optional(key, isText) || !optional(line, isCount)) return null;
  return {
    id,
    author,
    kind,
    text,
    card: (card as string | null | undefined) ?? null,
    key: (key as string | null | undefined) ?? null,
    line: (line as number | null | undefined) ?? null,
  };
}

/** A document's held comments as the server sent them, or none where it sent no such read. */
export function readHeld(value: unknown): DocumentHeld | null {
  const held = field(value, "held");
  const passes = field(value, "passes");
  if (!Array.isArray(held) || typeof passes !== "boolean") return null;
  const read = held.map(readHeldComment);
  if (read.some((comment) => comment === null)) return null;
  return { held: read as HeldComment[], passes };
}

/** The key a card's section has on the page. */
const cardKey = (slug: string) => `card:${slug}`;

/**
 * Where each of `held` is drawn among `sections`, by the key of the section it goes under: a
 * card's message under its card's, a callout under the section of its key, else the one that ends
 * on its line, else under `""`, the document's end. Each list keeps the order the server sent.
 */
export function placeHeld(
  held: readonly HeldComment[],
  sections: readonly Section[],
): Map<string, HeldComment[]> {
  const keys = new Set(sections.map((section) => section.key));
  const placed = new Map<string, HeldComment[]>();
  for (const comment of held) {
    let key = "";
    if (comment.card != null) {
      key = keys.has(cardKey(comment.card)) ? cardKey(comment.card) : "";
    } else if (comment.key != null && keys.has(comment.key)) {
      key = comment.key;
    } else {
      key = sections.find((section) => section.end === comment.line)?.key ?? "";
    }
    placed.set(key, [...(placed.get(key) ?? []), comment]);
  }
  return placed;
}

/** What a document's page holds of its held comments, for each section to draw its own. */
export interface HeldValue {
  /** By the key of the section each goes under, `""` the document's end ([`placeHeld`]). */
  placed: Map<string, HeldComment[]>;
  passes: boolean;
  /** Pass the comment `id` on: the page reads its held comments again once it answers. */
  pass: (id: number) => Promise<void>;
}

export const HeldContext = createContext<HeldValue | null>(null);

/**
 * The copy of a document the held comments read at its copy `asked` stand for once it is at
 * `version`, and whether they are read again: where they hold any and the copy moved past that
 * one, which moves where a pass writes them. A read asked before the copy was known (`null`)
 * stands for the first one known.
 */
export function heldCopy(
  asked: number | null,
  version: number | null,
  holding: boolean,
): [number | null, boolean] {
  const at = asked ?? version;
  return [at, holding && version !== null && version !== at];
}

/**
 * The held comments of the document `path` of the checkout `scope`, placed among its `sections`:
 * read at once, again each time `moved` (the feed's count of held comments moved) or `refreshed`
 * (the frame's) changes, after a pass from this page, and as the document's copy `version` moves
 * past the one the last read was asked at while any is held ([`heldCopy`]). None until read, and
 * none where nothing is held. A failed read draws none; the next move reads it again.
 */
export function useHeld(
  path: string,
  scope: string,
  sections: readonly Section[] | null,
  version: number | null,
  moved: number,
  refreshed: unknown,
): HeldValue | null {
  const [read, setRead] = useState<DocumentHeld | null>(null);
  const [passed, setPassed] = useState(0);
  const [copies, setCopies] = useState(0);
  const holding = (read?.held.length ?? 0) > 0;
  const seen = useRef(version);
  const asked = useRef<number | null>(null);
  useEffect(() => {
    seen.current = version;
    const [at, again] = heldCopy(asked.current, version, holding);
    asked.current = at;
    if (again) setCopies((count) => count + 1);
  }, [version, holding]);
  useEffect(() => {
    void moved;
    void refreshed;
    void passed;
    void copies;
    asked.current = seen.current;
    const reading = new AbortController();
    getJson(api(scope, DOCUMENT_HELD_PATH, path), reading.signal).then(
      (value) => {
        if (reading.signal.aborted) return;
        const held = readHeld(value);
        if (held !== null) setRead(held);
      },
      () => {},
    );
    return () => reading.abort();
  }, [path, scope, moved, refreshed, passed, copies]);
  return useMemo(() => {
    if (read === null || read.held.length === 0 || sections === null) return null;
    return {
      placed: placeHeld(read.held, sections),
      passes: read.passes,
      pass: async (id: number) => {
        try {
          await post(scoped(scope, DOCUMENT_PASS_ROUTE), { id });
        } finally {
          setPassed((count) => count + 1);
        }
      },
    };
  }, [read, sections, scope]);
}

/** The held comments that go under the section `sectionKey` (`""`: the document's end). */
export function HeldUnder({ sectionKey }: { sectionKey: string }) {
  const held = useContext(HeldContext);
  const comments = held?.placed.get(sectionKey) ?? null;
  if (held === null || comments === null) return null;
  return (
    <div className="held-comments">
      {comments.map((comment) => (
        <HeldOne key={comment.id} comment={comment} passes={held.passes} pass={held.pass} />
      ))}
    </div>
  );
}

/**
 * Pass the held comment `id` on with `pass`: what its box says then, nothing once passed (the next
 * read leaves it out and it is drawn no more), else the refusal in the server's words.
 */
export async function passOn(id: number, pass: (id: number) => Promise<void>): Promise<string> {
  try {
    await pass(id);
    return "";
  } catch (error) {
    return said(error);
  }
}

/** One held comment: who wrote it, its kind, "held", its words, and Pass on where it may be. */
export function HeldOne({
  comment,
  passes,
  pass,
}: {
  comment: HeldComment;
  passes: boolean;
  pass: (id: number) => Promise<void>;
}) {
  const mount = useMount();
  const working = useRef(false);
  const [busy, setBusy] = useState(false);
  const [told, setTold] = useState("");
  const stale = useStore(STALE);
  const press = async () => {
    if (working.current || STALE.get()) return;
    working.current = true;
    setBusy(true);
    setTold("");
    const refused = await passOn(comment.id, pass);
    if (mount.aborted || refused === "") return;
    setTold(refused);
    working.current = false;
    setBusy(false);
  };
  return (
    <div className="held-comment" id={`held-${comment.id}`} data-kind={comment.kind}>
      <div className="held-head">
        <b>{comment.author}</b>
        {comment.kind !== "message" && <span className="held-kind">{comment.kind}</span>}
        <b className="entry-held">held</b>
      </div>
      <div className="held-text literal">{comment.text}</div>
      {passes && (
        <div className="row">
          <Button size="xs" disabled={busy || stale} onClick={() => void press()}>
            {stale ? CHECKING : "Pass on"}
          </Button>
        </div>
      )}
      {told !== "" && <p role="status">{told}</p>}
    </div>
  );
}
