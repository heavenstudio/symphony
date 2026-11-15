// What typing in an open select's popup focuses: the option whose text is nearest what was typed
// (within the last second), as the Dioxus primitives' select finds it (dioxus-primitives
// `select/text_search.rs`): an edit distance over the typed letters and the option's first as many,
// the latest letters weighing most, where a letter mistyped for one near it on the keyboard, one
// that sounds alike in another script or one close by in Unicode costs less than another. The
// keyboard's layout is learnt from the keys pressed (their physical codes against the letters they
// typed), Qwerty until a key says otherwise.

/** How long the letters typed are kept for one search, in ms. */
export const TYPEAHEAD_MS = 1000;

type Layout = readonly (readonly string[])[];

const LAYOUTS: Record<string, Layout> = {
  qwerty: ["1234567890", "qwertyuiop", "asdfghjkl;", "zxcvbnm,./"].map((row) => [...row]),
  colemakDh: ["1234567890", "qwfpbjluy;", "arstgmneio", "xcdvzkh,./"].map((row) => [...row]),
  colemak: ["1234567890", "qwfpgjluy;", "arstdhneio", "zxcvbkm,./"].map((row) => [...row]),
  dvorak: ["1234567890", "',.pyfgcrl", "aoeuidhtns", ";qjkxbmwvz"].map((row) => [...row]),
  workman: ["1234567890", "qdrwbjfup;", "ashtgyneoi", "zxmcvkl,./"].map((row) => [...row]),
  azerty: ["1234567890", "azertyuiop", "qsdfghjklm", "wxcvbn,;:!"].map((row) => [...row]),
  qwertz: ["1234567890", "qwertzuiop", "asdfghjklö", "yxcvbnm,.-"].map((row) => [...row]),
};

/** The layouts a key's code is tried against, in the order the primitives try them. */
const KNOWN = ["qwerty", "colemakDh", "colemak", "dvorak", "workman", "azerty", "qwertz"];

/** Where `char` sits on `layout`: its column and row. */
function place(layout: Layout, char: string): [number, number] | null {
  for (const [row, keys] of layout.entries()) {
    const column = keys.indexOf(char);
    if (column >= 0) return [column, row];
  }
  return null;
}

/** The letter a key's physical code types on a Qwerty keyboard: `KeyA` is `a`, `Digit1` is `1`. */
export function codeToChar(code: string): string | null {
  const key = /^Key([A-Z])$/.exec(code) ?? /^Digit([0-9])$/.exec(code);
  return key ? (key[1] ?? "").toLowerCase() : null;
}

/** The first known layout every key learnt fits, by its place on Qwerty; none when none fits. */
export function guessLayout(known: Map<string, string>): string | null {
  return (
    KNOWN.find((name) => {
      const layout = LAYOUTS[name] ?? [];
      return [...known].every(([code, typed]) => {
        const from = codeToChar(code);
        const at = from === null ? null : place(LAYOUTS.qwerty ?? [], from);
        const to = place(layout, typed);
        return at !== null && to !== null && at[0] === to[0] && at[1] === to[1];
      });
    }) ?? null
  );
}

/** How far apart two letters sit on `layout`, 0 to 1; none for a letter it does not hold. */
export function keyDistance(layoutName: string | null, a: string, b: string): number | null {
  const layout = layoutName === null ? undefined : LAYOUTS[layoutName];
  if (!layout) return null;
  const from = place(layout, a);
  const to = place(layout, b);
  if (from === null || to === null) return null;
  const distance = Math.hypot(from[0] - to[0], from[1] - to[1]);
  return Math.min(Math.max(distance / 10, 0), 1);
}

const PHONETIC: readonly string[] = [
  "aаαاআ",
  "bбβبব",
  "sсσسস",
  "tтτتত",
  "nнνنন",
  "rрρرর",
  "lлλلল",
  "mмμمম",
  "kкκكক",
  "pпπپপ",
  "fфφفফ",
  "oоοوও",
  "eеεهএ",
  "iиιيই",
  "uуυوউ",
  "dдδدদ",
  "gгγجগ",
  "hхηهহ",
  "vвβوভ",
  "zзζزজ",
  "yйυي",
];

