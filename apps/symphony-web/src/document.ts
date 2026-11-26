// A document's snapshots and the feed's keyed changes to them, as the Dioxus client's docs.rs holds
// them. Only a free section takes new markup; a section's source lines always advance, so a comment
// sent during an update still names its block. Each change gives new values and keeps the old ones
// it did not touch, so a section drawn from an unchanged value draws nothing again.

import { byline } from "./byline";
import type {
  Document,
  DocumentCard,
  DocumentChange,
  DocumentTask,
  Folder,
  Fragment,
  Section,
  Span,
} from "./wire";
import { VERDICT_TASKS } from "./wire";

/** The page's document: the copy shown, the feed's latest, and whether the server removed it. */
export interface DocumentState {
  readonly shown: Document;
  readonly latest: Document;
  readonly removed: boolean;
}

export function documentState(document: Document): DocumentState {
  return { shown: document, latest: document, removed: false };
}

/** `slug` of a card's section key (`card:SLUG`), or null for any other section. */
export function cardSlug(key: string): string | null {
  return key.startsWith("card:") ? key.slice("card:".length) : null;
}

/**
 * `record` with `key` holding `value`, in place: a key such as `__proto__` is one like any other,
 * as it is in the JSON the record was read from.
 */
export function put<T>(record: Record<string, T>, key: string, value: T): void {
  Object.defineProperty(record, key, {
    value,
    enumerable: true,
    writable: true,
    configurable: true,
  });
}

/** `items` sorted by `rank`; the sort is stable, so two of a rank keep their order. */
function ranked<T>(items: T[], rank: (item: T) => number): T[] {
  return items
    .map((item) => [rank(item), item] as const)
    .sort(([a], [b]) => a - b)
    .map(([, item]) => item);
}

/** Where `key` is in `keys`, past the end when it is not there. */
const place = (keys: readonly string[], key: string): number => {
  const at = keys.indexOf(key);
  return at < 0 ? keys.length : at;
};

/** The feed's `delta` applied to the latest copy; a change of another document changes nothing. */
export function change(state: DocumentState, delta: DocumentChange): DocumentState {
  const latest = state.latest;
  if (delta.path !== latest.path) return state;
  if (delta.removed === true) return { ...state, removed: true };
  const deleted = new Set(delta.deleted ?? []);
  let sections = latest.sections.filter((section) => !deleted.has(section.key));
  for (const section of delta.sections ?? []) {
    const at = sections.findIndex((old) => old.key === section.key);
    if (at < 0) sections.push(section);
    else sections[at] = section;
  }
  for (const { key, start, end, part } of delta.positions ?? []) {
    const at = sections.findIndex((section) => section.key === key);
    const section = sections[at];
    if (section !== undefined) sections[at] = { ...section, start, end, part };
  }
  const order = delta.order;
  if (order != null) sections = ranked(sections, (section) => place(order, section.key));
  const threads = { ...latest.threads };
  for (const [slug, fragment] of Object.entries(delta.threads ?? {})) {
    if (fragment === null) delete threads[slug];
    else put(threads, slug, fragment);
  }
  let cards = latest.cards;
  for (const card of delta.cards ?? []) {
    if (card.removed === true) cards = cards.filter((old) => old.slug !== card.key);
    const value = card.value;
    if (value != null) {
      const at = cards.findIndex((old) => old.slug === card.key);
      cards = at < 0 ? [...cards, value] : cards.with(at, value);
    }
  }
  const next: Document = { ...latest, sections, threads, cards };
  if (delta.title != null) next.title = delta.title;
  if (delta.waiting != null) next.waiting = delta.waiting;
  if (delta.crumbs != null) next.crumbs = delta.crumbs;
  if (delta.headings != null) next.headings = delta.headings;
  if (delta.tasks != null) next.tasks = delta.tasks;
  if (delta.comparison != null) next.comparison = delta.comparison[0] ?? null;
  return { ...state, latest: next };
}

const sameList = <T>(a: readonly T[], b: readonly T[]): boolean =>
  a.length === b.length && a.every((item, at) => item === b[at]);

/**
 * Whether `a` and `b` hold the same objects, field by field, compared by identity, not by value: a
 * copy rebuilt from equal values is another one, and a page draws again from it.
 */
