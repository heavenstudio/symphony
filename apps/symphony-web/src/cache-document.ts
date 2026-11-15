// The parts of a document's item in the client cache (`cache.ts`): what a document page, or an
// entrypoint's folder page, keeps of a visit, as the Dioxus client's `cache::document` keeps them.
// Never its versions, diffs, asks or history, nor a folder page but an entrypoint's own.

import type { Item } from "./cache";
import { type Body, bodies, cardSlug, put } from "./document";
import type { DocumentChange, DocumentOutline, Section } from "./wire";

/**
 * A document or folder page by its path as the API names it (`docs/X.md`, or `@tree/docs/X.md` for
 * a branch's); `entrypoint` for an entrypoint's own page, which is pinned.
 */
export function documentItem(path: string, entrypoint: boolean): Item {
  return { kind: "document", id: `document:${path}`, pinned: entrypoint };
}

/**
 * `/api/document?view=outline`; or, for a document read ahead of its first visit, its first part
 * (`view=first`), which carries the bodies of its leading sections (`readAhead`).
 */
export const OUTLINE = "outline";
/**
 * The bodies of the sections the page drew, in one map by section key, each with its card and
 * thread and the `validator` it was drawn under (`KeptBody`): a section's body is read again only
 * when the outline names another validator for it.
 */
export const SECTIONS = "sections";
/** An entrypoint's own folder page (`/api/folder?view=outline`). */
export const FOLDER = "folder";

/** A section's body as this device keeps it, under the validator it was taken under. */
export interface KeptBody {
  validator: string;
  body: Body;
}

/**
 * Whether a kept outline is a document's first part, read ahead of its first visit: one that
 * carries bodies, which an outline never does.
 */
export function readAhead(outline: DocumentOutline): boolean {
  const { threads, sections } = outline.document;
  return Object.keys(threads).length > 0 || sections.some((section) => section.blocks.length > 0);
}

/**
 * A document's first part read ahead (`readAhead`), as a visit would have kept it: its outline,
 * the bodies taken out, and each body under its validator.
 */
export function split(first: DocumentOutline): [DocumentOutline, Record<string, KeptBody>] {
  const { document } = first;
  const hashes = first.thread_hashes ?? {};
  const kept: Record<string, KeptBody> = {};
  for (const body of bodies(document, new Set(document.sections.map((section) => section.key)))) {
    const valid = validator(body.section, hashes);
    if (valid !== null) put(kept, body.section.key, { validator: valid, body });
  }
  const outline: DocumentOutline = {
    ...first,
    document: {
      ...document,
      sections: document.sections.map((section) => ({ ...section, blocks: [] })),
      threads: {},
    },
  };
  return [outline, kept];
}

/**
 * What tells a section's kept body current: the outline's `hash` of it, and for a card the hash of
 * its thread (`thread_hashes`), which its own leaves out. Null for a section never kept: one
 * showing a difference from main, whose `before` the outline leaves out.
 */
export function validator(
  section: Section,
  threads: Readonly<Record<string, string>>,
): string | null {
  if (section.comparison != null) return null;
  const slug = cardSlug(section.key);
  if (slug === null) return section.hash;
  return `${section.hash}/${Object.hasOwn(threads, slug) ? threads[slug] : ""}`;
}

/** The validator of each section of `outline` that may be kept, by key. */
export function validators(outline: DocumentOutline): Map<string, string> {
  const found = new Map<string, string>();
  for (const section of outline.document.sections) {
    const valid = validator(section, outline.thread_hashes ?? {});
    if (valid !== null) found.set(section.key, valid);
  }
  return found;
}

const sameMap = (a: ReadonlyMap<string, string>, b: ReadonlyMap<string, string>): boolean =>
  a.size === b.size && [...a].every(([key, value]) => b.get(key) === value);

/**
 * Which of the section bodies a page holds are current, and so kept on this device and not read
 * again: each body is taken under the `validator` the server's outline names for its section, and
 * stays current while the outline names the same one. Before the server has answered, the bodies
 * drawn from this device are current by the outline kept with them, and unconfirmed until the
 * server's outline names the same validator or the body is read again.
 */
