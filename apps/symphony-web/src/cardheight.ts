// How tall a task's card draws in a board column, worked out from the task before it is drawn, so
// the row waiting for it holds the place it will take and a column neither grows nor shrinks as
// its rows come in.
//
// It follows board.css's card in a column. The card's text is 244 px wide in a desk's 288 px
// column and 236 px in a phone's 280 px (`max-width: 760px`), 3 px less behind the wider left
// border of a p0 or p1. Its borders, padding and margins take 40 px; each line its slug, its title
// and, for a subtask, its parent wrap to takes 20.3 px (14 px at 1.45); then come the rows its chips
// wrap to, 4 px apart: its build and test phases when it has any, 6 px below the title, then its
// link chips and its role chips. A role chip keeps to one line (21.2 px); a phase chip (19.2 px a
// line) and a link chip (15.2 px a line) wrap inside themselves when wider than the card. Text is
// measured in Helvetica's widths, the face `--font-sans` comes to where the browser has no system
// face of its own to give it, and the card breaks it as `overflow-wrap:anywhere` does.

import type { Task } from "./wire";

/** Tenths of a pixel: a card's box, a line of its text, the gap between chips, a chip's two
 * borders, and the space above its phases. */
const BOX = 400;
const LINE = 203;
export const GAP = 40;
const BORDER = 20;
const PHASES = 60;

/** A kind of chip: the size of its text, the room its padding, borders and dot take beside the
 * text, and how tall each line of its text is, in tenths of a pixel; and whether it wraps. */
export interface Chip {
  size: number;
  beside: number;
  line: number;
  wraps: boolean;
}

/** A link chip: its file name at 10.5 px, 7 px padded each side. */
export const LINK: Chip = { size: 105, beside: 160, line: 152, wraps: true };
/** A role chip: its role and holder at 12 px, 8 px padded with a 7 px dot 4 px before. */
export const ROLE: Chip = { size: 120, beside: 290, line: 192, wraps: false };
/** A phase chip: drawn as a role chip is, but wrapping its words. */
export const PHASE: Chip = { ...ROLE, wraps: true };

/** The board's layout, which sets how wide a card's text runs: a phone's at 760 px or less. */
export type Layout = "desk" | "phone";

/** The layout of a window `width` px wide. */
export function layoutAt(width: number): Layout {
  return width <= 760 ? "phone" : "desk";
}

/** How wide a card's text runs in `layout`, in tenths of a pixel, for a task of `priority`. */
export function textWidth(layout: Layout, priority: number): number {
  const column = layout === "desk" ? 2440 : 2360;
  return priority === 0 || priority === 1 ? column - 30 : column;
}

/** The faces a card's text is set in: the slug's, the parent's and the chips', the title's. */
export type Face = "mono" | "regular" | "bold";

/** A character's width in thousandths of its font's size, as Helvetica draws it. */
export function advance(ch: string, face: Face): number {
  if (face === "mono") return 600;
  const pick = (regular: number, heavy: number) => (face === "bold" ? heavy : regular);
  if ("ijl".includes(ch)) return pick(222, 278);
  if (ch === "'") return pick(191, 238);
  if (" ,./I[]\\|".includes(ch)) return 278;
  if ("ft!:;".includes(ch)) return pick(278, 333);
  if ("()-`".includes(ch)) return 333;
  if (ch === "r") return pick(333, 389);
  if (ch === "z") return 500;
  if ("cksvxyJ".includes(ch)) return pick(500, 556);
  if ("bdghnopqu?L".includes(ch)) return pick(556, 611);
  if ("FTZ".includes(ch)) return 611;
  if ("ABK&".includes(ch)) return pick(667, 722);
  if ("EPSVXY".includes(ch)) return 667;
  if ("CDHNRU".includes(ch)) return 722;
  if (ch === "w") return pick(722, 778);
  if ("GOQ".includes(ch)) return 778;
  if (ch === "m") return pick(833, 889);
  if (ch === "M") return 833;
  if (ch === "W") return 944;
  return 556;
}

/** The width of `text` in `face`, in thousandths of its font's size, a character at a time. */
function measure(text: string, face: Face): number {
  let width = 0;
  for (const ch of text) width += advance(ch, face);
  return width;
}

/** What Rust's `char::is_whitespace` takes for a space: Unicode's White_Space. */
const SPACE = /[\t-\r \u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]+/u;

/** `word` cut after each hyphen and slash, each piece keeping its own. */
function pieces(word: string): string[] {
  return word.match(/[^-/]*[-/]|[^-/]+$/g) ?? [];
}