function sameDocument(a: Document, b: Document): boolean {
  const threads = Object.keys(a.threads);
  return (
    a.version === b.version &&
    a.path === b.path &&
    a.title === b.title &&
    a.waiting === b.waiting &&
    a.crumbs === b.crumbs &&
    a.headings === b.headings &&
    a.tasks === b.tasks &&
    (a.comparison ?? null) === (b.comparison ?? null) &&
    sameList(a.sections, b.sections) &&
    sameList(a.cards, b.cards) &&
    threads.length === Object.keys(b.threads).length &&
    threads.every((slug) => Object.hasOwn(b.threads, slug) && a.threads[slug] === b.threads[slug])
  );
}

/**
 * Show the latest copy but the sections `busy` holds, which keep their shown markup, card and
 * thread, at their latest place in the source. While any is held, the shown order stays, so a
 * section the owner is in never moves under their pointer when the register regroups. A shown copy
 * whose fields would all be the same objects (`sameDocument`, by identity) stays the same object,
 * so the page draws nothing again.
 */
export function refresh(state: DocumentState, busy: ReadonlySet<string>): DocumentState {
  const { shown, latest } = state;
  const sections = [...latest.sections];
  const threads = { ...latest.threads };
  const cards = [...latest.cards];
  for (const old of shown.sections) {
    if (!busy.has(old.key)) continue;
    const at = sections.findIndex((section) => section.key === old.key);
    const section = sections[at];
    if (section === undefined) sections.push(old);
    else if (section.start === old.start && section.end === old.end && section.part === old.part)
      sections[at] = old;
    else sections[at] = { ...old, start: section.start, end: section.end, part: section.part };
    const slug = cardSlug(old.key);
    if (slug === null) continue;
    const thread = threadOf(shown, old.key);
    if (thread !== undefined) put(threads, slug, thread);
    else delete threads[slug];
    const card = shown.cards.find((one) => one.slug === slug);
    if (card !== undefined) {
      const target = cards.findIndex((one) => one.slug === slug);
      if (target < 0) cards.push(card);
      else cards[target] = card;
    }
  }
  const keys = shown.sections.map((section) => section.key);
  const next: Document = {
    ...latest,
    sections: busy.size === 0 ? sections : ranked(sections, (section) => place(keys, section.key)),
    threads,
    cards,
  };
  return sameDocument(shown, next) ? state : { ...state, shown: next };
}

/**
 * The sections whose shown markup and place stay as they are (`refresh`): those the owner is
 * `engaged` in (a field, an open form or excerpt), but for a card whose own save is `released`; a
 * section holding a closed draft only once it is gone from `latest`, so the draft can still be
 * copied; and the sections still `loading` their bodies.
 */
export function held(
  engaged: ReadonlySet<string>,
  drafts: ReadonlySet<string>,
  released: ReadonlyMap<string, number>,
  loading: Iterable<string>,
  latest: Document,
): Set<string> {
  const kept = new Set([...engaged].filter((key) => !released.has(key)));
  for (const key of drafts) {
    if (!latest.sections.some((section) => section.key === key)) kept.add(key);
  }
  for (const key of loading) kept.add(key);
  return kept;
}

/** The entries of `thread`'s thread blocks, in order. */
function entries(thread: Fragment | undefined) {
  return (thread?.blocks ?? []).flatMap((block) => (block.type === "thread" ? block.entries : []));
}

/** The thread of the card `key` (`card:SLUG`) in `document`, if it is a card's and has one. */
function threadOf(document: Document, key: string): Fragment | undefined {
  const slug = cardSlug(key);
  return slug !== null && Object.hasOwn(document.threads, slug)
    ? document.threads[slug]
    : undefined;
}

/** How many lines of a person's (the owner's, or a person's a board named) the card `key`
 * (`card:SLUG`) shows in `document`'s thread. */
export function ownerLines(document: Document, key: string): number {
  return entries(threadOf(document, key)).filter((entry) => entry.who === "owner").length;
}

/**
 * The owner's save in the card `key` went through: the card is released until it shows one line of
 * the owner's more than `before`, the lines its latest copy had when the save was sent
 * (`releasedFrom`), and more than a release of it still waiting asks for.
 */
export function release(released: Map<string, number>, key: string, before: number): void {
  const had = released.get(key);
  released.set(key, (had === undefined ? before : Math.max(before, had)) + 1);
}

/**
 * The lines of the owner's a card save's release waits past, if any. A save the server wrote now
 * waits past `before`, the card's lines when it was sent. A save the server answered as a `repeat`
 * of a tap it had written writes no line: it waits past the card's lines when that tap was first
 * sent, if it was this page's save that no reply answered (`retried`); else it releases nothing,
 * since `before` may already count the line the tap did write, and a release waiting for a line
 * that never comes would keep the card from holding its place for good.
 */