export class Bodies {
  /** The validators of the outline the server last answered, by key, less the sections the feed changed since. */
  private validators = new Map<string, string>();
  /** That outline's thread hashes, which a body read since is taken under. */
  private threads: Readonly<Record<string, string>> = {};
  /** The validator each body the page holds was taken under, by key. */
  private taken = new Map<string, string>();
  /** The bodies drawn from this device that the server has not confirmed yet. */
  private unconfirmed = new Set<string>();
  /**
   * A body was taken or let go since the device was last given what is taken (`unkept`). A body
   * the feed changed is not: the device's copy of it is recognised as out of date by the next
   * outline.
   */
  private dirty = false;

  /**
   * The bodies this device `kept` drawn with `outline`, the outline kept with them: of them, those
   * current by it, each taken under its validator and unconfirmed, in the order of their keys.
   */
  drawn(outline: DocumentOutline, kept: Readonly<Record<string, KeptBody>>): Body[] {
    const valid = validators(outline);
    const drawn: Body[] = [];
    for (const key of Object.keys(kept).sort()) {
      const one = kept[key];
      if (one !== undefined && one.body.section.key === key && valid.get(key) === one.validator) {
        this.taken.set(key, one.validator);
        this.unconfirmed.add(key);
        drawn.push(one.body);
      }
    }
    return drawn;
  }

  /**
   * The bodies `drawn` came with a document's first part read ahead of the visit, kept in its
   * outline (`readAhead`), which the outline the visit reads replaces on the device: the device is
   * to keep those still current as the visit's own.
   */
  drawnAhead(): void {
    this.dirty = true;
  }

  /**
   * The server answered the outline: bodies are taken under its validators from now on. Returns
   * the keys of the bodies held that are still current by it, which the page keeps and reads no
   * more. A body drawn from this device that is not current stays unconfirmed while the page still
   * wants it (`wanted`), until it is read again.
   */
  answered(outline: DocumentOutline, wanted: ReadonlySet<string>): Set<string> {
    this.validators = validators(outline);
    this.threads = outline.thread_hashes ?? {};
    const held = this.taken.size;
    for (const [key, valid] of this.taken) {
      if (this.validators.get(key) !== valid) this.taken.delete(key);
    }
    this.dirty ||= this.taken.size !== held;
    const current = new Set(this.taken.keys());
    for (const key of this.unconfirmed) {
      if (current.has(key) || !wanted.has(key)) this.unconfirmed.delete(key);
    }
    return current;
  }

  /**
   * The page read the bodies of the sections `asked` and holds the answer's `sections`: each whose
   * section the server's outline names as it is now is taken under its validator, and no other
   * asked one is. Whether what is taken changed.
   */
  read(asked: ReadonlySet<string>, sections: readonly Section[]): boolean {
    const before = new Map(this.taken);
    for (const key of asked) this.taken.delete(key);
    for (const section of sections) {
      const valid = validator(section, this.threads);
      if (valid !== null && this.validators.get(section.key) === valid) {
        this.taken.set(section.key, valid);
      } else {
        this.taken.delete(section.key);
      }
    }
    for (const key of asked) this.unconfirmed.delete(key);
    const changed = !sameMap(this.taken, before);
    this.dirty ||= changed;
    return changed;
  }

  /**
   * The feed changed `delta`'s sections (their text, card or thread): no validator the outline
   * named tells them current any more, until the server's next outline. Whether what is taken
   * changed.
   */
  changed(delta: DocumentChange): boolean {
    const cards = [
      ...Object.keys(delta.threads ?? {}),
      ...(delta.cards ?? []).map((card) => card.key),
    ].map((slug) => `card:${slug}`);
    const keys = [
      ...(delta.sections ?? []).map((section) => section.key),
      ...(delta.deleted ?? []),
      ...cards,
    ];
    let changed = false;
    for (const key of keys) {
      this.validators.delete(key);
      changed = this.taken.delete(key) || changed;
    }
    return changed;
  }

  /** Whether every body drawn from this device is confirmed or read again. */
  confirmed(): boolean {
    return this.unconfirmed.size === 0;
  }

  /**
   * Which bodies this device is to keep, each under its validator, by key in order, when one was
   * taken or let go since it was last given them; null when it holds them already.
   */
  unkept(): Map<string, string> | null {
    if (!this.dirty) return null;
    this.dirty = false;
    // A map's keys are never equal.
    return new Map([...this.taken].sort(([a], [b]) => (a < b ? -1 : 1)));
  }

  /**
   * The validator each body the page holds is taken under, by key: those this device is to keep,
   * each as the server sent it (`DocumentLive.keepBodies`), never as the page's copy has it since.
   */
  current(): ReadonlyMap<string, string> {
    return new Map(this.taken);
  }
}
