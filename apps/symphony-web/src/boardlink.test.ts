import { beforeEach, expect, it, vi } from "vitest";
import { linkedSlug, linkedTask } from "./boardlink";
import { getJson } from "./request";
import { aTask } from "./testing";

vi.mock("./request", () => ({ getJson: vi.fn() }));
const read = vi.mocked(getJson);
const signal = new AbortController().signal;
beforeEach(() => read.mockReset());

it("resolves only row fragments and ignores an empty address without a read", async () => {
  expect(linkedSlug("column-ready")).toBe("");
  expect(linkedSlug("row-work/a")).toBe("work/a");
  expect(await linkedTask("", "", signal)).toBeNull();
  expect(read).not.toHaveBeenCalled();
});

it("finds the exact slug after other text matches, retaining scope and encoding cursors", async () => {
  const target = aTask("work/a");
  read.mockResolvedValueOnce({ tasks: [aTask("other")], next: "a/b+" });
  read.mockResolvedValueOnce({ tasks: [target], next: null });
  expect(await linkedTask("branch", target.slug, signal)).toEqual(target);
  expect(read.mock.calls).toEqual([
    ["/@branch/api/task-board?q=work%2Fa&older=1&work=0", signal],
    ["/@branch/api/task-board?q=work%2Fa&older=1&work=0&cursor=a%2Fb%2B", signal],
  ]);
});

it("ends an absent target at the last page and refuses a repeated cursor", async () => {
  read.mockResolvedValueOnce({ tasks: [], next: null });
  expect(await linkedTask("", "missing", signal)).toBeNull();
  read.mockResolvedValue({ tasks: [], next: "same" });
  expect(await linkedTask("", "missing", signal)).toBeNull();
  expect(read).toHaveBeenCalledTimes(3);
});

it("reports malformed summaries and request failures", async () => {
  read.mockResolvedValueOnce({ tasks: [false] });
  await expect(linkedTask("", "x", signal)).rejects.toThrow("summary could not be read");
  read.mockRejectedValueOnce(new Error("offline"));
  await expect(linkedTask("", "x", signal)).rejects.toThrow("offline");
});

it("stops paging when navigation cancels the read", async () => {
  const controller = new AbortController();
  read.mockImplementationOnce(async () => {
    controller.abort();
    return { tasks: [], next: "more" };
  });
  expect(await linkedTask("", "x", controller.signal)).toBeNull();
  expect(read).toHaveBeenCalledTimes(1);
});