export function releasedFrom(
  repeat: boolean,
  before: number,
  retried: Unanswered | null,
): number | null {
  return repeat ? (retried?.before ?? null) : before;
}

/** What a save in a card says: its verdict (`message` for a message) and its note. */
export interface Words {
  readonly action: string;
  readonly note: string;
}

export function words(action: string, note: string): Words {
  return { action, note };
}

const sameWords = (a: Words, b: Words): boolean => a.action === b.action && a.note === b.note;

/**
 * A save in a card that no reply answered (the send gave up waiting, or the server's answer could
 * not be read): the server may have written it. The words sent under its tap, in the order last
 * sent (a retry under the tap may say other words, and the server holds whichever reached it
 * first, if any), the tap, and the lines of the owner's the card had when it was first sent.
 */
export class Unanswered {
  constructor(
    readonly sent: readonly Words[],
    readonly tap: string | null,
    readonly before: number,
  ) {}

  /**
   * The card `key` in `latest` shows a line of the owner's more than it had when this was first
   * sent: the save landed, and the feed, which brings every change, is its answer.
   */
  answered(latest: Document, key: string): boolean {
    return ownerLines(latest, key) > this.before;
  }

  /**
   * The card's unanswered save once `said`, sent under `tap` while the card had `before` lines of
   * the owner's, went unanswered too, after `earlier`. A retry under the earlier save's tap adds
   * its words to the earlier ones and still waits past the lines of the first sending, since one
   * tap writes at most one line, which may be an earlier one's; any other save starts anew.
   */
  static after(
    earlier: Unanswered | null,
    said: Words,
    tap: string | null,
    before: number,
  ): Unanswered {
    const retried = earlier?.retriedBy(tap) === true ? earlier : null;
    const sent = (retried?.sent ?? []).filter((old) => !sameWords(old, said));
    return new Unanswered([...sent, said], tap, retried?.before ?? before);
  }

  /**
   * Whether a save sent under `tap` retried this one: the server writes one verdict line for a
   * tap, so a reply to it answers this save too.
   */
  retriedBy(tap: string | null): boolean {
    return this.tap !== null && this.tap === tap;
  }

  /** The words last sent under the tap. */
  last(): Words | null {
    return this.sent.at(-1) ?? null;
  }

  /**
   * The words the server holds, once the card `key`'s latest copy has its line (`answered`): the
   * only words sent, else the one sending whose verdict and plain words the card's newest line of
   * the owner's shows. Null when the page cannot tell which: words with markup, or two sendings
   * that read alike.
   */
  held(latest: Document, key: string): Words | null {
    const [only, ...more] = this.sent;
    if (only !== undefined && more.length === 0) return only;
    const line = newestOwnerLine(latest, key);
    if (line === null) return null;
    const matching = this.sent.filter(
      (sent) => sent.action === line.verdict && collapse(sent.note) === line.text,
    );
    return matching.length === 1 ? (matching[0] ?? null) : null;
  }
}

/**
 * The verdict and the words, as plain text with its spaces collapsed, of the newest line of the
 * owner's on the card `key` in `document`; null without one, or when its words hold more than
 * plain text, emphasis, code and links.
 */
function newestOwnerLine(
  document: Document,
  key: string,
): { verdict: string | null; text: string } | null {
  const entry = entries(threadOf(document, key)).findLast((one) => one.who === "owner");
  if (entry === undefined) return null;
  let text = "";
  // A person's name, first of a named person's blocks, is no word of the line's.
  for (const block of byline(entry)[1]) {
    if (block.type !== "paragraph") return null;
    const words = plain(block.spans);
    if (words === null) return null;
    text += ` ${words}`;
  }
  return { verdict: entry.verdict ?? null, text: collapse(text) };
}

/** `spans` as plain text; null for anything but text, emphasis, code and links. */
function plain(spans: Span[]): string | null {
  let text = "";
  for (const span of spans) {
    switch (span.type) {
      case "text":
      case "code":
        text += span.text;
        break;
      case "emphasis":
      case "strong":
      case "link":
      case "wiki-link": {
        const inner = plain(span.spans);
        if (inner === null) return null;
        text += inner;
        break;
      }
      case "line-break":
        text += " ";
        break;
      default:
        return null;
    }
  }
  return text;
}

/** The characters Rust's `char::is_whitespace` names: Unicode's White_Space. */
const WHITE_SPACE = /\p{White_Space}+/u;

/** `text`'s words, one space between each, as Rust's `split_whitespace` and `join(" ")`. */
export function collapse(text: string): string {
  return text
    .split(WHITE_SPACE)
    .filter((word) => word !== "")
    .join(" ");
}

