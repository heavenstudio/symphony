import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  type Attach,
  AttachControls,
  type Attachments,
  compose,
  MAX_FILES,
  restored,
  routed,
  saved,
  sendAfter,
  storageKey,
  UNSENT,
} from "./attach";
import { isUpload, take, tooLarge, upload } from "./attach-upload";
import { reply, server, until } from "./testing";
import { UPLOAD_ACCEPT, UPLOAD_MAX_BYTES, type Upload } from "./wire";

afterEach(() => {
  vi.unstubAllGlobals();
});

/** A stored file as the server answers it: a picture, or a document. */
const stored = (name: string, picture: boolean): Upload => {
  const url = `/artifacts/uploads/${"a".repeat(64)}-${name}.${picture ? "png" : "pdf"}`;
  return { url, name, picture, bytes: 12, markup: `${picture ? "!" : ""}[${name}](${url})` };
};

/** A picked file: its name and size are all the composer reads before sending it. */
const picked = (name: string, size = 12) => ({ name, size }) as File;

describe("a post's files", () => {
  const shot = stored("shot.png", true);
  const brief = stored("brief.pdf", false);

  it("are one line each after its words; with no file the text is as typed", () => {
    expect(compose("  typed \n", [])).toBe("  typed \n");
    expect(compose("Look at this\n\n", [shot, brief])).toBe(
      `Look at this\n\n${shot.markup}\n${brief.markup}`,
    );
    expect(compose(" \n ", [shot])).toBe(shot.markup);
  });

  it("read back from the draft this device keeps, a broken row left out", () => {
    const files = [stored("shot one.png", true), brief];
    expect(restored(saved(files))).toEqual(files);
    expect(restored("")).toEqual([]);
    expect(restored("broken\trow")).toEqual([]);
    expect(restored(`${saved([shot])}\r\nurl\tname\tfalse\t-1\tmarkup`)).toEqual([shot]);
    expect(restored("url\tname\ttrue\t+7\t[a](b)\tmore")).toEqual([
      { url: "url", name: "name", picture: true, bytes: 7, markup: "[a](b)" },
    ]);
    expect(restored(`url\tname\ttrue\t${"9".repeat(20)}\tmarkup`)).toEqual([]);
    expect(storageKey("review-draft:docs/a.md:intro")).toBe(
      "review-attach:review-draft:docs/a.md:intro",
    );
  });

  it("past the server's size are named before a byte is sent", () => {
    expect(tooLarge("a.png", UPLOAD_MAX_BYTES)).toBeNull();
    expect(tooLarge("big.png", UPLOAD_MAX_BYTES * 1.5)).toBe(
      "big.png is 15.0 MB; uploads are at most 10 MB",
    );
  });

  it("are uploads only as the server answers them", () => {
    expect(isUpload(shot)).toBe(true);
    for (const value of [null, "shot", 3, { ...shot, picture: "yes" }, { ...shot, bytes: "12" }]) {
      expect(isUpload(value)).toBe(false);
    }
  });
});

describe("an upload", () => {
  const file = new File(["png"], "a b.png", { type: "image/png" });

  it("sends the raw file under its name and takes the stored line", async () => {
    const asked = server();
    const fetched = vi.fn(globalThis.fetch);
    vi.stubGlobal("fetch", fetched);
    const sent = upload(file);
    await until(() => expect(asked).toHaveLength(1));
    expect(asked[0]?.url).toBe("/api/upload?name=a%20b.png");
    expect(fetched.mock.calls[0]?.[1]).toMatchObject({
      method: "POST",
      body: file,
      headers: { "Content-Type": "image/png" },
    });
    const answer = stored("a b.png", true);
    asked[0]?.answer(reply(200, answer));
    await expect(sent).resolves.toEqual(answer);
  });

  it("of a file with no type is sent as bytes", async () => {
    server();
    const fetched = vi.fn(globalThis.fetch);
    vi.stubGlobal("fetch", fetched);
    void upload(new File(["?"], "notes"));
    await until(() => expect(fetched).toHaveBeenCalled());
    expect(fetched.mock.calls[0]?.[1]?.headers).toEqual({
      "Content-Type": "application/octet-stream",
    });
  });

  it("says the server's refusal, its status, or an answer that is no upload", async () => {
    for (const [answer, said] of [
      [reply(413, { error: "the file is too large" }), "the file is too large"],
      [reply(500, { other: true }), "the server answered 500"],
      [new Response("<html>", { status: 502 }), "the server answered 502"],
      [reply(200, { url: "/x" }), "the server's answer is no upload"],
    ] as const) {
      const asked = server();
      const sent = upload(file);
      await until(() => expect(asked).toHaveLength(1));
      asked[0]?.answer(answer);
      await expect(sent).rejects.toThrow(said);
    }
  });

  it("with no answer in its time is given up, and said so", async () => {
    vi.useFakeTimers();
    try {
      vi.stubGlobal("document", { getElementById: () => null });
      vi.stubGlobal(
        "fetch",
        (_url: string, init: RequestInit) =>
          new Promise((_, refuse) =>
            init.signal?.addEventListener("abort", () => refuse(new Error("aborted"))),
          ),
      );
      const sent = upload(file);
      const said = expect(sent).rejects.toThrow("no answer from the server; try again");
      await vi.advanceTimersByTimeAsync(120_000);
      await said;
    } finally {
      vi.useRealTimers();
    }
  });

  it("whose request fails says why", async () => {
    vi.stubGlobal("document", { getElementById: () => null });
    vi.stubGlobal("fetch", () => Promise.reject(new TypeError("Failed to fetch")));
    await expect(upload(file)).rejects.toThrow("Failed to fetch");
  });
});

