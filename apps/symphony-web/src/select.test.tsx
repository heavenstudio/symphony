import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  FieldControl,
  keyAction,
  type OverlayRequest,
  overlay,
  SelectField,
  type SelectFieldProps,
} from "./select";

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

/** An element of the page: its id and whether it is disabled. */
class FakeElement {
  constructor(
    readonly id: string,
    private readonly disabled = false,
  ) {}
  hasAttribute(name: string) {
    return name === "disabled" && this.disabled;
  }
}

/**
 * A field's control on a page whose overlay module records what it is asked, whose trigger records
 * its focus, and whose window records the keys it hears. What it asked and what the page saw.
 */
function controlled(field: Partial<SelectFieldProps> = {}) {
  const asked: OverlayRequest[] = [];
  const focus = vi.fn();
  const listening = new Set<unknown>();
  vi.stubGlobal("window", { reviewOverlay: (request: OverlayRequest) => asked.push(request) });
  vi.stubGlobal("document", {
    getElementById: (id: string) => (id === "field" ? { focus } : null),
  });
  vi.stubGlobal("Element", FakeElement);
  vi.stubGlobal("addEventListener", (_type: string, heard: unknown) => listening.add(heard));
  vi.stubGlobal("removeEventListener", (_type: string, heard: unknown) => listening.delete(heard));
  const onChange = vi.fn();
  const shown = vi.fn();
  const page = new AbortController();
  const control = new FieldControl(
    "field",
    {
      label: "Finished tasks",
      value: "1",
      options: [
        ["1", "Last 24 hours"],
        ["7", "Last week"],
      ],
      onChange,
      ...field,
    },
    page.signal,
    shown,
  );
  const stop = control.start();
  return { control, asked, focus, listening, onChange, shown, page, stop };
}

/** The popup `asked` opened, whose callback answers it. */
function popup(asked: OverlayRequest[]) {
  const request = asked.find((one) => one.kind === "select");
  if (request?.kind !== "select") throw new Error("no popup was opened");
  return request;
}

/** A key pressed on `target`, and whether it was kept from going on. */
function press(control: FieldControl, key: string, target: unknown) {
  const event = { key, target, preventDefault: vi.fn(), stopPropagation: vi.fn() };
  control.key(event as unknown as KeyboardEvent);
  return event;
}

/** What waits for this turn to end, run. */
const settle = () => new Promise<void>((done) => queueMicrotask(done));

describe("a field's popup", () => {
  it("opens on a click, asking the overlay module for the field as it is drawn", () => {
    const { control, asked, shown } = controlled();
    control.field = { ...control.field, value: "7" };
    control.activate();
    expect(control.open).toBe(true);
    expect(shown).toHaveBeenCalledWith(true);
    const request = popup(asked);
    expect(request).toEqual({
      kind: "select",
      owner: "field",
      id: "field",
      label: "Finished tasks",
      value: "7",
      options: [
        ["1", "Last 24 hours"],
        ["7", "Last week"],
      ],
      callback: control.answer,
    });
  });

  it("gives the choice to the page, closes, and gives the trigger its focus back", async () => {
    const { control, asked, focus, onChange, shown } = controlled();
    control.activate();
    popup(asked).callback("7");
    // Taken once the popup's own handlers have run.
    expect(onChange).not.toHaveBeenCalled();
    await settle();
    expect(onChange).toHaveBeenCalledWith("7");
    expect(shown).toHaveBeenLastCalledWith(false);
    expect(control.open).toBe(false);
    expect(asked.at(-1)).toEqual({ kind: "close", owner: "field" });
    expect(focus).toHaveBeenCalledTimes(1);
  });

  it("closes with no choice on a second click, or once the popup closes by itself", async () => {
    const { control, asked, focus, onChange } = controlled();
    control.activate();
    control.activate();
    await settle();
    expect(control.open).toBe(false);
    expect(asked.map(({ kind }) => kind)).toEqual(["select", "close"]);
    control.activate();
    popup(asked).callback(null);
    await settle();
    expect(onChange).not.toHaveBeenCalled();
    expect(focus).toHaveBeenCalledTimes(2);
    expect(control.open).toBe(false);
  });

  it("drops an answer that comes after the field is gone, or its page stopped", async () => {
    let page = controlled();
    page.control.activate();
    page.stop();
    popup(page.asked).callback("7");
    await settle();
    expect(page.onChange).not.toHaveBeenCalled();
    expect(page.focus).not.toHaveBeenCalled();
    page = controlled();
    page.control.activate();
    page.page.abort();
    popup(page.asked).callback("7");
    await settle();
    expect(page.onChange).not.toHaveBeenCalled();
    expect(page.control.open).toBe(true);
  });

  it("is closed when the field goes, its keys no longer heard", () => {
    const { control, asked, listening, stop } = controlled();
    expect(listening.has(control.key)).toBe(true);
    stop();
    expect(listening.size).toBe(0);
    expect(asked).toEqual([{ kind: "close", owner: "field" }]);
  });
});

describe("a key heard by a field", () => {
  it("opens the popup with an arrow on its trigger, and closes it with Escape", async () => {
    const { control, asked, onChange } = controlled();
    const trigger = new FakeElement("field");
    let event = press(control, "ArrowDown", trigger);
    expect(event.preventDefault).toHaveBeenCalled();
    expect(control.open).toBe(true);
    // Open, an arrow scrolls nothing and opens nothing more.
    event = press(control, "ArrowUp", trigger);
    expect(event.preventDefault).toHaveBeenCalled();
    expect(asked).toHaveLength(1);
    event = press(control, "Escape", trigger);
    expect(event.stopPropagation).toHaveBeenCalled();
    await settle();
    expect(control.open).toBe(false);
    expect(onChange).not.toHaveBeenCalled();
    expect(asked.at(-1)).toEqual({ kind: "close", owner: "field" });
  });

  it("is left alone on another element, a disabled trigger, or for another key", () => {
    const { control, asked } = controlled();
    for (const target of [new FakeElement("other"), new FakeElement("field", true), {}, null]) {
      expect(press(control, "ArrowDown", target).preventDefault).not.toHaveBeenCalled();
    }
    expect(press(control, "Enter", new FakeElement("field")).preventDefault).not.toHaveBeenCalled();
    expect(asked).toEqual([]);
  });
});
