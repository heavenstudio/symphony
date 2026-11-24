import { describe, expect, it } from "vitest";
import { readOwner } from "./owner-read";
import type { Owner } from "./wire";

const DESIGN: Owner = {
  name: "design",
  kind: "group",
  by: "map",
  said: "from the area app",
  hand_to: ["engineering", "ana", "owner"],
};

describe("whose a thing is, as the server says it", () => {
  it("reads an owner, its names to hand it to empty when left out", () => {
    expect(readOwner(DESIGN)).toEqual(DESIGN);
    const { hand_to: _, ...bare } = DESIGN;
    expect(readOwner(bare)).toEqual({ ...bare, hand_to: [] });
    expect(readOwner({ ...DESIGN, hand_to: null })).toEqual({ ...DESIGN, hand_to: [] });
  });

  it("is none for a field it draws that cannot be read", () => {
    for (const broken of [
      { ...DESIGN, name: 1 },
      { ...DESIGN, kind: null },
      { ...DESIGN, by: undefined },
      { ...DESIGN, said: [] },
      { ...DESIGN, hand_to: "ana" },
      { ...DESIGN, hand_to: ["ana", 2] },
      null,
      "design",
    ]) {
      expect(readOwner(broken)).toBeNull();
    }
  });
});
