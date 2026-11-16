// A select's popup, as it is drawn and as it is used. The use is the component called as React
// would draw it, its hooks answered here (`runtime`): the states a test gives, a spy for each
// setter, its refs kept from one draw to the next, and its effects held for the test to run; the
// handlers it draws are then called as the browser would.
/* biome-ignore-all lint/correctness/useHookAtTopLevel: the hooks mocked here answer for a component
 * called outside a render, and call React's own only while React renders. */

import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, type Mock, vi } from "vitest";
import {
  arrowsFrom,
  choosesOnClick,
  clickChooses,
  nextIndex,
  previousIndex,
  SelectPopup,
  type SelectPopupProps,
  staysWithin,
  tapped,
  typesLetter,
} from "./select-popup";
import { elementOf, type Found } from "./testing";
import { TYPEAHEAD_MS } from "./typeahead";

/** The hooks of the component drawn while `on`; React's own otherwise. */
const runtime = vi.hoisted(() => ({
  on: false,
  /** The values the next `useState` calls give, in order; past them, each gives its initial. */
  states: [] as unknown[],
  setters: [] as Mock[],
  refs: [] as { current: unknown }[],
  ref: 0,
  effects: [] as (() => undefined | (() => void))[],
}));

vi.mock("react", async (original) => {
  const actual = await original<typeof import("react")>();
  return {
    ...actual,
    useState: (initial: unknown) => {
      if (!runtime.on) return actual.useState(initial);
      const value = runtime.states.length > 0 ? runtime.states.shift() : initial;
      const set = vi.fn();
      runtime.setters.push(set);
      return [value, set];
    },
    useRef: (initial: unknown) => {
      if (!runtime.on) return actual.useRef(initial);
      const at = runtime.ref++;
      runtime.refs[at] ??= { current: initial };
      return runtime.refs[at];
    },
    useEffect: (effect: () => undefined | (() => void), deps?: unknown[]) => {
      if (!runtime.on) return actual.useEffect(effect, deps);
      runtime.effects.push(effect);
    },
  };
});

/** Where the popup was anchored, and whether the anchor was let go. */
const anchored = vi.hoisted(() => ({ calls: [] as [string, string][], released: 0 }));

vi.mock("./popup-position", () => ({
  anchor: (trigger: string, popup: string) => {
    anchored.calls.push([trigger, popup]);
    return () => {
      anchored.released += 1;
    };
  },
}));

/** A node of the page, as far as the popup asks one: what it holds. */
class Box {
  constructor(readonly inside: Box[] = []) {}
  contains(other: unknown): boolean {
    return other === this || this.inside.some((box) => box.contains(other));
  }
}

/** A node of the page outside the popup. */
const elsewhere = () => new Box() as unknown as Node;

/** The popup's list, as its ref holds it, with an element for each of its three options. */
function placed() {
  const options = [new Box(), new Box(), new Box()];
  const list = new Box(options);
  runtime.refs[0] = { current: list };
  return { list: list as unknown as Node, options: options as unknown as Node[] };
}

