// The check a read that carries blocks passes where it arrives (a task's brief and messages, a
// document's sections, the chat's answers): every field the block components draw, each of its
// type, and nothing else, as Rust's serde reads them. A field a newer server adds changes nothing,
// and neither does a kind of block, span or media it adds (a minor of the plugin kit,
// docs/engineering/PLUGIN_PROTOCOL.md): it passes with its fields unread, and the components say it where it
// stands as content this client cannot draw. A word the client does not know (a callout's kind, a
// figure's size, a tag, a shape) fails the whole read, as does a kind that is no text, and the
// page says so and reads again. Each table of kinds and words names every one the wire types know,
// which the type check holds. The checks it is made of are the other reads' too
// (`document-read.ts`).

import type {
  Block,
  CalloutKind,
  CardGroup,
  CardState,
  ContainerKind,
  FigureSize,
  InlineKind,
  Media,
  Span,
  ThreadAuthor,
  VectorKind,
} from "./wire";

export type Check = (value: unknown) => boolean;

export const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);
export const isText: Check = (value) => typeof value === "string";
export const isFlag: Check = (value) => typeof value === "boolean";
/**
 * A whole number of at least 0 and at most `most`, as the Dioxus client's serde-wasm-bindgen reads
 * an unsigned field: a safe integer (none past 2^53 - 1, which it refuses as it cannot tell them
 * apart), within the type's range.
 */
export const unsigned =
  (most: number): Check =>
  (value) =>
    Number.isSafeInteger(value) && (value as number) >= 0 && (value as number) <= most;
/** A Rust `u8`. */
export const isByte = unsigned(255);
/** A Rust `u64`. */
export const isCount = unsigned(Number.MAX_SAFE_INTEGER);
/** A field Rust reads as an `Option`: absent or null, or the value. */
export const maybe =
  (check: Check): Check =>
  (value) =>
    value == null || check(value);
/** A field Rust reads with its default when it is absent. */
export const absentOr =
  (check: Check): Check =>
  (value) =>
    value === undefined || check(value);
export const listOf =
  (check: Check): Check =>
  (value) =>
    Array.isArray(value) && value.every(check);
export const oneOf =
  (words: Record<string, true>): Check =>
  (value) =>
    typeof value === "string" && Object.hasOwn(words, value);
export const shaped =
  (fields: Record<string, Check>): Check =>
  (value) =>
    isRecord(value) && Object.entries(fields).every(([name, check]) => check(value[name]));
/** A map Rust reads by its keys, each value of `check`. */
export const mapOf =
  (check: Check): Check =>
  (value) =>
    isRecord(value) && Object.values(value).every(check);

const CARD_STATES: Record<CardState, true> = {
  pending: true,
  approved: true,
  "auto-approved": true,
  rejected: true,
  question: true,
  resolved: true,
  dismissed: true,
};
const CARD_GROUPS: Record<CardGroup, true> = {
  decide: true,
  auto: true,
  confirm: true,
  agent: true,
  rejected: true,
  resolved: true,
};
const AUTHORS: Record<ThreadAuthor, true> = { owner: true, agent: true, unknown: true };
const CALLOUTS: Record<CalloutKind, true> = {
  comment: true,
  like: true,
  dislike: true,
  question: true,
  decision: true,
  resolved: true,
};
const SIZES: Record<FigureSize, true> = {
  small: true,
  row: true,
  large: true,
  tile: true,
  cell: true,
  slot: true,
  wide: true,
};
const CONTAINERS: Record<ContainerKind, true> = {
  div: true,
  section: true,
  figure: true,
  figcaption: true,
  aside: true,
  header: true,
  footer: true,
  address: true,
  center: true,
};
const INLINES: Record<InlineKind, true> = {
  span: true,
  small: true,
  sub: true,
  sup: true,
  time: true,
  mark: true,
  kbd: true,
  samp: true,
  delete: true,
  insert: true,
  underline: true,
  abbreviation: true,
};
const VECTORS: Record<VectorKind, true> = {
  path: true,
  circle: true,
  ellipse: true,
  rect: true,
  line: true,
  polyline: true,
  polygon: true,
};

