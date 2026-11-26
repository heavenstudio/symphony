// The checks a document's or a folder's page reads pass where they arrive: the outline, a read of
// sections, the feed's change, a folder, the versions and a diff, the asks and the bodies this
// device kept. As the blocks' (`blocks-read.ts`), each checks the fields the page draws, each of
// its type as the Dioxus client's serde reads it, and nothing else: a field a newer server adds
// changes nothing, and one of another type fails the whole read. A field the server always writes
// is required, though serde would take it absent (a section's and a fragment's blocks).

import {
  absentOr,
  type Check,
  isBlocks,
  isByte,
  isCount,
  isFlag,
  isRecord,
  isText,
  listOf,
  mapOf,
  maybe,
  oneOf,
  shaped,
  unsigned,
} from "./blocks-read";
import type { KeptBody } from "./cache-document";
import type {
  Ask,
  Difference,
  DocumentChange,
  DocumentDiff,
  DocumentOutline,
  DocumentSections,
  Folder,
  Fragment,
  Version,
} from "./wire";

/** A Rust `usize` as the Dioxus client's wasm32 reads it: at most 2^32 - 1. */
const isSize = unsigned(0xffff_ffff);
/** A Rust `i64`, read from a safe integer alone. */
const isInteger: Check = (value) => Number.isSafeInteger(value);
const isTexts = listOf(isText);

const DIFFERENCES: Record<Difference, true> = { added: true, changed: true, removed: true };

const isCrumb = shaped({ name: isText, href: maybe(isText) });
const isHeading = shaped({ level: isByte, anchor: isText, text: isText });
const isFragment = shaped({ blocks: isBlocks });
/** A Rust `[String; 2]`: two texts, no more and no fewer. */
const isEnds: Check = (value) => isTexts(value) && (value as unknown[]).length === 2;
const isSection = shaped({
  start: isSize,
  end: isSize,
  key: isText,
  hash: isText,
  part: maybe(isSize),
  blocks: isBlocks,
  comparison: maybe(shaped({ kind: oneOf(DIFFERENCES), before: absentOr(isBlocks) })),
  ends: maybe(isEnds),
});
const isCard = shaped({ slug: isText, state: isText, text: isText, group: isText, open: isFlag });
const isTask = shaped({ slug: isText, title: isText, status: isText });
const isComparison = shaped({ task: isText });

const isDocument = shaped({
  version: isCount,
  path: isText,
  title: isText,
  waiting: isTexts,
  crumbs: listOf(isCrumb),
  headings: listOf(isHeading),
  sections: listOf(isSection),
  cards: listOf(isCard),
  threads: mapOf(isFragment),
  tasks: listOf(isTask),
  comparison: maybe(isComparison),
});

const isChange = shaped({
  path: isText,
  removed: absentOr(isFlag),
  sections: absentOr(listOf(isSection)),
  deleted: absentOr(isTexts),
  positions: absentOr(
    listOf(shaped({ key: isText, start: isSize, end: isSize, part: maybe(isSize) })),
  ),
  order: maybe(isTexts),
  threads: absentOr(mapOf(maybe(isFragment))),
  title: maybe(isText),
  waiting: maybe(isTexts),
  crumbs: maybe(listOf(isCrumb)),
  headings: maybe(listOf(isHeading)),
  cards: absentOr(listOf(shaped({ key: isText, value: maybe(isCard), removed: absentOr(isFlag) }))),
  tasks: maybe(listOf(isTask)),
  comparison: maybe(listOf(isComparison)),
});

const isFolder = shaped({
  path: isText,
  title: isText,
  crumbs: listOf(isCrumb),
  docs: isSize,
  intro: maybe(isDocument),
  entries: listOf(
    shaped({
      kind: isText,
      path: isText,
      title: isText,
      docs: isSize,
      waiting: isSize,
      summary: isText,
    }),
  ),
});

/** `value` as `T` when `check` takes it, else null. */
const read =
  <T>(check: Check) =>
  (value: unknown): T | null =>
    check(value) ? (value as T) : null;

/** `/api/document?view=outline`, or a first part read ahead of the visit (`view=first`). */
export const readOutline = read<DocumentOutline>(
  shaped({ document: isDocument, anchors: mapOf(isTexts), thread_hashes: absentOr(mapOf(isText)) }),
);

/** `/api/section?anchor=`: the section an anchor names, as a link's excerpt draws it. */
export const readFragment = read<Fragment>(isFragment);

/** The ids each section holds, by its key, as an outline and a feed's change name them. */
export const readAnchors = read<Record<string, string[]>>(mapOf(isTexts));

/** `/api/section`: the sections asked for, as a change at the document's version. */
export const readSections = read<DocumentSections>(shaped({ version: isCount, change: isChange }));

/** A feed event's change of a document. */
export const readChange = read<DocumentChange>(isChange);

/** `/api/folder?view=outline`. */
export const readFolder = read<Folder>(isFolder);

/** `/api/versions`: a document's commits, newest first. */
export function readVersions(value: unknown): Version[] | null {
  const versions = isRecord(value) ? value.versions : undefined;
  return listOf(shaped({ sha: isText, date: isText, subject: isText }))(versions)
    ? (versions as Version[])
    : null;
}

/** `/api/diff`: one commit's change to a document. */
export const readDiff = read<DocumentDiff>(shaped({ sha: isText, subject: isText, diff: isText }));

/** What the asks' panel draws of `/api/asks`: the asks, whom the chat goes to, and whether the one
 * reading may ask and follow an ask up. */
export interface Asks {
  asks: Ask[];
  recipient: string | null;
  mayAsk: boolean;
}

const isAsk = shaped({
  id: isInteger,
  anchor: isText,
  quote: isText,
  status: isText,
  messages: listOf(shaped({ seq: isInteger, by: isText, blocks: absentOr(isBlocks) })),
});

/**
 * `/api/asks`: its asks, and the chat's recipient, which the panel says only when it is a name, as
 * the Dioxus client reads it apart from them; and whether the one reading may ask (`may_ask`),
 * which a server that does not say it (an older one, which drew Ask for everyone) leaves true.
 */
export function readAsks(value: unknown): Asks | null {
  if (!isRecord(value) || !listOf(isAsk)(value.asks) || !absentOr(isFlag)(value.may_ask)) {
    return null;
  }
  const { asks, recipient, may_ask } = value;
  return {
    asks: asks as Ask[],
    recipient: typeof recipient === "string" ? recipient : null,
    mayAsk: may_ask !== false,
  };
}

const isBody = shaped({
  section: isSection,
  card: maybe(isCard),
  thread: maybe(isFragment),
});

/** The bodies this device kept of a document's sections, by key (`SECTIONS`). */
export const readKeptBodies = read<Record<string, KeptBody>>(
  mapOf(shaped({ validator: isText, body: isBody })),
);