/** The keyboard the reader types on, learnt from the keys they press. */
export class Keyboard {
  private readonly learnt = new Map<string, string>();
  layout: string | null = "qwerty";

  /** A key pressed: its physical code typed `char`. */
  learn(code: string, char: string): void {
    this.learnt.set(code, char.toLowerCase().charAt(0) || char);
    this.layout = guessLayout(this.learnt);
  }

  /** What typing `a` for `b` costs: the least of their distance on the keyboard, their sound in
   * another script and their distance in Unicode. */
  cost(a: string, b: string): number {
    if (a === b) return 0;
    const lowerA = a.toLowerCase().charAt(0) || a;
    const lowerB = b.toLowerCase().charAt(0) || b;
    const physical = keyDistance(this.layout, lowerA, lowerB);
    const unicode = Math.min(
      Math.max(Math.abs((a.codePointAt(0) ?? 0) - (b.codePointAt(0) ?? 0)) / 100, 0.1),
      1,
    );
    const phonetic = PHONETIC.some((group) => group.includes(lowerA) && group.includes(lowerB))
      ? 0.2
      : 1;
    return Math.min(
      physical === null ? Number.POSITIVE_INFINITY : physical * 0.3,
      unicode,
      phonetic,
    );
  }
}

/** How much a letter at `at` of `length` weighs: the later, the more, up to 1. */
export function recencyBias(at: number, length: number): number {
  return (Math.log(at + 1.5) / Math.log(length + 1.5)) ** 4;
}

/** The edit distance from `typed` to `value`, each step weighed by its recency, over the most it
 * could be. */
export function editDistance(
  typed: string[],
  value: string[],
  cost: (a: string, b: string) => number,
): number {
  const rows: number[][] = Array.from({ length: typed.length + 1 }, () =>
    new Array<number>(value.length + 1).fill(0),
  );
  const at = (i: number, j: number) => rows[i]?.[j] ?? 0;
  const set = (i: number, j: number, cell: number) => {
    const row = rows[i];
    if (row) row[j] = cell;
  };
  let previous = 0;
  for (let j = 0; j <= value.length; j += 1) {
    previous += (1 - recencyBias(j, value.length)) * 0.5;
    set(0, j, previous);
  }
  previous = 0;
  for (let i = 0; i <= typed.length; i += 1) {
    previous += recencyBias(i, typed.length) * 0.5;
    set(i, 0, previous);
  }
  for (let i = 1; i <= typed.length; i += 1) {
    for (let j = 1; j <= value.length; j += 1) {
      const a = typed[i - 1] ?? "";
      const b = value[j - 1] ?? "";
      const step = a === b ? 0 : cost(a, b);
      set(
        i,
        j,
        Math.min(
          at(i - 1, j) + recencyBias(i, typed.length),
          at(i, j - 1) + (1 - recencyBias(j, value.length)),
          at(i - 1, j - 1) + step * 2 * recencyBias(i, typed.length),
        ),
      );
    }
  }
  return at(typed.length, value.length) / Math.max(at(typed.length, 0), at(0, value.length));
}

/** The distance from what was typed to an option's text, over its first as many letters as were
 * typed (the latest typed ones, when the text is shorter). */
export function distance(typed: string[], value: string[], keyboard: Keyboard): number {
  const head = value.slice(0, Math.min(value.length, typed.length));
  const tail = typed.slice(Math.max(typed.length - head.length, 0));
  return editDistance(tail, head, (a, b) => keyboard.cost(a, b));
}

/** The option nearest what was typed, by its index, among those `available` takes: the first of
 * the nearest; none when nothing was typed. */
export function bestMatch(
  keyboard: Keyboard,
  typed: string,
  options: { index: number; text: string }[],
  available: (index: number) => boolean = () => true,
): number | null {
  if (typed === "") return null;
  const letters = [...typed];
  let best: [number, number] | null = null;
  for (const option of options) {
    if (!available(option.index)) continue;
    const away = distance(letters, [...option.text], keyboard);
    if (best === null || away < best[0]) best = [away, option.index];
  }
  return best?.[1] ?? null;
}