/**
 * Text set line after line across a card, as the card breaks it: between words, after a hyphen or
 * a slash, and inside a part wider than a line. Widths are kept in ten-thousandths of a pixel, a
 * face's thousandths of its size times the size in tenths.
 */
export class Lines {
  private readonly width: number;
  private lines = 0;
  private used = 0;
  /** The space before the next word. */
  private gap = 0;

  /** Nothing set yet across `width` tenths of a pixel. */
  constructor(width: number) {
    this.width = Math.max(width * 1000, 1);
  }

  /** A space in `face` at `size` tenths of a pixel before what is set next. */
  space(face: Face, size: number): this {
    this.gap = advance(" ", face) * size;
    return this;
  }

  /** `text` in `face` at `size` tenths of a pixel, set after what the lines hold. */
  set(text: string, face: Face, size: number): this {
    const space = advance(" ", face) * size;
    for (const word of text.split(SPACE).filter((one) => one !== "")) {
      pieces(word).forEach((part, index) => {
        let before = 0;
        if (index === 0) {
          before = this.gap;
          this.gap = space;
        }
        this.place(measure(part, face) * size, before);
      });
    }
    this.gap = 0;
    return this;
  }

  /** One unbreakable part `width` wide, `before` after the last on its line or on a line of its
   * own, broken across lines where it is wider than one. */
  private place(width: number, before: number): void {
    const wide = Math.max(width, 1);
    if (this.lines === 0 || this.used + before + wide > this.width) {
      this.lines += Math.ceil(wide / this.width);
      this.used = ((wide - 1) % this.width) + 1;
    } else {
      this.used += before + wide;
    }
  }

  /** How many lines the text set takes. */
  count(): number {
    return this.lines;
  }
}

/**
 * How wide and tall a chip of `kind` reading `text` draws in a card whose text is `width` wide: as
 * wide as its text, or as the card, where a chip that wraps takes as many lines as its text needs.
 */
export function chip(text: string, kind: Chip, width: number): [number, number] {
  const natural = kind.beside + Math.floor((measure(text, "regular") * kind.size) / 1000);
  const lines =
    kind.wraps && natural > width
      ? Math.max(
          new Lines(Math.max(width - kind.beside, 0)).set(text, "regular", kind.size).count(),
          1,
        )
      : 1;
  return [Math.min(natural, width), BORDER + kind.line * lines];
}

/**
 * The height of `chips` (each its width and height, in tenths of a pixel) wrapped in rows across a
 * card's text `width` wide, a gap between them each way, each row as tall as its tallest chip; no
 * chips take none.
 */
export function chipRows(chips: [number, number][], width: number): number {
  let total = 0;
  let row: number | null = null;
  let used = 0;
  for (const [wide, height] of chips) {
    const one = Math.min(wide, width);
    if (row !== null && used + GAP + one <= width) {
      used += GAP + one;
      row = Math.max(row, height);
    } else {
      if (row !== null) total += row + GAP;
      row = height;
      used = one;
    }
  }
  return total + (row ?? 0);
}

/** The last part of a link's path: what its chip says. */
export function linkName(link: string): string {
  return link.split("/").pop() ?? link;
}

/**
 * What `task`'s card draws at in `layout`, in whole pixels, its margin included, with the build
 * and test phases its card shows (`phaseLabels`).
 */
export function rowHeight(
  task: Pick<Task, "slug" | "title" | "priority" | "parent" | "links" | "roles">,
  phases: string[],
  layout: Layout,
): number {
  const width = textWidth(layout, task.priority);
  const slug = new Lines(width)
    .set(task.slug, "mono", 115)
    .space("regular", 140)
    .set(`p${task.priority}`, "bold", 101)
    .count();
  const title = new Lines(width).set(task.title, "bold", 140).count();
  const parent =
    task.parent === null || task.parent === undefined
      ? 0
      : new Lines(width).set(`Subtask of ${task.parent}`, "regular", 140).count();
  const phaseRows =
    phases.length === 0
      ? 0
      : PHASES +
        chipRows(
          phases.map((label) => chip(label, PHASE, width)),
          width,
        );
  const links = task.links.map((link) => chip(linkName(link), LINK, width));
  const roles = task.roles.map((role) => chip(`${role.role} ${role.handle}`, ROLE, width));
  return Math.floor(
    (BOX +
      LINE * (slug + title + parent) +
      phaseRows +
      chipRows(links, width) +
      chipRows(roles, width) +
      5) /
      10,
  );
}
