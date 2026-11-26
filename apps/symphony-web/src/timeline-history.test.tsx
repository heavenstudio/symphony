import { isValidElement, type ReactElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import FIXTURE from "../../../crates/symphony-api/fixtures/task-timeline.json";
import { arrowPlace, stopsOf, TimelineDrawn } from "./timeline";
import {
  address,
  endpointLane,
  endpointWords,
  historyWords,
  isoTime,
  messagesOf,
  readHistory,
} from "./timeline-history";
import { lanes, readTimeline, rows, Scale } from "./timeline-layout";
import { MessagePanel, type ThreadAccess } from "./timeline-messages";
import type {
  History,
  HistorySource,
  MessageEndpoint,
  RoleSpan,
  TaskEntry,
  TaskTimeline,
  TimelineMessage,
} from "./wire";

const known = (role: string, by = "one"): MessageEndpoint => ({
  state: "known",
  candidates: [{ role, by, source: "parent/task" }],
});
const unknown: MessageEndpoint = { state: "unknown", candidates: [] };
const msg = (id = 4, at = 50): TimelineMessage => ({
  id,
  at,
  sender: "one",
  recipient: "@executor",
  kind: "msg",
  from: known("orchestrator"),
  to: known("executor", "two"),
  after_close: false,
});
const history = (): History => ({
  sources_complete: true,
  sources: [{ slug: "task", state: "complete", first_id: 1, last_id: 8, before: null }],
  roles: [
    {
      role: "orchestrator",
      by: "one",
      source: "parent/task",
      from: 20,
      from_id: 2,
      to: 80,
      to_id: 8,
      uncertain: false,
    },
  ],
  messages: [msg()],
});
const timeline = (): TaskTimeline => ({
  ...(readTimeline(FIXTURE) as TaskTimeline),
  created: 0,
  now: 100,
  closed: 90,
  stages: [],
  claims: [],
  rounds: [],
  runs: [],
  gates: [],
  history: history(),
});
const entry = (id: number): TaskEntry => ({
  message: {
    id,
    ts: 5,
    slug: "task",
    sender: "outside",
    recipient: null,
    text: "Original full text",
    kind: "msg",
    person: false,
  },
  stamp: "exact",
  blocks: [],
});

describe("recorded timeline history", () => {
  it("validates every shape used, leaves absent history absent, and rejects malformed metadata", () => {
    expect(isoTime(50.123)).toBe("1970-01-01T00:00:50.123Z");
    expect(isoTime(1e20)).toBeUndefined();
    expect(readHistory(history())).toEqual(history());
    expect(readTimeline(FIXTURE)?.history).toBeUndefined();
    expect(readTimeline({ ...FIXTURE, history: history() })?.history).toEqual(history());
    for (const bad of [
      null,
      {},
      { ...history(), roles: [{ ...history().roles[0], from: Infinity }] },
      { ...history(), messages: [{ ...msg(), to: { state: "known", candidates: [null] } }] },
      { ...history(), sources: [{ slug: "task", state: "partial", before: "2" }] },
    ]) {
      expect(readHistory(bad)).toBeNull();
    }
    expect(readTimeline({ ...FIXTURE, history: {} })).toBeNull();
  });
  it("uses only a unique known historical role; a role address stays a role even with unknown holder", () => {
    expect(endpointLane(known("orchestrator"), "one")).toBe("orchestrator");
    expect(endpointLane(unknown, "@reviewer")).toBe("reviewer");
    for (const state of ["unknown", "ambiguous", "outside", "everyone"] as const) {
      expect(endpointLane({ ...known("executor"), state }, "one")).toBe("other");
    }
    expect(
      endpointLane(
        {
          state: "known",
          candidates: [...known("executor").candidates, ...known("reviewer").candidates],
        },
        "one",
      ),
    ).toBe("other");
    expect(endpointLane(known("future"), "one")).toBe("other");
    expect(endpointWords(known("executor"))).toContain("one, executor on parent/task");
    expect(endpointWords({ ...known("executor"), state: "ambiguous" })).toContain("Ambiguous");
    expect(endpointWords(unknown)).toContain("unknown");
    expect(endpointWords({ ...unknown, state: "outside" })).toContain("Outside");
    expect(endpointWords({ ...unknown, state: "everyone" })).toBe("Everyone on task.");
    expect(endpointWords({ ...known("executor"), state: "unknown" })).toContain("possible:");
    expect(address({ ...msg(), recipient: null })).toBe("one → Everyone on task");
  });
  it("merges earlier thread metadata once, retains unknowns and orders collisions by id", () => {
    const h = { ...history(), messages: [msg(8, 50), msg(4, 50)] };
    const all = messagesOf(h, [
      entry(4),
      entry(2),
      { ...entry(3), message: { ...entry(3).message, kind: "system" } },
    ]);
    expect(all.map((m) => m.id)).toEqual([2, 4, 8]);
    expect(all[0]?.from.state).toBe("unknown");
    expect(all[0]?.to.state).toBe("everyone");
    expect(all[1]?.from.state).toBe("known");
    expect(messagesOf(undefined, [entry(2)])[0]?.after_close).toBeNull();
  });
  it("extends one common scale beyond closure and before creation without changing the closed stage", () => {
    const t = timeline();
    t.history = { ...history(), messages: [msg(4, -3601), { ...msg(8, 200), after_close: true }] };
    const scale = new Scale(t);
    expect(scale.start).toBeLessThanOrEqual(-3601);
    expect(scale.end).toBe(200);
    expect(t.closed).toBe(90);
    expect(lanes(t).now).toBeNull();
    expect(lanes(t).lanes.map((l) => l.key)).toEqual([
      "stage",
      "orchestrator",
      "executor",
      "other",
    ]);
  });
  it("keeps the calendar day on a message after closure", () => {
    const t = timeline();
    t.history = { ...history(), messages: [{ ...msg(9, 90000), after_close: true }] };
    expect(rows(t).at(-1)?.day).toBe("01-02");
    expect(t.closed).toBe(90);
  });
  it("draws handovers and uncertain boundaries without invented durations; phone entries stay instants", () => {
    const t = timeline();
    t.history?.roles.push({
      ...(history().roles[0] as RoleSpan),
      by: "next",
      from: 80,
      to: 70,
      uncertain: true,
    });
    const orchestrator = lanes(t).lanes.find((l) => l.key === "orchestrator");
    expect(orchestrator?.segs[0]?.width).toBeCloseTo(200 / 3);
    expect(orchestrator?.segs[1]?.width).toBe(0);
    expect(orchestrator?.segs[1]?.title).toContain("uncertain boundaries");
    const phone = rows(t);
    expect(phone.map((r) => r.start)).toEqual([20, 50, 70, 80, 80]);
    expect(phone.every((r) => r.took === null && r.bar === null)).toBe(true);
    expect(phone.find((r) => r.message === 4)?.label).toBe("one → @executor");
  });
  it("distinguishes unavailable, partial ancestry and known empty records", () => {
    expect(historyWords(undefined)).toContain("unavailable");
    // A whole history says nothing: the timeline does not explain itself.
    expect(historyWords(history())).toBeNull();
    expect(historyWords({ ...history(), sources_complete: false })).toContain("Incomplete");
    expect(
      historyWords({
        ...history(),
        sources: [{ ...(history().sources[0] as HistorySource), state: "partial" }],
      }),
    ).toContain("task: partial");
    expect(
      historyWords({
        ...history(),
        sources: [{ ...(history().sources[0] as HistorySource), state: "unavailable" }],
      }),
    ).toContain("task: unavailable");
  });
});

describe("a selected message's arrow", () => {
  // A layer starting at the name column, as `.tl-address-track` does, and tracks that start past
  // it: the arrow is placed by what is drawn, never by the layer's own percent.
  const layer = { left: 144, top: 300, width: 1048, height: 240 };
  const mark = { left: 268.25, top: 344, width: 24, height: 24 };
  const track = (top: number) => ({ left: 152, top, width: 1040, height: 36 });
  it("runs down through its mark's middle to the middle of its recipient's track", () => {
    expect(arrowPlace(layer, mark, track(386))).toEqual({ left: 136.25, from: 56, to: 104 });
  });
  it("runs up to a recipient above its sender, and stays on one lane's middle for its own", () => {
    expect(arrowPlace(layer, mark, track(302))).toEqual({ left: 136.25, from: 56, to: 20 });
    expect(arrowPlace(layer, mark, track(338))).toEqual({ left: 136.25, from: 56, to: 56 });
  });
  it("follows a mark shrunk about its middle by a press", () => {
    const held = { left: 268.61, top: 344.36, width: 23.28, height: 23.28 };
    const place = arrowPlace(layer, held, track(386));
    expect(place?.left).toBeCloseTo(136.25, 10);
    expect(place?.from).toBeCloseTo(56, 10);
  });
  it("is none while its mark or its recipient's track is not drawn", () => {
    expect(arrowPlace(layer, undefined, track(386))).toBeNull();
    expect(arrowPlace(layer, mark, undefined)).toBeNull();
  });
});

describe("timeline message details", () => {
  it("keeps the chronology accessible while only phone message rows enter its tab order", () => {
    const draw = (wide: boolean) =>
      renderToStaticMarkup(
        <TimelineDrawn
          timeline={timeline()}
          selection={{ messages: [msg()], selected: null, choose: vi.fn() }}
          wide={wide}
          all
          clipped={false}
          detail=""
          stops={stopsOf(0, vi.fn(), vi.fn())}
          toggle={vi.fn()}
        />,
      );
    const phone = draw(false);
    const desktop = draw(true);
    expect(phone).toContain('class="tl-label tl-message-row" tabindex="0" data-message="4"');
    expect(desktop).toContain('class="tl-label tl-message-row" tabindex="-1" data-message="4"');
    expect(desktop).toContain('<ol class="tl-list" aria-label="Steps">');
    expect(desktop).toContain("one → @executor</button>");
  });
  it("selects colliding entries with previous, next and keyboard directions", () => {
    const choose = vi.fn();
    const nodes = (node: ReactNode): ReactElement<Record<string, unknown>>[] => {
      if (Array.isArray(node)) return node.flatMap(nodes);
      if (!isValidElement<Record<string, unknown>>(node)) return [];
      return [node, ...nodes(node.props.children as ReactNode)];
    };
    const tree = nodes(
      MessagePanel({
        timeline: timeline(),
        selection: { messages: [msg(2), msg(4), msg(5)], selected: 4, choose },
        loading: false,
        error: null,
        retry: vi.fn(),
        earlier: vi.fn(),
      }),
    );
    const select = tree.find((n) => n.type === "select");
    const change = select?.props.onChange as (e: { currentTarget: { value: string } }) => void;
    change({ currentTarget: { value: "5" } });
    expect(choose).toHaveBeenLastCalledWith(5);
    const key = select?.props.onKeyDown as (e: { key: string; preventDefault(): void }) => void;
    const event = { key: "ArrowLeft", preventDefault: vi.fn() };
    key(event);
    expect(choose).toHaveBeenLastCalledWith(2);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    key({ ...event, key: "ArrowRight" });
    expect(choose).toHaveBeenLastCalledWith(5);
    const previous = tree.find((n) => n.props.children === "Previous message");
    if (!previous) throw new Error("Missing previous control");
    (previous.props.onClick as () => void)();
    expect(choose).toHaveBeenLastCalledWith(2);
    const next = tree.find((n) => n.props.children === "Next message");
    if (!next) throw new Error("Missing next control");
    (next.props.onClick as () => void)();
    expect(choose).toHaveBeenLastCalledWith(5);
    choose.mockClear();
    key({ ...event, key: "Enter" });
    expect(choose).not.toHaveBeenCalled();
  });
  const panel = (
    message: TimelineMessage,
    access?: ThreadAccess,
    error: string | null = null,
    loading = false,
  ) =>
    renderToStaticMarkup(
      <MessagePanel
        timeline={timeline()}
        selection={{ messages: [message], selected: message.id, choose: vi.fn() }}
        thread={access}
        error={error}
        loading={loading}
        retry={vi.fn()}
        earlier={vi.fn()}
      />,
    );
  it("keeps exact actor, original role address, fractional time, source attribution and original-thread link", () => {
    const m = { ...msg(), at: 50.123, after_close: true };
    const thread: ThreadAccess = {
      entries: [entry(4)],
      before: null,
      pending: false,
      error: null,
      earlier: async () => null,
      latest: async () => null,
      render: (e) => <p>{e.message.text}</p>,
    };
    const html = panel(m, thread);
    for (const text of [
      "one → @executor",
      "50.123",
      "parent/task",
      "after task closure",
      "Original full text",
      'href="#message-4"',
    ])
      expect(html).toContain(text);
    // No note over a whole history, and nothing to step through with one message.
    expect(html).not.toContain("tl-note");
    expect(html).not.toContain("not recorded");
    expect(html).not.toContain("Previous message");
    expect(html).not.toMatch(/<button[^>]*disabled/);
    expect(html).not.toContain("Load earlier messages");
  });
  it("draws its picker only where it can act: none over no message, steps over two", () => {
    const drawn = (messages: TimelineMessage[]) =>
      renderToStaticMarkup(
        <MessagePanel
          timeline={timeline()}
          selection={{ messages, selected: messages[0]?.id ?? null, choose: vi.fn() }}
          error={null}
          loading={false}
          retry={vi.fn()}
          earlier={vi.fn()}
        />,
      );
    const none = drawn([]);
    expect(none).toContain("No recorded messages in the available range.");
    expect(none).not.toContain("tl-message-choice");
    expect(none).not.toContain("<button");
    const two = drawn([msg(), msg(5, 60)]);
    expect(two).toContain('id="tl-message-choice"');
    expect(two).toMatch(/disabled="">Previous message<\/button>/);
    expect(two).toMatch(/data-size="default">Next message<\/button>/);
  });
  it("keeps loading, failed and missing text explicit, without inventing empty complete history", () => {
    expect(panel(msg(), undefined, null, true)).toContain("Loading message");
    expect(panel(msg(), undefined, "Thread unavailable")).toContain("Thread unavailable");
    expect(panel(msg(), undefined, "Thread unavailable")).toContain("Retry message");
    expect(panel({ ...msg(), after_close: null })).toContain(
      "Closure state at send time is unknown",
    );
    expect(panel(msg())).toContain("not in the loaded thread");
  });
});
