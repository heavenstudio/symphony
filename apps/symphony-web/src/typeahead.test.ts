import { describe, expect, it } from "vitest";
import {
  bestMatch,
  codeToChar,
  distance,
  editDistance,
  guessLayout,
  Keyboard,
  keyDistance,
  recencyBias,
} from "./typeahead";

const options = (...texts: string[]) => texts.map((text, index) => ({ index, text }));
const letters = (text: string) => [...text];

describe("the edit distance of what was typed", () => {
  it("is nearly none for the same letters, more for any other", () => {
    const one = () => 1;
    // Very small but not exactly 0, for the recency bias.
    expect(editDistance(letters("abc"), letters("abc"), one)).toBeLessThan(0.01);
    expect(editDistance(letters("abc"), letters("abd"), one)).toBeGreaterThan(0);
    expect(editDistance(letters("ab"), letters("abc"), one)).toBeGreaterThan(0);
    expect(editDistance(letters("abc"), letters("ab"), one)).toBeGreaterThan(0);
  });

  it("costs less for a letter mistyped for one beside it on the keyboard", () => {
    const qwerty = new Keyboard();
    const keys = (a: string, b: string) => keyDistance("qwerty", a, b) ?? 1;
    expect(editDistance(letters("qw"), letters("qe"), keys)).toBeLessThan(
      editDistance(letters("qw"), letters("qp"), keys),
    );
    expect(distance(letters("q"), letters("w"), qwerty)).toBeLessThan(
      distance(letters("q"), letters("p"), qwerty),
    );
  });

  it("weighs the latest letters most", () => {
    expect(recencyBias(9, 10)).toBeGreaterThan(recencyBias(0, 10));
    expect(recencyBias(0, 1)).toBeGreaterThan(0);
    expect(recencyBias(0, 1)).toBeLessThanOrEqual(1);
  });

  it("reads an option's first letters, as many as were typed, against the latest typed", () => {
    const keyboard = new Keyboard();
    expect(distance(letters("abc"), letters("abc"), keyboard)).toBeLessThan(0.01);
    expect(distance(letters("abc"), letters("abd"), keyboard)).toBeGreaterThan(0);
    // Longer than the option: its last letters are read.
    expect(distance(letters("xyzab"), letters("ab"), keyboard)).toBeLessThan(0.01);
    expect(distance(letters("abcde"), letters("xyz"), keyboard)).toBeGreaterThan(0);
    // An option read for its first letter alone: the rest of it costs nothing.
    expect(distance(letters("a"), letters("apple"), keyboard)).toBe(
      distance(letters("a"), letters("a"), keyboard),
    );
  });
});

describe("the keyboard's layout", () => {
  it("is guessed from the keys pressed against what they typed", () => {
    expect(
      guessLayout(
        new Map([
          ["KeyQ", "q"],
          ["KeyW", "w"],
          ["KeyE", "e"],
        ]),
      ),
    ).toBe("qwerty");
    expect(
      guessLayout(
        new Map([
          ["KeyQ", "q"],
          ["KeyW", "w"],
          ["KeyE", "f"],
          ["KeyR", "p"],
        ]),
      ),
    ).toBe("colemakDh");
    expect(guessLayout(new Map([["KeyQ", "a"]]))).toBe("azerty");
    expect(guessLayout(new Map())).toBe("qwerty");
    // A key no layout places, or one typed with no physical letter, fits none.
    expect(guessLayout(new Map([["KeyA", "ф"]]))).toBeNull();
    expect(guessLayout(new Map([["Space", " "]]))).toBeNull();
  });

  it("names a key's Qwerty letter by its physical code", () => {
    expect(codeToChar("KeyA")).toBe("a");
    expect(codeToChar("KeyZ")).toBe("z");
    expect(codeToChar("Digit7")).toBe("7");
    expect(codeToChar("Enter")).toBeNull();
    expect(codeToChar("Keya")).toBeNull();
  });

  it("puts letters beside each other closer than letters apart", () => {
    expect(keyDistance("qwerty", "a", "a")).toBe(0);
    expect(keyDistance("qwerty", "q", "w")).toBeLessThan(keyDistance("qwerty", "q", "p") ?? 0);
    expect(keyDistance("qwerty", "1", "/")).toBeCloseTo(Math.hypot(9, 3) / 10);
    expect(keyDistance("qwerty", "α", "β")).toBeNull();
    expect(keyDistance(null, "q", "w")).toBeNull();
  });

  it("learns the layout from each key, Qwerty until one says otherwise", () => {
    const keyboard = new Keyboard();
    expect(keyboard.layout).toBe("qwerty");
    keyboard.learn("KeyQ", "A");
    expect(keyboard.layout).toBe("azerty");
    keyboard.learn("KeyA", "ф");
    keyboard.learn("KeyS", "ы");
    expect(keyboard.layout).toBeNull();
  });
});

