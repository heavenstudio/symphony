import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { keyAction, type OverlayRequest, overlay, SelectField } from "./select";

afterEach(() => vi.unstubAllGlobals());

const field = (value: string, disabled = false) =>
  renderToStaticMarkup(
    <SelectField
      label="Finished tasks"
      value={value}
      options={[
        ["1", "Last 24 hours"],
        ["7", "Last week"],
      ]}
      onChange={() => {}}
      disabled={disabled}
    />,
  );

describe("a select field", () => {
  it("draws its trigger, the chosen option's text on it, its popup closed", () => {
    const html = field("7");
    expect(html).toMatch(
      /^<div class="dx-select"><button id="symphony-web-field-\d+" type="button" role="combobox" class="dx-select-trigger" aria-label="Finished tasks" aria-haspopup="listbox" aria-expanded="false" aria-controls="symphony-web-field-\d+-popup"><span>Last week<\/span><svg/,
    );
    expect(html).toContain('<path d="m6 9 6 6 6-6"></path>');
  });

  it("shows no text for a value none of its options has", () => {
    expect(field("30")).toContain("<span></span>");
  });

  it("can be drawn disabled", () => {
    expect(field("1", true)).toContain('disabled=""');
    expect(field("1")).not.toContain("disabled");
  });

  it("gives each field an id of its own", () => {
    const id = (html: string) => html.match(/id="([^"]*)"/)?.[1];
    expect(id(field("1"))).not.toBe(id(field("1")));
  });
});

describe("a key on a field's trigger", () => {
  it("closes its open popup with Escape, opens a closed one with an arrow", () => {
    expect(keyAction("Escape", true)).toBe("close");
    expect(keyAction("Escape", false)).toBeNull();
    for (const arrow of ["ArrowDown", "ArrowUp"]) {
      expect(keyAction(arrow, false)).toBe("open");
      expect(keyAction(arrow, true)).toBe("stay");
    }
    expect(keyAction("Enter", false)).toBeNull();
    expect(keyAction("a", true)).toBeNull();
  });
});

describe("the overlay module", () => {
  it("is asked when the shell gives one, and nothing happens without it", () => {
    const asked: OverlayRequest[] = [];
    vi.stubGlobal("window", { reviewOverlay: (request: OverlayRequest) => asked.push(request) });
    overlay({ kind: "close", owner: "a" });
    expect(asked).toEqual([{ kind: "close", owner: "a" }]);
    vi.stubGlobal("window", {});
    expect(() => overlay({ kind: "close", owner: "a" })).not.toThrow();
  });
});
