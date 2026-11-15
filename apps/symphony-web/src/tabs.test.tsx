import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { movedTo, TabContent, TabList, Tabs, TabTrigger, tabStop } from "./tabs";

describe("a key on a tab", () => {
  it("moves the focus along the list by its arrows, round it, and to its ends", () => {
    expect(movedTo("ArrowRight", true, 0, 3)).toBe(1);
    expect(movedTo("ArrowRight", true, 2, 3)).toBe(0);
    expect(movedTo("ArrowLeft", true, 0, 3)).toBe(2);
    expect(movedTo("ArrowLeft", true, 2, 3)).toBe(1);
    expect(movedTo("Home", true, 2, 3)).toBe(0);
    expect(movedTo("End", false, 0, 3)).toBe(2);
  });

  it("moves along a vertical list by its up and down arrows alone", () => {
    expect(movedTo("ArrowDown", false, 0, 2)).toBe(1);
    expect(movedTo("ArrowUp", false, 0, 2)).toBe(1);
    expect(movedTo("ArrowRight", false, 0, 2)).toBeNull();
    expect(movedTo("ArrowDown", true, 0, 2)).toBeNull();
  });

  it("starts from the first or the last with no tab focused yet, and moves nothing else", () => {
    expect(movedTo("ArrowRight", true, null, 3)).toBe(0);
    expect(movedTo("ArrowLeft", true, null, 3)).toBe(2);
    expect(movedTo("Enter", true, 1, 3)).toBeNull();
    expect(movedTo("Home", true, null, 0)).toBeNull();
  });
});

describe("the list's stop for Tab", () => {
  it("is the tab focused last, else the chosen one, else the first", () => {
    expect([0, 1, 2].map((index) => tabStop(index, 2, 1, false))).toEqual([-1, -1, 0]);
    expect([0, 1, 2].map((index) => tabStop(index, null, 1, false))).toEqual([-1, 0, -1]);
    expect([0, 1, 2].map((index) => tabStop(index, null, -1, false))).toEqual([0, -1, -1]);
  });

  it("is none while the tabs are disabled", () => {
    expect([0, 1].map((index) => tabStop(index, 0, 0, true))).toEqual([-1, -1]);
  });
});

const draw = (value: string, disabled = false) =>
  renderToStaticMarkup(
    <Tabs value={value} values={["a", "b"]} onChange={() => {}} disabled={disabled} horizontal>
      <TabList>
        <TabTrigger id="a-tab" value="a" controls="a-panel">
          A
        </TabTrigger>
        <TabTrigger id="b-tab" value="b" controls="b-panel">
          B
        </TabTrigger>
      </TabList>
      <TabContent id="a-panel" value="a" labelledBy="a-tab">
        <p>first</p>
      </TabContent>
      <TabContent id="b-panel" value="b" labelledBy="b-tab">
        <p>second</p>
      </TabContent>
    </Tabs>,
  );

describe("tabs", () => {
  it("draw the chosen tab and its panel, the others' panels hidden and empty", () => {
    expect(draw("b")).toBe(
      '<div class="dx-tabs" data-variant="default" data-orientation="horizontal" data-disabled="false">' +
        '<div role="tablist" class="dx-tabs-list">' +
        '<button role="tab" id="a-tab" class="dx-tabs-trigger" tabindex="-1" type="button" aria-selected="false" aria-controls="a-panel" data-state="inactive" data-disabled="false">A</button>' +
        '<button role="tab" id="b-tab" class="dx-tabs-trigger" tabindex="0" type="button" aria-selected="true" aria-controls="b-panel" data-state="active" data-disabled="false">B</button>' +
        "</div>" +
        '<div role="tabpanel" id="a-panel" class="dx-tabs-content" tabindex="0" data-state="inactive" hidden="" aria-labelledby="a-tab"></div>' +
        '<div role="tabpanel" id="b-panel" class="dx-tabs-content" tabindex="0" data-state="active" aria-labelledby="b-tab"><p>second</p></div>' +
        "</div>",
    );
  });

  it("disabled, take no focus and no click", () => {
    const markup = draw("a", true);
    expect(markup).toContain('data-orientation="horizontal" data-disabled="true"');
    expect(markup.match(/tabindex="-1"/g)).toHaveLength(2);
    expect(markup.match(/ disabled=""/g)).toHaveLength(2);
  });

  it("refuse a tab drawn outside them", () => {
    expect(() =>
      renderToStaticMarkup(
        <TabTrigger id="x" value="x" controls="y">
          X
        </TabTrigger>,
      ),
    ).toThrow("a tab outside its tabs");
  });
});