describe("what typing a letter for another costs", () => {
  it("is nothing for the same letter, and least between letters near in Unicode", () => {
    const keyboard = new Keyboard();
    expect(keyboard.cost("a", "a")).toBe(0);
    keyboard.learn("KeyA", "ф");
    // No layout: the letters' Unicode distance, a tenth at least, and all at most.
    expect(keyboard.cost("a", "b")).toBe(0.1);
    expect(keyboard.cost("a", "中")).toBe(1);
  });

  it("is a fifth for letters that sound alike in another script, all for unrelated ones", () => {
    const keyboard = new Keyboard();
    keyboard.learn("KeyA", "ф");
    expect(keyboard.cost("a", "а")).toBe(0.2);
    expect(keyboard.cost("b", "ب")).toBe(0.2);
    expect(keyboard.cost("x", "ж")).toBe(1);
    // A capital reads as its small letter for its sound and its place on the keyboard.
    expect(keyboard.cost("F", "ф")).toBe(0.2);
  });

  it("weighs the keyboard's distance at three tenths, when it places both letters", () => {
    const keyboard = new Keyboard();
    expect(keyboard.cost("q", "w")).toBeCloseTo(0.03);
    expect(keyboard.cost("Q", "w")).toBeCloseTo(0.03);
  });
});

describe("the option typing focuses", () => {
  const fruit = options("Apple", "Banana", "Cherry");

  it("is the one nearest what was typed, and none for nothing typed", () => {
    const keyboard = new Keyboard();
    expect(bestMatch(keyboard, "App", fruit)).toBe(0);
    expect(bestMatch(keyboard, "ban", fruit)).toBe(1);
    expect(bestMatch(keyboard, "", fruit)).toBeNull();
    // No match: the closest option still.
    expect(bestMatch(keyboard, "xyz", fruit)).not.toBeNull();
    expect(bestMatch(keyboard, "c", [])).toBeNull();
  });

  it("is the first of the nearest, among those available", () => {
    const keyboard = new Keyboard();
    expect(bestMatch(keyboard, "a", options("Alpha", "Alpine"))).toBe(0);
    expect(bestMatch(keyboard, "App", fruit, (index) => index !== 0)).not.toBe(0);
  });

  it("finds a letter of another script by what the keyboard learnt", () => {
    const keyboard = new Keyboard();
    keyboard.learn("KeyA", "ф");
    keyboard.learn("KeyS", "ы");
    const mixed = options("ф", "Banana");
    expect(bestMatch(keyboard, "ф", mixed)).toBe(0);
    expect(bestMatch(keyboard, "b", mixed)).toBe(1);
  });

  it("finds the option a priority's digit names", () => {
    const keyboard = new Keyboard();
    const priorities = options("0 — urgent", "1 — high", "2 — normal", "3 — low");
    expect(bestMatch(keyboard, "2", priorities)).toBe(2);
    expect(bestMatch(keyboard, "3", priorities)).toBe(3);
  });
});
