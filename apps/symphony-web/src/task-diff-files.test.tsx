import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DiffFileList, File, Patch } from "./task-diff-files";
import { fire, reading, reply, server, turn, until } from "./testing";

// React's state and effects stood in for, as no DOM runs here: a part's states are kept by the
// order it asks for them, from one draw to the next, and each effect it sets is kept to be run by
// its test.
const react = vi.hoisted(() => ({
  effects: [] as (() => undefined | (() => void))[],
  states: [] as unknown[],
  at: 0,
}));

vi.mock("react", async (original) => ({
  ...(await original<typeof import("react")>()),
  useEffect: (effect: () => undefined | (() => void)) => {
    react.effects.push(effect);
  },
  useLayoutEffect: (effect: () => undefined | (() => void)) => {
    react.effects.push(effect);
  },
  useState: (first: unknown) => {
    const at = react.at;
    react.at += 1;
    if (!(at in react.states)) react.states[at] = typeof first === "function" ? first() : first;
    const set = (next: unknown) => {
      react.states[at] = typeof next === "function" ? next(react.states[at]) : next;
    };
    return [react.states[at], set];
  },
}));

/** One draw of `part`: its markup, and the effects it set, which the test runs. */
function draw(part: ReactElement) {
  react.at = 0;
  react.effects.length = 0;
  const html = renderToStaticMarkup(part);
  return { html, effects: [...react.effects] };
}

const PATCH = { scope: "wt", slug: "tools/x", base: "a", head: "b", path: "one.ts" };
const patch = <Patch {...PATCH} />;
const ANSWER = { base: "a", head: "b", path: "one.ts", patch: "@@\n+added\n", truncated: false };

beforeEach(() => {
  react.states.length = 0;
});

afterEach(() => vi.unstubAllGlobals());

describe("a file's patch", () => {
  it("is read under the checkout's scope at the comparison's commits, then drawn", async () => {
    const asked = server();
    const first = draw(patch);
    expect(first.html).toBe(
      '<p role="status" aria-busy="true">Reading this file&#x27;s patch…</p>',
    );
    first.effects[0]?.();
    await until(() => expect(asked).toHaveLength(1));
    expect(asked[0]?.url).toBe("/@wt/api/task-diff-file?slug=tools%2Fx&base=a&head=b&path=one.ts");
    asked[0]?.answer(reply(200, ANSWER));
    const html = await until(() => {
      const drawn = draw(patch).html;
      expect(drawn).toContain("+added");
      return drawn;
    });
    expect(html).toContain('aria-label="File patch"');
    expect(html).not.toContain("Reading this file");
  });

  it("refuses another comparison's answer, and reads again on its Retry", async () => {
    const asked = server();
    draw(patch).effects[0]?.();
    await until(() => expect(asked).toHaveLength(1));
    asked[0]?.answer(reply(200, { ...ANSWER, head: "c" }));
    const html = await until(() => {
      const drawn = draw(patch).html;
      expect(drawn).toContain("Retry patch");
      return drawn;
    });
    expect(html).toContain("The file patch is not readable");
    react.at = 0;
    fire(
      (patch.type as (props: unknown) => ReactElement)(patch.props),
      reading("Retry patch"),
      "onClick",
    );
    const again = draw(patch);
    again.effects[0]?.();
    expect(draw(patch).html).toContain("Reading this file");
    await until(() => expect(asked).toHaveLength(2));
    asked[1]?.answer(reply(200, ANSWER));
    await until(() => expect(draw(patch).html).toContain("+added"));
  });

  it("says the server's refusal", async () => {
    const asked = server();
    draw(patch).effects[0]?.();
    await until(() => expect(asked).toHaveLength(1));
    asked[0]?.answer(reply(503, { error: "Board unavailable" }));
    await until(() => expect(draw(patch).html).toContain("Board unavailable"));
  });

  it("is given up as its comparison leaves, and sets nothing after", async () => {
    const asked = server();
    const leave = draw(patch).effects[0]?.();
    await until(() => expect(asked).toHaveLength(1));
    leave?.();
    expect(asked[0]?.signal?.aborted).toBe(true);
    asked[0]?.answer(reply(200, ANSWER));
    await turn();
    expect(draw(patch).html).toContain("Reading this file");
  });
});

describe("a changed file", () => {
  const file = (added: number | null, removed: number | null) => (
    <File file={{ path: "one.ts", added, removed }} scope="" slug="tools/x" base="a" head="b" />
  );

  it("says its counts, or that it is binary, and reads its patch only once opened", () => {
    const counted = draw(file(2, 1)).html;
    expect(counted).toContain('<span data-change="added">+2</span>');
    expect(counted).toContain('<span data-change="removed">−1</span>');
    expect(counted).not.toContain("Reading this file");
    expect(draw(file(null, null)).html).toContain("Binary file");
    const element = file(2, 1);
    const tree = () => (element.type as (props: unknown) => ReactElement)(element.props);
    const toggle = (open: boolean) => {
      react.at = 0;
      fire(tree(), (found) => found.type === "details", "onToggle", { currentTarget: { open } });
    };
    toggle(false);
    expect(draw(element).html).not.toContain("Reading this file");
    toggle(true);
    expect(draw(element).html).toContain("Reading this file");
    // Closed again, its patch stays read.
    toggle(false);
    expect(draw(element).html).toContain("Reading this file");
  });
});

describe("the comparison's files", () => {
  it("are one each, under the comparison's commits, and put their style in the page once", () => {
    const sheets: { id: string; textContent: string }[] = [];
    const theme = { theme: true };
    const inserted: unknown[] = [];
    vi.stubGlobal("document", {
      getElementById: (id: string) => sheets.find((sheet) => sheet.id === id) ?? null,
      createElement: () => ({ id: "", textContent: "" }),
      head: {
        querySelector: (selector: string) => (selector === "[data-symphony-theme]" ? theme : null),
        insertBefore: (element: { id: string; textContent: string }, before: unknown) => {
          sheets.push(element);
          inserted.push(before);
        },
      },
    });
    const list = (
      <DiffFileList
        files={[
          { path: "one.ts", added: 1, removed: 0 },
          { path: "two.ts", added: 0, removed: 3 },
        ]}
        scope=""
        slug="tools/x"
        base="a"
        head="b"
      />
    );
    const { html, effects } = draw(list);
    expect(html.match(/<details>/g)).toHaveLength(2);
    expect(html).toContain("one.ts");
    expect(html).toContain("two.ts");
    for (const effect of effects) effect();
    for (const effect of draw(list).effects) effect();
    expect(sheets).toHaveLength(1);
    expect(sheets[0]?.id).toBe("task-diff-files-style");
    // Its text is the sheet's, which Vitest empties.
    expect(typeof sheets[0]?.textContent).toBe("string");
    expect(inserted).toEqual([theme]);
  });
});