const isTextMap = mapOf(isText);
const isPair: Check = (value) => Array.isArray(value) && value.length === 2 && value.every(isText);
const isPresentation = shaped({
  class: absentOr(isText),
  id: absentOr(isText),
  title: absentOr(isText),
  attributes: absentOr(isTextMap),
});
const isImage = shaped({ src: isText, alt: isText, presentation: isPresentation });
const isShape = shaped({ kind: oneOf(VECTORS), attributes: isTextMap });
const isTarget = shaped({ path: isText, anchor: maybe(isText) });
const isView = shaped({
  label: isText,
  title: maybe(isText),
  set: listOf(isPair),
  call: maybe(isText),
});
const isViewer = shaped({
  element: isText,
  script: isText,
  attributes: listOf(isPair),
  views: absentOr(listOf(isView)),
  download: maybe(isText),
  open_label: maybe(isText),
  close_label: maybe(isText),
});
const isItem = shaped({ name: isText, src: isText, poster: maybe(isText) });
const blocks: Check = (value) => Array.isArray(value) && value.every(isBlock);
/** A list of blocks, each of a kind with its fields. */
export const isBlocks = blocks;
const spans: Check = (value) => Array.isArray(value) && value.every(isSpan);
const isCell = shaped({ header: isFlag, spans, presentation: isPresentation });
const isEntry = shaped({
  who: oneOf(AUTHORS),
  timestamp: maybe(isText),
  verdict: maybe(isText),
  blocks,
});
const isVerdict = shaped({
  group: oneOf(CARD_GROUPS),
  open: isFlag,
  posted: maybe(isText),
  decided: maybe(isText),
});

/** The fields each media kind draws. */
const MEDIA: Record<Media["kind"], Record<string, Check>> = {
  mockup: { src: isText, label: isText },
  viewer: { src: isText, label: isText, poster: maybe(isText), viewer: isViewer },
  gallery: { items: listOf(isItem), viewer: absentOr(isViewer) },
};

/** The fields each block kind draws: a card's source line and a comment's words are not drawn. */
const BLOCKS: Record<Block["type"], Record<string, Check>> = {
  heading: { level: isByte, anchor: isText, spans },
  paragraph: { spans },
  list: { ordered: isFlag, start: isCount, items: listOf(blocks) },
  table: {
    head: listOf(isCell),
    rows: listOf(listOf(isCell)),
    presentation: absentOr(isPresentation),
  },
  code: { language: isText, text: isText },
  quote: { blocks },
  rule: {},
  callout: { kind: oneOf(CALLOUTS), title: spans, resolved: isFlag, blocks },
  card: {
    slug: isText,
    mark: oneOf(CARD_STATES),
    title: isText,
    body: blocks,
    verdict: isVerdict,
    thread: listOf(isEntry),
    comparison: blocks,
  },
  component: { kind: isText, blocks },
  media: { media: isMedia },
  image: { image: isImage },
  figure: {
    picture: maybe(isImage),
    caption: spans,
    href: maybe(isText),
    badge: maybe(isText),
    size: oneOf(SIZES),
  },
  caption: { blocks },
  comparison: { figures: blocks },
  group: { blocks },
  container: { tag: oneOf(CONTAINERS), presentation: isPresentation, blocks },
  details: { summary: spans, open: isFlag, blocks },
  "settled-card": { summary: spans, open: isFlag, blocks },
  thread: { entries: listOf(isEntry) },
  comment: {},
  unknown: { tag: isText, text: isText },
};

/** The fields each span kind draws. */
const SPANS: Record<Span["type"], Record<string, Check>> = {
  text: { text: isText },
  emphasis: { spans },
  strong: { spans },
  code: { text: isText },
  link: { href: isText, target: maybe(isTarget), title: isText, spans },
  "wiki-link": { href: isText, target: maybe(isTarget), spans },
  "line-break": {},
  image: { image: isImage },
  media: { media: isMedia },
  component: { kind: isText, blocks },
  icon: { shape: listOf(isShape), label: isText, named: isFlag, tip: absentOr(isText) },
  styled: { tag: oneOf(INLINES), presentation: isPresentation, spans },
  graphic: { view_box: isText, presentation: isPresentation, shapes: listOf(isShape) },
  blocks: { blocks },
  unknown: { tag: isText, text: isText },
};

/**
 * Whether `value` is tagged with a kind by its `tag` field: one `kinds` names, with its fields, or
 * one it does not name, a later kind this client draws as unsupported, whatever its fields.
 */
function tagged(
  value: unknown,
  tag: string,
  kinds: Record<string, Record<string, Check>>,
): boolean {
  if (!isRecord(value)) return false;
  const kind = value[tag];
  if (typeof kind !== "string") return false;
  const fields = Object.hasOwn(kinds, kind) ? kinds[kind] : undefined;
  return fields === undefined || shaped(fields)(value);
}

export function isMedia(value: unknown): value is Media {
  return tagged(value, "kind", MEDIA);
}

export function isBlock(value: unknown): value is Block {
  return tagged(value, "type", BLOCKS);
}

export function isSpan(value: unknown): value is Span {
  return tagged(value, "type", SPANS);
}

/** `value` as blocks the components draw, or null when it is none. */
export function readBlocks(value: unknown): Block[] | null {
  return Array.isArray(value) && value.every(isBlock) ? (value as Block[]) : null;
}