describe("a batch of files", () => {
  const held = (files: Upload[] = []): Attachments => ({ files, pending: [], trouble: "earlier" });

  it("is sent one at a time, each shown on its way and kept once stored", async () => {
    const attachments = held();
    const seen: string[][] = [];
    const kept = vi.fn();
    const send = vi.fn(async (file: File) => stored(file.name, true));
    await take(
      [picked("one.png"), picked("two.png")],
      attachments,
      () => seen.push([...attachments.pending]),
      kept,
      () => true,
      send,
    );
    expect(send).toHaveBeenCalledTimes(2);
    expect(seen).toEqual([["one.png"], [], ["two.png"], []]);
    expect(attachments.files.map((file) => file.name)).toEqual(["one.png", "two.png"]);
    expect(attachments.trouble).toBeNull();
    expect(kept).toHaveBeenCalledTimes(2);
  });

  it("holds a file the server already holds once", async () => {
    const attachments = held([stored("one.png", true)]);
    await take(
      [picked("one.png")],
      attachments,
      () => {},
      () => {},
      () => true,
      async (file) => stored(file.name, true),
    );
    expect(attachments.files).toHaveLength(1);
  });

  it("names a file past the size and the server's refusal, and goes on", async () => {
    const attachments = held();
    const send = vi.fn(async (file: File) => {
      if (file.name === "bad.svg") throw new Error("not a picture");
      return stored(file.name, true);
    });
    await take(
      [picked("big.png", UPLOAD_MAX_BYTES + 1), picked("bad.svg"), picked("fine.png")],
      attachments,
      () => {},
      () => {},
      () => true,
      send,
    );
    expect(send).toHaveBeenCalledTimes(2);
    expect(attachments.files.map((file) => file.name)).toEqual(["fine.png"]);
    expect(attachments.trouble).toBeNull();
    await take(
      [picked("bad.svg")],
      attachments,
      () => {},
      () => {},
      () => true,
      send,
    );
    expect(attachments.trouble).toBe("bad.svg: not a picture");
    await take(
      [picked("big.png", UPLOAD_MAX_BYTES * 2)],
      attachments,
      () => {},
      () => {},
      () => true,
      send,
    );
    expect(attachments.trouble).toBe("big.png is 20.0 MB; uploads are at most 10 MB");
  });

  it("past the most files a post carries is refused, the rest not sent", async () => {
    const full = Array.from({ length: MAX_FILES - 1 }, (_, at) => stored(`${at}.png`, true));
    const attachments = held(full);
    const send = vi.fn(async (file: File) => stored(file.name, true));
    await take(
      [picked("last.png"), picked("over.png"), picked("again.png")],
      attachments,
      () => {},
      () => {},
      () => true,
      send,
    );
    expect(send).toHaveBeenCalledTimes(1);
    expect(attachments.files).toHaveLength(MAX_FILES);
    expect(attachments.trouble).toBe(`At most ${MAX_FILES} files per post`);
  });

  it("answered after its composer left changes nothing", async () => {
    const attachments = held();
    const changed = vi.fn();
    let alive = true;
    await take(
      [picked("one.png"), picked("two.png")],
      attachments,
      changed,
      () => {},
      () => alive,
      async (file) => {
        alive = false;
        return stored(file.name, true);
      },
    );
    expect(changed).toHaveBeenCalledTimes(1);
    expect(attachments.files).toEqual([]);
    expect(attachments.pending).toEqual(["one.png"]);
  });
});

