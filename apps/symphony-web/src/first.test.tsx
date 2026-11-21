import { renderToStaticMarkup as render } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { FirstSteps, noTasks } from "./first";
import { GETTING_STARTED } from "./wire";

describe("an empty page's first steps", () => {
  it("are one line that ends in the guide's link, opened beside the Symphony", () => {
    expect(render(<FirstSteps>Nothing here yet:</FirstSteps>)).toBe(
      `<p class="first-steps">Nothing here yet: <a href="${GETTING_STARTED}" target="_blank" rel="noopener noreferrer">Getting started</a></p>`,
    );
  });
  it("link the guide in the Symphony's repository", () => {
    expect(GETTING_STARTED).toMatch(/^https:\/\/[^ ]+\/blob\/main\/docs\/GETTING_STARTED\.md$/);
  });
});

describe("a board with no task", () => {
  it("is one whose every stage counts none", () => {
    expect(noTasks([["ready", 0]])).toBe(true);
    expect(
      noTasks([
        ["ready", 0],
        ["done", 0],
        ["cancelled", 0],
      ]),
    ).toBe(true);
  });
  it("is not one with a task open, finished or cancelled in its window", () => {
    for (const stage of ["backlog", "ready", "done", "cancelled"]) {
      expect(
        noTasks([
          ["ready", 0],
          [stage, 1],
        ]),
      ).toBe(false);
    }
  });
  it("is one that counts no stage, as a new board answers", () => {
    expect(noTasks([])).toBe(true);
  });
});
