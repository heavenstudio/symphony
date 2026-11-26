import { describe, expect, it } from "vitest";
import { decides, readOwner } from "./owner-read";
import type { Owner } from "./wire";

const DESIGN: Owner = {
  name: "design",
  kind: "group",
  by: "map",
  said: "from the area app",
  hand_to: ["engineering", "ana", "owner"],
  decides: true,
};

describe("whose a thing is, as the server says it", () => {
  it("reads an owner, its names to hand it to empty and its deciding false when left out", () => {
    expect(readOwner(DESIGN)).toEqual(DESIGN);
    const { hand_to: _, decides: _decides, ...bare } = DESIGN;
    expect(readOwner(bare)).toEqual({ ...bare, hand_to: [], decides: false });
    expect(readOwner({ ...DESIGN, hand_to: null })).toEqual({ ...DESIGN, hand_to: [] });
    expect(readOwner({ ...DESIGN, decides: null })).toEqual({ ...DESIGN, decides: false });
    expect(readOwner({ ...DESIGN, decides: false })).toEqual({ ...DESIGN, decides: false });
  });

  it("decides a thing where the server says so, and where it said no owner at all", () => {
    expect(decides(DESIGN)).toBe(true);
    expect(decides({ ...DESIGN, decides: false })).toBe(false);
    expect(decides(null)).toBe(true);
    expect(decides(undefined)).toBe(true);
  });

  it("is none for a field it draws that cannot be read", () => {
    for (const broken of [
      { ...DESIGN, name: 1 },
      { ...DESIGN, kind: null },
      { ...DESIGN, by: undefined },
      { ...DESIGN, said: [] },
      { ...DESIGN, hand_to: "ana" },
      { ...DESIGN, hand_to: ["ana", 2] },
      { ...DESIGN, decides: "yes" },
      null,
      "design",
    ]) {
      expect(readOwner(broken)).toBeNull();
    }
  });
});
