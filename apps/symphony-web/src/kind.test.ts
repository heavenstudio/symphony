import { describe, expect, it } from "vitest";
import { isKind, KINDS, taskKind } from "./kind";

describe("a task's kind", () => {
  it("is one of the board's five, in the order a list names them", () => {
    expect(KINDS).toEqual(["feature", "fix", "red", "tests", "docs"]);
    expect(KINDS.every(isKind)).toBe(true);
    expect([isKind("chore"), isKind("Fix"), isKind(null), isKind(3)]).toEqual([
      false,
      false,
      false,
      false,
    ]);
  });

  it("is read from the board's own fields, none where there is none or another word", () => {
    expect(taskKind({ _board: { kind: "red", arm: "a" }, owner: { kind: "fix" } })).toBe("red");
    expect(taskKind({ owner: { kind: "fix" } })).toBeNull();
    expect(taskKind({ _board: { kind: "chore" } })).toBeNull();
    expect(taskKind({ _board: "fix" })).toBeNull();
    expect(taskKind(null)).toBeNull();
    expect(taskKind(undefined)).toBeNull();
  });
});