/** What the editor says once the server has `action` (a verdict, `message` or `comment`). */
export function savedText(action: string, kept = false): string {
  if (action === "message") return kept ? "Message held" : "Message sent";
  if (action === "comment") return kept ? "Comment held" : "Comment saved";
  return `Verdict saved: ${action}`;
}

/**
 * How a card's answered save leaves its editor: what it says, whether the box is emptied and
 * whether the form closes.
 */
export interface Ending {
  status: string;
  clear: boolean;
  close: boolean;
}

/**
 * How a card's save ends once answered, by `heldWords`, the words the server holds under its tap
 * (null when the page cannot tell), against `last`, the words last sent under it, and `boxed`,
 * what the box holds now. Only words the server holds are cleared or said to be saved. Words sent
 * and not held stay as the card's draft with the form closed, the save being answered; words the
 * owner has written since sending stay as they are, the form open. A comment or a message the
 * board `kept` for people says so.
 */
export function ending(heldWords: Words | null, last: Words, boxed: string, kept = false): Ending {
  const unchanged = boxed === last.note;
  if (heldWords === null) {
    return {
      status: "Verdict saved; the card shows its words: send yours again if they are not there",
      clear: false,
      close: unchanged,
    };
  }
  const saved = savedText(heldWords.action, kept);
  if (unchanged && sameWords(heldWords, last)) return { status: saved, clear: true, close: true };
  return {
    status: unchanged ? `${saved}; your change was not sent: send it again to add it` : saved,
    clear: false,
    close: unchanged,
  };
}

/**
 * The tap a verdict in a card goes under, if not a new one: the tap of the card's save that no
 * reply answered, whatever the words now say, so a retry after "no answer" never writes a second
 * verdict line; else the one `stored` for the card (`ID\nSIGNATURE`, kept until a reply answers it)
 * when it was for this same `signature`, a retry across a reload.
 */
export function tap(
  unanswered: Unanswered | null,
  stored: string | null,
  signature: string,
): string | null {
  if (unanswered?.tap != null) return unanswered.tap;
  if (stored === null) return null;
  const split = stored.indexOf("\n");
  if (split < 0) return null;
  const id = stored.slice(0, split);
  return stored.slice(split + 1) === signature && id !== "" ? id : null;
}

/**
 * The cards the owner just saved in, by how many lines of the owner's each must show: the owner's
 * answer shows at once, whatever holds the card. A release ends once the feed has brought the
 * save's own change and the card shows it, or once the card is gone; a reply an agent wrote before
 * it, shown on the way, ends nothing.
 */
export function settle(released: Map<string, number>, shown: Document): void {
  for (const [key, lines] of released) {
    const present = shown.sections.some((section) => section.key === key);
    if (!present || ownerLines(shown, key) >= lines) released.delete(key);
  }
}

/** A section's blocks as a fragment of its own. */
export function fragment(section: Section): Fragment {
  return { blocks: section.blocks };
}

/**
 * A section's body as a page draws it: the section with its blocks and, for a card, its card and
 * thread, as `/api/section` answers them and this device keeps them (`SECTIONS`,
 * `cache-document.ts`, which the page writes as the server sent them).
 */
export interface Body {
  section: Section;
  card?: DocumentCard | null;
  thread?: Fragment | null;
}

/** The bodies `document` holds of the sections `keys` names: each one drawn, with its card and thread. */
export function bodies(document: Document, keys: ReadonlySet<string>): Body[] {
  return document.sections
    .filter((section) => keys.has(section.key) && section.blocks.length > 0)
    .map((section) => {
      const slug = cardSlug(section.key);
      if (slug === null) return { section };
      const body: Body = { section };
      const card = document.cards.find((one) => one.slug === slug);
      if (card !== undefined) body.card = card;
      const thread = threadOf(document, section.key);
      if (thread !== undefined) body.thread = thread;
      return body;
    });
}

/**
 * `drawn` bodies in `document`, an outline's, whose sections have no blocks: the section of the
 * same key takes its body's blocks and keeps its place from the outline, and a card's takes its
 * thread and the card's whole text, its state the outline's. A body whose section the outline no
 * longer names is left out.
 */