describe("a composer's files before their sending has come", () => {
  const held = (): Attachments => ({ files: [], pending: [], trouble: null });

  /** The sending, come as `come` says: a take it records, or none, which fails. */
  function sending(come: boolean) {
    const took = vi.fn(async () => {});
    const load = vi.fn(async () => {
      if (!come) throw new Error("no such chunk");
      return { take: took };
    });
    return { took, load };
  }

  /** The page's shell, which says its build is `gone`. */
  function aShell(gone: boolean) {
    const asked = vi.fn(async () => gone);
    vi.stubGlobal("window", { reviewShell: { gone: asked } });
    return asked;
  }

  it("are taken once it has come, after the batches before them", async () => {
    const { took, load } = sending(true);
    const attachments = held();
    const [changed, kept, alive] = [vi.fn(), vi.fn(), () => true];
    let before: () => void = () => {};
    const earlier = new Promise<void>((done) => {
      before = done;
    });
    const files = [picked("one.png")];
    const done = sendAfter(earlier, files, attachments, changed, kept, alive, load);
    await Promise.resolve();
    expect(load).not.toHaveBeenCalled();
    before();
    await done;
    expect(took).toHaveBeenCalledExactlyOnceWith(files, attachments, changed, kept, alive);
  });

  it("say they were not sent, once the shell says the build is there", async () => {
    const gone = aShell(false);
    const attachments = held();
    const changed = vi.fn();
    await sendAfter(
      Promise.resolve(),
      [picked("one.png")],
      attachments,
      changed,
      () => {},
      () => true,
      sending(false).load,
    );
    expect(gone).toHaveBeenCalledOnce();
    expect(attachments.trouble).toBe(UNSENT);
    expect(changed).toHaveBeenCalledOnce();
    expect([attachments.files, attachments.pending]).toEqual([[], []]);
  });

  it("say nothing where the build is gone, or the composer left", async () => {
    for (const [gone, alive] of [
      [true, true],
      [false, false],
    ]) {
      const asked = aShell(gone);
      const attachments = held();
      const changed = vi.fn();
      await sendAfter(
        Promise.resolve(),
        [picked("one.png")],
        attachments,
        changed,
        () => {},
        () => alive,
        sending(false).load,
      );
      expect([gone, alive, attachments.trouble]).toEqual([gone, alive, null]);
      expect(changed).not.toHaveBeenCalled();
      expect(asked).toHaveBeenCalledTimes(alive ? 1 : 0);
    }
  });

  it("are sent by the uploads' chunk itself by default", async () => {
    vi.stubGlobal("document", { getElementById: () => null });
    const asked = server();
    const attachments = held();
    const done = sendAfter(
      Promise.resolve(),
      [picked("one.png")],
      attachments,
      () => {},
      () => {},
      () => true,
    );
    await until(() => expect(asked).toHaveLength(1));
    expect(asked[0]?.url).toBe("/api/upload?name=one.png");
    asked[0]?.answer(reply(200, stored("one.png", true)));
    await done;
    expect(attachments.files).toEqual([stored("one.png", true)]);
  });
});

/** A composer's form as `routed` reads it: its key, whether its controls are off, its marks. */
function zoneOf(key: string | null, off = false) {
  const marks = new Map<string, string>();
  return {
    marks,
    getAttribute: (name: string) => (name === "data-attach" ? key : null),
    querySelector: () => (off ? {} : null),
    setAttribute: (name: string, value: string) => marks.set(name, value),
    removeAttribute: (name: string) => marks.delete(name),
  };
}

/** An event of the page's on a target inside `zone` (none when it is null). */
function eventOn(type: string, zone: object | null, extra: object = {}, target: object = {}) {
  return {
    type,
    target: Object.assign(target, { closest: () => zone }),
    preventDefault: vi.fn(),
    ...extra,
  } as unknown as Event & { preventDefault: ReturnType<typeof vi.fn> };
}