beforeEach(() => {
  runtime.refs = [];
  anchored.calls = [];
  anchored.released = 0;
  vi.stubGlobal("Node", Box);
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

const PRIORITIES: [string, string][] = [
  ["0", "Urgent"],
  ["1", "High"],
  ["2", "Normal"],
];

/** The popup's props: the priority field's, `2` chosen, with a spy for each callback. */
function props(fields: Partial<SelectPopupProps> = {}): SelectPopupProps {
  return {
    id: "priority",
    label: "Priority",
    value: "2",
    options: PRIORITIES,
    onChange: vi.fn(),
    onClose: vi.fn(),
    ...fields,
  };
}

/** The popup drawn with `states` (open, then the option focused), and its setters. */
function draw(given: SelectPopupProps, states: unknown[] = []) {
  runtime.on = true;
  runtime.states = [...states];
  runtime.setters = [];
  runtime.ref = 0;
  runtime.effects = [];
  try {
    const element = SelectPopup(given) as ReactElement<Record<string, unknown>>;
    const [setOpen, setFocused] = runtime.setters as [Mock, Mock];
    return { element, setOpen, setFocused, effects: runtime.effects };
  } finally {
    runtime.on = false;
  }
}

const isList = (element: Found) => element.props.role === "listbox";
const option = (at: number) => (element: Found) => element.props.id === `priority-popup-${at}`;

/** A key event: its key, its physical code, and spies for what a handler stops. */
const key = (name: string, code = "") => ({
  key: name,
  code,
  preventDefault: vi.fn(),
  stopPropagation: vi.fn(),
});

/** A pointer event of `button` at `x`, `y`, a mouse's unless `pointerType` says otherwise. */
const pointer = (x: number, y: number, button = 0, pointerType = "mouse") => ({
  button,
  clientX: x,
  clientY: y,
  pointerType,
  preventDefault: vi.fn(),
});

function handler(element: ReactElement, test: (found: Found) => boolean, name: string) {
  const found = elementOf(element, test)?.props[name];
  if (typeof found !== "function") throw new Error(`No ${name} was drawn`);
  return found as (event: unknown) => void;
}

describe("the focus moved round the list", () => {
  it("goes to the next option, from the last to the first, the first from none", () => {
    expect(nextIndex(3, null)).toBe(0);
    expect(nextIndex(3, 0)).toBe(1);
    expect(nextIndex(3, 2)).toBe(0);
    expect(nextIndex(0, null)).toBeNull();
  });

  it("goes to the option before, from the first to the last, the last from none", () => {
    expect(previousIndex(3, null)).toBe(2);
    expect(previousIndex(3, 0)).toBe(2);
    expect(previousIndex(3, 2)).toBe(1);
    expect(previousIndex(0, 1)).toBeNull();
  });

  it("starts from the focused option, else the chosen one, else none", () => {
    expect(arrowsFrom(PRIORITIES, "2", 0)).toBe(0);
    expect(arrowsFrom(PRIORITIES, "1", null)).toBe(1);
    expect(arrowsFrom(PRIORITIES, "9", null)).toBeNull();
    expect(arrowsFrom([], "1", null)).toBeNull();
  });
});

describe("what a key or a pointer means", () => {
  it("types a letter when it names one character, of any script", () => {
    expect(typesLetter("a")).toBe(true);
    expect(typesLetter("ф")).toBe(true);
    expect(typesLetter("😀")).toBe(true);
    expect(typesLetter("Enter")).toBe(false);
    expect(typesLetter("ArrowDown")).toBe(false);
  });

  it("chooses on a mouse's release, and on a touch's that moved 5 pixels at most", () => {
    expect(tapped([10, 10], [200, 200], false)).toBe(true);
    expect(tapped([10, 10], [13, 14], true)).toBe(true);
    expect(tapped([10, 10], [14, 14], true)).toBe(false);
  });

  it("chooses on a click its tap's, or one with no release before it while open", () => {
    expect(clickChooses({ key: "1", chooses: true }, "1", true)).toBe(true);
    expect(clickChooses({ key: "1", chooses: true }, "1", false)).toBe(true);
    expect(clickChooses({ key: "1", chooses: true }, "0", true)).toBe(false);
    expect(clickChooses({ key: "1", chooses: false }, "1", true)).toBe(false);
    expect(clickChooses(null, "1", true)).toBe(true);
    expect(clickChooses(null, "1", false)).toBe(false);
  });

  it("keeps the focus within what holds it, the node itself among it, and nothing else", () => {
    const { list, options } = placed();
    expect(staysWithin(list, options[1] ?? null)).toBe(true);
    expect(staysWithin(list, list)).toBe(true);
    expect(staysWithin(list, elsewhere())).toBe(false);
    expect(staysWithin(list, null)).toBe(false);
    expect(staysWithin(list, {} as EventTarget)).toBe(false);
    expect(staysWithin(null, list)).toBe(false);
  });
});

describe("a select's popup drawn", () => {
  it("is its list, open and hidden until placed, labelled, its options with the chosen one checked", () => {
    const html = renderToStaticMarkup(<SelectPopup {...props()} />);
    expect(html).toMatch(
      /^<div class="dx-select" data-symphony-control="select" data-state="open"><div id="priority-popup" role="listbox" tabindex="0" aria-multiselectable="false" data-state="open" class="dx-select-list" style="position:fixed;visibility:hidden" aria-label="Priority">/,
    );
    expect(html).toContain(
      '<div role="option" id="priority-popup-0" tabindex="-1" aria-selected="false" aria-disabled="false" data-disabled="false" class="dx-select-option">Urgent</div>',
    );
    expect(html).toContain(
      '<div role="option" id="priority-popup-2" tabindex="-1" aria-selected="true" aria-disabled="false" data-disabled="false" class="dx-select-option">Normal<svg xmlns="http://www.w3.org/2000/svg" width="1rem" height="1rem" viewBox="0 0 24 24" fill="none" stroke="var(--secondary-color-5)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"></path></svg></div>',
    );
    expect(html.match(/<svg/g)).toHaveLength(1);
  });

  it("lets the focused option take Tab's place, and closed, neither", () => {
    const focused = draw(props(), [true, 1]).element;
    expect(elementOf(focused, isList)?.props.tabIndex).toBe(-1);
    expect(elementOf(focused, option(1))?.props.tabIndex).toBe(0);
    expect(elementOf(focused, option(0))?.props.tabIndex).toBe(-1);
    const closed = draw(props(), [false, null]).element;
    expect(closed.props["data-state"]).toBe("closed");
    expect(elementOf(closed, isList)?.props.tabIndex).toBe(-1);
  });
});

describe("a select's popup used", () => {
  it("chooses the focused option with Enter or Space, then closes", () => {
    for (const name of ["Enter", " "]) {
      const given = props();
      const { element, setOpen } = draw(given, [true, 1]);
      const event = key(name);
      handler(element, isList, "onKeyDown")(event);
      expect(given.onChange).toHaveBeenCalledWith("1");
      expect(setOpen).toHaveBeenCalledWith(false);
      expect(given.onClose).toHaveBeenCalledOnce();
      expect(event.preventDefault).toHaveBeenCalled();
      expect(event.stopPropagation).toHaveBeenCalled();
    }
  });

  it("chooses nothing with Enter while no option has the focus, or once closed", () => {
    for (const states of [
      [true, null],
      [false, 1],
      [true, 7],
    ]) {
      const given = props();
      const event = key("Enter");
      handler(draw(given, states).element, isList, "onKeyDown")(event);
      expect(given.onChange).not.toHaveBeenCalled();
      expect(given.onClose).not.toHaveBeenCalled();
      expect(event.preventDefault).toHaveBeenCalled();
    }
  });

  it("moves the focus with the arrows, Home and End, forgetting what was typed", () => {
    const moves: [string, number | null, number | null][] = [
      ["ArrowDown", 0, 1],
      ["ArrowDown", 2, 0],
      ["ArrowUp", 0, 2],
      ["ArrowUp", 1, 0],
      ["Home", 2, 0],
      ["End", 0, 2],
    ];
    for (const [name, from, to] of moves) {
      const { element, setFocused } = draw(props({ value: "1" }), [true, from]);
      (runtime.refs[1] as { current: { typed: string } }).current.typed = "no";
      const event = key(name);
      handler(element, isList, "onKeyDown")(event);
      expect(setFocused, `${name} from ${from}`).toHaveBeenCalledWith(to);
      expect((runtime.refs[1] as { current: { typed: string } }).current.typed).toBe("");
      expect(event.preventDefault).toHaveBeenCalled();
      expect(event.stopPropagation).toHaveBeenCalled();
    }
  });

  it("moves the arrows from the chosen option while none has the focus, from the ends with none chosen", () => {
    const moves: [string, string, number][] = [
      ["ArrowUp", "1", 0],
      ["ArrowDown", "1", 2],
      ["ArrowUp", "0", 2],
      ["ArrowDown", "2", 0],
      ["ArrowUp", "9", 2],
      ["ArrowDown", "9", 0],
    ];
    for (const [name, value, to] of moves) {
      const { element, setFocused } = draw(props({ value }), [true, null]);
      handler(element, isList, "onKeyDown")(key(name));
      expect(setFocused, `${name} with ${value} chosen`).toHaveBeenCalledWith(to);
    }
  });

  it("moves no focus among no options", () => {
    for (const name of ["ArrowDown", "ArrowUp", "Home", "End"]) {
      const { element, setFocused } = draw(props({ options: [] }), [true, null]);
      handler(element, isList, "onKeyDown")(key(name));
      expect(setFocused).toHaveBeenCalledWith(null);
    }
  });

  it("focuses the option nearest what was typed within a second, learning the keyboard", async () => {
    vi.useFakeTimers();
    const given = props();
    const first = draw(given, [true, null]);
    handler(first.element, isList, "onKeyDown")(key("n", "KeyN"));
    expect(first.setFocused).toHaveBeenCalledWith(2);
    const search = (
      runtime.refs[1] as { current: { typed: string; keyboard: { layout: unknown } } }
    ).current;
    expect(search.typed).toBe("n");
    expect(search.keyboard.layout).toBe("qwerty");
    // A key typing what no layout puts there teaches the keyboard that none it knows is the
    // reader's.
    const second = draw(given, [true, 2]);
    handler(second.element, isList, "onKeyDown")(key("ф", "KeyA"));
    expect(search.typed).toBe("nф");
    expect(search.keyboard.layout).toBeNull();
    await vi.advanceTimersByTimeAsync(TYPEAHEAD_MS - 1);
    expect(search.typed).toBe("nф");
    await vi.advanceTimersByTimeAsync(1);
    expect(search.typed).toBe("");
    // Typed again: a search of its own.
    const third = draw(given, [true, 2]);
    handler(third.element, isList, "onKeyDown")(key("U", "KeyU"));
    expect(third.setFocused).toHaveBeenCalledWith(0);
    expect(search.typed).toBe("U");
  });

  it("focuses nothing for a letter typed among no options", () => {
    const { element, setFocused } = draw(props({ options: [] }), [true, null]);
    handler(element, isList, "onKeyDown")(key("a", "KeyA"));
    expect(setFocused).not.toHaveBeenCalled();
  });

  it("closes with Escape, once", () => {
    const given = props();
    const { element, setOpen } = draw(given, [true, null]);
    const event = key("Escape");
    handler(element, isList, "onKeyDown")(event);
    expect(setOpen).toHaveBeenCalledWith(false);
    expect(given.onClose).toHaveBeenCalledOnce();
    expect(given.onChange).not.toHaveBeenCalled();
    expect(event.preventDefault).toHaveBeenCalled();
    expect(event.stopPropagation).toHaveBeenCalled();
    handler(draw(given, [false, null]).element, isList, "onKeyDown")(key("Escape"));
    expect(given.onClose).toHaveBeenCalledOnce();
  });

  it("lets any other key go on as it would", () => {
    const given = props();
    const { element, setFocused, setOpen } = draw(given, [true, 1]);
    const event = key("Tab");
    handler(element, isList, "onKeyDown")(event);
    expect(event.preventDefault).not.toHaveBeenCalled();
    expect(setFocused).not.toHaveBeenCalled();
    expect(setOpen).not.toHaveBeenCalled();
  });

  it("chooses the option a pointer pressed and released, keeping the focus where it was", () => {
    const given = props();
    const { element } = draw(given, [true, null]);
    const press = pointer(10, 10);
    handler(element, option(0), "onPointerDown")(press);
    expect(press.preventDefault).toHaveBeenCalled();
    handler(element, option(0), "onPointerUp")(pointer(40, 12));
    expect(given.onChange).toHaveBeenCalledWith("0");
    expect(given.onClose).toHaveBeenCalledOnce();
    // Released again with no press: nothing.
    handler(element, option(0), "onPointerUp")(pointer(40, 12));
    expect(given.onChange).toHaveBeenCalledOnce();
  });

  it("chooses nothing for a press on one option released on another, as the primitive", () => {
    const given = props();
    const { element } = draw(given, [true, null]);
    handler(element, option(0), "onPointerDown")(pointer(10, 10));
    handler(element, option(1), "onPointerUp")(pointer(10, 40));
    expect(given.onChange).not.toHaveBeenCalled();
    // The press is spent: a release back on the option it went down on chooses nothing either.
    handler(element, option(0), "onPointerUp")(pointer(10, 10));
    expect(given.onChange).not.toHaveBeenCalled();
    expect(given.onClose).not.toHaveBeenCalled();
    // A touch moved onto another option is no tap of either.
    handler(element, option(2), "onPointerDown")(pointer(10, 70, 0, "touch"));
    handler(element, option(1), "onPointerUp")(pointer(10, 70, 0, "touch"));
    expect(given.onChange).not.toHaveBeenCalled();
    // A press and its release on one option choose it.
    handler(element, option(1), "onPointerDown")(pointer(10, 40));
    handler(element, option(1), "onPointerUp")(pointer(30, 42));
    expect(given.onChange).toHaveBeenCalledExactlyOnceWith("1");
  });

  it("takes no other button, a touch that scrolled, or a press the browser cancelled", () => {
    const given = props();
    const { element } = draw(given, [true, null]);
    const right = pointer(10, 10, 2);
    handler(element, option(1), "onPointerDown")(right);
    expect(right.preventDefault).not.toHaveBeenCalled();
    handler(element, option(1), "onPointerUp")(pointer(10, 10, 2));
    handler(element, option(1), "onPointerDown")(pointer(10, 10, 0, "touch"));
    handler(element, option(1), "onPointerUp")(pointer(10, 40, 0, "touch"));
    handler(element, option(1), "onPointerDown")(pointer(10, 10, 0, "touch"));
    handler(element, option(1), "onPointerCancel")({});
    handler(element, option(1), "onPointerUp")(pointer(10, 10, 0, "touch"));
    expect(given.onChange).not.toHaveBeenCalled();
    // A touch that stayed is a tap, chosen on its click.
    handler(element, option(1), "onPointerDown")(pointer(10, 10, 0, "touch"));
    handler(element, option(1), "onPointerUp")(pointer(12, 13, 0, "touch"));
    handler(element, option(1), "onClick")({});
    expect(given.onChange).toHaveBeenCalledWith("1");
  });

  it("chooses a finger's or a pen's tap on the option's own click, so it lands on nothing under it", () => {
    expect(["mouse", "touch", "pen", ""].map(choosesOnClick)).toEqual([false, true, true, false]);
    for (const pointerType of ["touch", "pen"]) {
      const given = props();
      const { element } = draw(given, [true, null]);
      handler(element, option(1), "onPointerDown")(pointer(10, 40, 0, pointerType));
      handler(element, option(1), "onPointerUp")(pointer(11, 41, 0, pointerType));
      // Released, the popup stays open for the tap's click, which another option's is not.
      expect(given.onChange).not.toHaveBeenCalled();
      expect(given.onClose).not.toHaveBeenCalled();
      handler(element, option(1), "onClick")({});
      expect(given.onChange).toHaveBeenCalledExactlyOnceWith("1");
      expect(given.onClose).toHaveBeenCalledOnce();
      // Chosen, the popup is drawn closed: another click on it is nothing.
      handler(draw(given, [false, null]).element, option(1), "onClick")({});
      expect(given.onChange).toHaveBeenCalledOnce();
      // Another option's click after a tap is not the tap's.
      const other = props();
      const again = draw(other, [true, null]).element;
      handler(again, option(1), "onPointerDown")(pointer(10, 40, 0, pointerType));
      handler(again, option(1), "onPointerUp")(pointer(11, 41, 0, pointerType));
      handler(again, option(0), "onClick")({});
      expect(other.onChange).not.toHaveBeenCalled();
    }
  });

  it("takes the click after a mouse's release, a touch that moved, or a press let go elsewhere, for nothing", () => {
    const given = props();
    const { element } = draw(given, [true, null]);
    // A mouse pressed on one option and released on another: its click is on neither, and a
    // click on the one released on is not a choice.
    handler(element, option(0), "onPointerDown")(pointer(10, 10));
    handler(element, option(1), "onPointerUp")(pointer(10, 40));
    handler(element, option(1), "onClick")({});
    // A touch that moved further than a tap, whose click a browser sends all the same.
    handler(element, option(2), "onPointerDown")(pointer(10, 70, 0, "touch"));
    handler(element, option(2), "onPointerUp")(pointer(10, 80, 0, "touch"));
    handler(element, option(2), "onClick")({});
    // A release with no press before it.
    handler(element, option(2), "onPointerUp")(pointer(10, 70));
    handler(element, option(2), "onClick")({});
    expect(given.onChange).not.toHaveBeenCalled();
    expect(given.onClose).not.toHaveBeenCalled();
  });

  it("chooses on a click with no pointer released before it, as an assistive technology's, while open", () => {
    const given = props();
    // Gecko's activation, or a script's click(): the click alone.
    handler(draw(given, [true, null]).element, option(0), "onClick")({});
    expect(given.onChange).toHaveBeenCalledExactlyOnceWith("0");
    expect(given.onClose).toHaveBeenCalledOnce();
    // A click once spent leaves the next to stand alone: a tap's click, then a click of its own.
    const afterTap = props();
    const { element } = draw(afterTap, [true, null]);
    handler(element, option(1), "onPointerDown")(pointer(10, 40, 0, "touch"));
    handler(element, option(1), "onPointerUp")(pointer(10, 40, 0, "touch"));
    handler(element, option(0), "onClick")({});
    expect(afterTap.onChange).not.toHaveBeenCalled();
    handler(element, option(2), "onClick")({});
    expect(afterTap.onChange).toHaveBeenCalledExactlyOnceWith("2");
    // Closed, a click alone chooses nothing.
    const closed = props({ closed: true });
    handler(draw(closed, [true, null]).element, option(0), "onClick")({});
    expect(closed.onChange).not.toHaveBeenCalled();
  });

  // Each engine's activation of an option, as its own sources send it (the module's header).
  it("chooses once for each engine's activation of an option, its popup open until it chooses", () => {
    // Blink, for TalkBack or any assistive technology's default action: the option focused first,
    // then a mouse's press and release at its middle, then its click.
    const blink = props();
    const { list, options } = placed();
    const first = draw(blink, [true, null]);
    handler(
      first.element,
      isList,
      "onBlur",
    )({
      target: list,
      currentTarget: list,
      relatedTarget: options[1],
    });
    handler(first.element, option(1), "onFocus")({});
    expect(first.setFocused).toHaveBeenCalledExactlyOnceWith(1);
    expect(blink.onClose).not.toHaveBeenCalled();
    const focused = draw(blink, [true, 1]).element;
    handler(focused, option(1), "onPointerDown")(pointer(60, 40));
    handler(focused, option(1), "onPointerUp")(pointer(60, 40));
    expect(blink.onChange).toHaveBeenCalledExactlyOnceWith("1");
    handler(draw(blink, [false, 1]).element, option(1), "onClick")({});
    expect(blink.onChange).toHaveBeenCalledOnce();
    expect(blink.onClose).toHaveBeenCalledOnce();
    // WebKit on iOS, for VoiceOver, on a page that hears touches: a touch, then the click.
    const ios = props();
    const touched = draw(ios, [true, null]).element;
    handler(touched, option(2), "onPointerDown")(pointer(60, 70, 0, "touch"));
    handler(touched, option(2), "onPointerUp")(pointer(60, 70, 0, "touch"));
    expect(ios.onChange).not.toHaveBeenCalled();
    handler(touched, option(2), "onClick")({});
    expect(ios.onChange).toHaveBeenCalledExactlyOnceWith("2");
    // WebKit on the Mac: a mouse's pointer events, then the click.
    const mac = props();
    const pressed = draw(mac, [true, null]).element;
    handler(pressed, option(0), "onPointerDown")(pointer(60, 10));
    handler(pressed, option(0), "onPointerUp")(pointer(60, 10));
    handler(draw(mac, [false, null]).element, option(0), "onClick")({});
    expect(mac.onChange).toHaveBeenCalledExactlyOnceWith("0");
    expect(mac.onClose).toHaveBeenCalledOnce();
  });

  it("closes once the focus leaves the list, or the option that held it", () => {
    const given = props();
    const list = draw(given, [true, null]);
    const own = {};
    handler(list.element, isList, "onBlur")({ target: own, currentTarget: own });
    expect(given.onClose).toHaveBeenCalledOnce();
    const focused = draw(props(), [true, 1]);
    // An option's blur reaches the list too, and is the option's alone.
    handler(focused.element, isList, "onBlur")({ target: {}, currentTarget: own });
    handler(focused.element, isList, "onBlur")({ target: own, currentTarget: own });
    handler(focused.element, option(0), "onBlur")({});
    expect(focused.setOpen).not.toHaveBeenCalled();
    handler(focused.element, option(1), "onBlur")({ relatedTarget: elsewhere() });
    expect(focused.setFocused).toHaveBeenCalledWith(null);
    expect(focused.setOpen).toHaveBeenCalledWith(false);
  });

  it("stays open as the focus moves among its options or to the list, as a screen reader moves it", () => {
    const given = props();
    const { list, options } = placed();
    // From the list to an option, which then holds it.
    const opened = draw(given, [true, null]);
    handler(
      opened.element,
      isList,
      "onBlur",
    )({
      target: list,
      currentTarget: list,
      relatedTarget: options[2],
    });
    handler(opened.element, option(2), "onFocus")({});
    expect(opened.setFocused).toHaveBeenCalledExactlyOnceWith(2);
    // From one option to another: the one it left lets go of nothing, the other takes it.
    const moved = draw(given, [true, 2]);
    handler(moved.element, option(2), "onBlur")({ relatedTarget: options[0] });
    handler(moved.element, option(0), "onFocus")({});
    expect(moved.setFocused).toHaveBeenCalledExactlyOnceWith(0);
    // From an option to the list itself (a press on its edge): no option holds it.
    const edge = draw(given, [true, 0]);
    handler(edge.element, option(0), "onBlur")({ relatedTarget: list });
    handler(edge.element, isList, "onFocus")({ target: options[0], currentTarget: list });
    expect(edge.setFocused).not.toHaveBeenCalled();
    handler(edge.element, isList, "onFocus")({ target: list, currentTarget: list });
    expect(edge.setFocused).toHaveBeenCalledExactlyOnceWith(null);
    // The focus the popup gives itself moves nothing.
    const onOption = draw(given, [true, 1]);
    handler(onOption.element, option(1), "onFocus")({});
    const onList = draw(given, [true, null]);
    handler(onList.element, isList, "onFocus")({ target: list, currentTarget: list });
    for (const set of [onOption.setFocused, onList.setFocused, onOption.setOpen, onList.setOpen])
      expect(set).not.toHaveBeenCalled();
    expect(given.onClose).not.toHaveBeenCalled();
    expect(given.onChange).not.toHaveBeenCalled();
  });

  // The list's edge given the focus by a press leaves no option holding it, so the arrows start
  // again as at the opening: from the chosen option (here High, 1), neither from the option last
  // focused (Urgent, 0) nor from the list's first.
  it("moves the arrows from the chosen option after a press on the list's edge", () => {
    for (const [name, to] of [
      ["ArrowDown", 2],
      ["ArrowUp", 0],
    ] as const) {
      const given = props({ value: "1" });
      const { list, options } = placed();
      const focused = draw(given, [true, 0]);
      handler(focused.element, option(0), "onBlur")({ relatedTarget: list });
      handler(focused.element, isList, "onFocus")({ target: list, currentTarget: list });
      expect(focused.setFocused).toHaveBeenCalledExactlyOnceWith(null);
      expect(focused.setOpen).not.toHaveBeenCalled();
      const edge = draw(given, [true, null]);
      handler(edge.element, isList, "onKeyDown")(key(name));
      expect(edge.setFocused, name).toHaveBeenCalledExactlyOnceWith(to);
      // And so after the focus moved from an option to another, the arrows go on from that one.
      const moved = draw(given, [true, 0]);
      handler(moved.element, option(0), "onBlur")({ relatedTarget: options[2] });
      handler(moved.element, option(2), "onFocus")({});
      const after = draw(given, [true, 2]);
      handler(after.element, isList, "onKeyDown")(key(name));
      expect(after.setFocused, name).toHaveBeenCalledExactlyOnceWith(name === "ArrowDown" ? 0 : 1);
      expect(given.onClose).not.toHaveBeenCalled();
    }
  });

  it("is anchored beside its trigger until it goes, its search forgotten with it", async () => {
    vi.useFakeTimers();
    const { element, effects } = draw(props(), [true, null]);
    const [place, forget] = effects;
    const release = place?.();
    expect(anchored.calls).toEqual([["priority", "priority-popup"]]);
    release?.();
    expect(anchored.released).toBe(1);
    handler(element, isList, "onKeyDown")(key("h", "KeyH"));
    const search = (runtime.refs[1] as { current: { typed: string } }).current;
    forget?.()?.();
    expect(vi.getTimerCount()).toBe(0);
    await vi.advanceTimersByTimeAsync(TYPEAHEAD_MS);
    expect(search.typed).toBe("h");
  });

  it("gives the focus to its list until an option takes it, and none once closed", () => {
    const list = { focus: vi.fn() };
    const chosen = { focus: vi.fn() };
    vi.stubGlobal("document", {
      getElementById: (id: string) => (id === "priority-popup-1" ? chosen : null),
    });
    const opened = draw(props(), [true, null]);
    runtime.refs[0] = { current: list };
    draw(props(), [true, null]).effects[2]?.();
    expect(list.focus).toHaveBeenCalledOnce();
    draw(props(), [true, 1]).effects[2]?.();
    expect(chosen.focus).toHaveBeenCalledOnce();
    draw(props(), [true, 2]).effects[2]?.();
    draw(props(), [false, null]).effects[2]?.();
    expect(list.focus).toHaveBeenCalledOnce();
    expect(opened.effects).toHaveLength(4);
  });

  it("closed by its request, is drawn closed and takes no focus, key or choice", () => {
    const given = props({ closed: true });
    const list = { focus: vi.fn() };
    runtime.refs[0] = { current: list };
    const { element, setOpen, effects } = draw(given, [true, 1]);
    expect(element.props["data-state"]).toBe("closed");
    expect(elementOf(element, isList)?.props["data-state"]).toBe("closed");
    expect(elementOf(element, isList)?.props.tabIndex).toBe(-1);
    effects[2]?.();
    expect(list.focus).not.toHaveBeenCalled();
    for (const name of ["Escape", "Enter"]) handler(element, isList, "onKeyDown")(key(name));
    expect(setOpen).not.toHaveBeenCalled();
    expect(given.onClose).not.toHaveBeenCalled();
    expect(given.onChange).not.toHaveBeenCalled();
  });

  it("is let go once its own exit has played, or at once with none playing", () => {
    const onLeft = vi.fn();
    const list = { getAnimations: () => [{ playState: "running" }] };
    runtime.refs[0] = { current: list };
    const { element, effects } = draw(props({ closed: true, onLeft }), [true, null]);
    effects[3]?.();
    expect(onLeft).not.toHaveBeenCalled();
    const ended = handler(element, isList, "onAnimationEnd");
    ended({ target: {}, currentTarget: list });
    expect(onLeft).not.toHaveBeenCalled();
    ended({ target: list, currentTarget: list });
    expect(onLeft).toHaveBeenCalledOnce();
    list.getAnimations = () => [{ playState: "finished" }];
    effects[3]?.();
    expect(onLeft).toHaveBeenCalledTimes(2);
    // Open, neither its entrance's end nor the effect lets it go.
    const open = draw(props({ onLeft }), [true, null]);
    handler(open.element, isList, "onAnimationEnd")({ target: list, currentTarget: list });
    open.effects[3]?.();
    expect(onLeft).toHaveBeenCalledTimes(2);
  });
});
