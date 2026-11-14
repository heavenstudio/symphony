import { afterEach, describe, expect, it, vi } from "vitest";
import { load, loadForTab, save, saveForTab } from "./storage";
import { memoryStorage } from "./testing";

afterEach(() => vi.unstubAllGlobals());

describe("what this device keeps", () => {
  it("is kept and read back, for good or for the tab", () => {
    vi.stubGlobal("localStorage", memoryStorage());
    vi.stubGlobal("sessionStorage", memoryStorage());
    expect(load("k")).toBeNull();
    save("k", "v");
    expect(load("k")).toBe("v");
    expect(loadForTab("k")).toBeNull();
    expect(saveForTab("k", "t")).toBe(true);
    expect(loadForTab("k")).toBe("t");
  });

  it("is nothing where the browser refuses its storage, and the page goes on", () => {
    vi.stubGlobal("localStorage", memoryStorage(true));
    vi.stubGlobal("sessionStorage", memoryStorage(true));
    save("k", "v");
    expect(load("k")).toBeNull();
    expect(saveForTab("k", "t")).toBe(false);
    expect(loadForTab("k")).toBeNull();
  });

  it("is under the project's id on the machine's one server", async () => {
    vi.resetModules();
    const local = memoryStorage();
    vi.stubGlobal("localStorage", local);
    vi.stubGlobal("document", {
      getElementById: () => ({ textContent: '{"name":"","register":"","base":"/alpha"}' }),
    });
    const storage = await import("./storage");
    storage.save("k", "v");
    expect(local.getItem("alpha:k")).toBe("v");
  });
});