describe("a paste, a drag, a drop or a pick on the page", () => {
  const file = picked("shot.png");
  const owner = () => {
    const taken: File[][] = [];
    return {
      taken,
      zones: new Map([["draft", { id: 1, take: (files: File[]) => taken.push(files) }]]),
    };
  };

  it("reaches the composer whose form holds its target", () => {
    const { taken, zones } = owner();
    const paste = eventOn("paste", zoneOf("draft"), { clipboardData: { files: [file] } });
    routed(paste, zones);
    expect(taken).toEqual([[file]]);
    expect(paste.preventDefault).toHaveBeenCalled();
    const zone = zoneOf("draft");
    zone.marks.set("data-dropping", "true");
    const drop = eventOn("drop", zone, { dataTransfer: { files: [file] } });
    routed(drop, zones);
    expect(taken).toHaveLength(2);
    expect(zone.marks.has("data-dropping")).toBe(false);
  });

  it("marks the form while files are dragged over it, and unmarks it as they leave", () => {
    const { taken, zones } = owner();
    const zone = zoneOf("draft");
    const over = eventOn("dragover", zone, { dataTransfer: { types: ["Files"] } });
    routed(over, zones);
    expect(over.preventDefault).toHaveBeenCalled();
    expect(zone.marks.get("data-dropping")).toBe("true");
    const text = eventOn("dragover", zoneOf("draft"), { dataTransfer: { types: ["text/plain"] } });
    routed(text, zones);
    expect(text.preventDefault).not.toHaveBeenCalled();
    routed(eventOn("dragleave", zone), zones);
    expect(zone.marks.has("data-dropping")).toBe(false);
    expect(taken).toEqual([]);
  });

  it("takes the picker's files and clears it, so the same file picked again fires again", () => {
    const { taken, zones } = owner();
    const input = { hasAttribute: () => true, files: [file], value: "C:\\fakepath\\shot.png" };
    routed(eventOn("change", zoneOf("draft"), {}, input), zones);
    expect(taken).toEqual([[file]]);
    expect(input.value).toBe("");
    const other = { hasAttribute: () => false, files: [file], value: "kept" };
    routed(eventOn("change", zoneOf("draft"), {}, other), zones);
    expect(other.value).toBe("kept");
    expect(taken).toHaveLength(1);
  });

  it("is left to the page outside a composer, in one sending, or with no file", () => {
    const { taken, zones } = owner();
    const cases = [
      eventOn("paste", null, { clipboardData: { files: [file] } }),
      eventOn("paste", zoneOf(null), { clipboardData: { files: [file] } }),
      eventOn("paste", zoneOf("draft", true), { clipboardData: { files: [file] } }),
      eventOn("paste", zoneOf("gone"), { clipboardData: { files: [file] } }),
      eventOn("paste", zoneOf("draft"), { clipboardData: { files: [] } }),
      eventOn("paste", zoneOf("draft"), {}),
      eventOn("keydown", zoneOf("draft")),
    ];
    for (const event of cases) {
      routed(event, zones);
      expect(event.preventDefault).not.toHaveBeenCalled();
    }
    routed({ type: "paste", target: null } as unknown as Event, zones);
    routed({ type: "paste", target: {} } as unknown as Event, zones);
    expect(taken).toEqual([]);
  });
});

/** A composer's attachments as `useAttach` gives them, its files and refusal set. */
function attachOf(files: Upload[], pending: string[] = [], trouble: string | null = null): Attach {
  return {
    zone: "fixture",
    files,
    pending,
    trouble,
    busy: pending.length > 0,
    empty: files.length === 0,
    compose: (text) => compose(text, files),
    clear: () => {},
    remove: () => {},
  };
}

describe("the attach controls", () => {
  it("offer the picker and show each file, each upload and the last refusal", () => {
    const html = renderToStaticMarkup(
      <form data-attach="fixture">
        <AttachControls
          attach={attachOf(
            [stored("shot.png", true), stored("brief.pdf", false)],
            ["next.png"],
            "old.svg is not a picture",
          )}
        />
      </form>,
    );
    expect(html).toContain('data-attach="fixture"');
    expect(html).toContain('type="file"');
    expect(html).toContain("multiple");
    expect(html).toContain(`accept="${UPLOAD_ACCEPT}"`);
    expect(html).toContain('data-attach-input="true"');
    expect(html).toContain('data-attach-off="false"');
    expect(html.match(/class="attach-file"/g)).toHaveLength(2);
    expect(html).toContain(`<img src="/artifacts/uploads/${"a".repeat(64)}-shot.png.png"`);
    expect(html).toContain('<span class="attach-icon" aria-hidden="true">📄</span>');
    expect(html).toContain('aria-label="Remove shot.png"');
    expect(html).toContain('aria-label="Remove brief.pdf"');
    expect(html).toContain('<li class="attach-file pending" role="status">');
    expect(html).toContain("Uploading next.png…");
    expect(html).toContain('<p class="attach-trouble" role="alert">old.svg is not a picture</p>');
  });

  it("show no chips when empty, and no picker when full or sending", () => {
    const empty = renderToStaticMarkup(<AttachControls attach={attachOf([])} />);
    expect(empty).not.toContain("attach-files");
    expect(empty).not.toContain('role="alert"');
    expect(empty).not.toContain("disabled");
    const full = Array.from({ length: MAX_FILES }, (_, at) => stored(`${at}.png`, true));
    expect(renderToStaticMarkup(<AttachControls attach={attachOf(full)} />)).toContain(
      "attach-pick disabled",
    );
    const sending = renderToStaticMarkup(
      <AttachControls attach={attachOf([stored("shot.png", true)])} disabled />,
    );
    expect(sending).toContain('data-attach-off="true"');
    expect(sending).toContain("attach-pick disabled");
    expect(sending.match(/disabled=""/g)).toHaveLength(2);
  });
});
