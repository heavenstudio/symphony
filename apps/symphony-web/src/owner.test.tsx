import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { handOver, OwnerLine, ownerLabel, readCardOwners } from "./owner";
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
  it("reads a document's cards' owners by slug, and none where one cannot be read", () => {
    expect(readCardOwners({ cards: { "new-palette": DESIGN } })).toEqual({ "new-palette": DESIGN });
    expect(readCardOwners({ cards: {} })).toEqual({});
    for (const broken of [{ cards: { a: {} } }, { cards: [] }, { cards: null }, {}, null]) {
      expect(readCardOwners(broken)).toBeNull();
    }
  });

  it("labels a group as one, and a person or the person at the machine as a person", () => {
    expect(ownerLabel(DESIGN)).toBe("group");
    expect(ownerLabel({ ...DESIGN, name: "ana", kind: "person" })).toBe("person");
    expect(ownerLabel({ ...DESIGN, name: "owner", kind: "machine" })).toBe("person");
  });
});

describe("the owner's line", () => {
  const drawn = (owner: Owner) =>
    renderToStaticMarkup(<OwnerLine owner={owner} id="hand" hand={async () => ({})} />);

  it("says whose it is and what gave it, with whom to hand it to for one who may", () => {
    const html = drawn(DESIGN);
    expect(html).toContain("group: design");
    expect(html).toContain("from the area app");
    expect(html).toContain('data-by="map"');
    expect(html).toContain('id="hand"');
  });

  it("draws no select for one who may hand it to no one", () => {
    const html = drawn({ ...DESIGN, hand_to: [] });
    expect(html).toContain("group: design");
    expect(html).not.toContain('id="hand"');
    const { hand_to: _handTo, ...unsaid } = DESIGN;
    expect(drawn(unsaid)).not.toContain('id="hand"');
  });

  it("says a hand-over done, or its refusal in the server's words", async () => {
    const asked: string[] = [];
    expect(await handOver("engineering", async (to) => asked.push(to))).toBe(
      "Handed to engineering",
    );
    expect(asked).toEqual(["engineering"]);
    const refused = async () => {
      throw new Error("bo is no member of the group design, whose it is, so may not hand it on");
    };
    expect(await handOver("engineering", refused)).toBe(
      "bo is no member of the group design, whose it is, so may not hand it on",
    );
  });
});