export function drawBodies(document: Document, drawn: readonly Body[]): Document {
  if (drawn.length === 0) return document;
  const sections = [...document.sections];
  const cards = [...document.cards];
  const threads = { ...document.threads };
  for (const body of drawn) {
    const key = body.section.key;
    const at = sections.findIndex((section) => section.key === key);
    const section = sections[at];
    if (section === undefined) continue;
    sections[at] = { ...section, blocks: body.section.blocks };
    const slug = cardSlug(key);
    if (slug === null) continue;
    const card = cards.findIndex((one) => one.slug === slug);
    const outlined = cards[card];
    if (body.card != null && outlined !== undefined)
      cards[card] = { ...outlined, text: body.card.text };
    if (body.thread != null) put(threads, slug, body.thread);
    else delete threads[slug];
  }
  return { ...document, sections, cards, threads };
}

/**
 * Whether the section at `index` starts and ends its run of the register's part: a section keeps
 * its keyed parent when the register regroups, and only the run's visible edges change, so a
 * focused editor never remounts to take its card's border.
 */
export function partEdges(sections: readonly Section[], index: number): [boolean, boolean] {
  const section = sections[index];
  if (section === undefined) return [false, false];
  const previous = index > 0 ? sections[index - 1] : undefined;
  const next = sections[index + 1];
  return [
    previous === undefined || previous.part !== section.part,
    next === undefined || next.part !== section.part,
  ];
}

/** Whether a card is settled: resolved or rejected, folded under its summary. */
export const isSettled = (card: DocumentCard | null): boolean =>
  card !== null && (card.group === "resolved" || card.group === "rejected");

/** The first blocks (a section's `ends`) that open its prose as text, or as a box. */
const OPENS_TEXT = new Set(["paragraph", "list", "thread"]);
const OPENS_BOX = new Set(["table", "callout"]);
/** The last blocks that close its prose so the next part's text takes no gap of its own. */
const CLOSES = new Set([...OPENS_TEXT, ...OPENS_BOX, "heading-2", "heading-3", "heading-4"]);

/**
 * The classes of what a section's part draws as, read from the outline: an open card's box
 * (`doc-part-card`), and by the kinds of its first and last blocks (its `ends`) whether its prose
 * opens with text (`doc-part-opens-text`: a paragraph, a list, a thread's list) or a box
 * (`doc-part-opens-box`: a table, a callout), and whether it closes with one of those or an h2 to
 * h4 (`doc-part-closes`), as BlocksView draws them. A settled card's fold and a section drawn with
 * its difference from main take no part in the spacing between parts, and a section kept from
 * before the outline named its ends counts as none. docs.css spaces parts as one card's sections
 * collapsed their margins, by the neighbour's form, and reads these for a neighbour not drawn yet:
 * drawing it changes no margin of the part being read, so the browser's scroll anchoring keeps the
 * reader's place.
 */
export function partForm(section: Section, card: DocumentCard | null): string {
  if (section.comparison != null || isSettled(card)) return "";
  const [first, last] = section.ends ?? ["", ""];
  const box = OPENS_BOX.has(first) ? " doc-part-opens-box" : "";
  const opens = OPENS_TEXT.has(first) ? " doc-part-opens-text" : box;
  const closes = CLOSES.has(last) ? " doc-part-closes" : "";
  return `${card === null ? "" : " doc-part-card"}${opens}${closes}`;
}

/**
 * What a folder's page says it holds, in the pill under its title: the documents the server
 * counts, which leave out a history folder's pages, then those its listing shows beyond them, so
 * the pill and the cards below add up.
 */
export function folderPill(folder: Folder): string {
  const docs = (n: number) => (n === 1 ? "1 doc" : `${n} docs`);
  const listed = folder.entries.reduce(
    (sum, entry) => sum + (entry.kind === "folder" ? entry.docs : entry.kind === "doc" ? 1 : 0),
    0,
  );
  const more = listed - folder.docs;
  return more > 0 ? `${docs(folder.docs)} · ${more} in history` : docs(folder.docs);
}

/** Whether `task` is one the Symphony filed for the owner's verdicts (`VERDICT_TASKS`). */
export function answersVerdicts(task: DocumentTask): boolean {
  return task.slug.startsWith(VERDICT_TASKS);
}

/**
 * A document's related work as its panel lists it: the open tasks, then the finished ones (done or
 * cancelled), each in the board's order. The tasks the Symphony files for the owner's verdicts
 * answer the owner, not the document, and are left out.
 */
export function related(tasks: readonly DocumentTask[]): {
  open: DocumentTask[];
  finished: DocumentTask[];
} {
  const work = tasks.filter((task) => !answersVerdicts(task));
  const finished = (task: DocumentTask) => task.status === "done" || task.status === "cancelled";
  return { open: work.filter((task) => !finished(task)), finished: work.filter(finished) };
}
