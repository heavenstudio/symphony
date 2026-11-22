// The client cache (docs/analysis/client-cache.md): what a page drew, kept on this device in
// IndexedDB, so a return visit draws at once and then asks the server only whether it changed.
// The page kinds reach it as `symphonyCache` (apps/symphony-web/src/cache.ts): no kind's entry
// carries IndexedDB.
//
// An item is what one visit opens (the board, a task, a document) and goes whole; its parts are
// the reads it drew, each kept with the tag the server answered it with. Every call names the
// format its client was built with (symphony_api::CACHE_FORMAT): a store written under another is
// cleared when it opens, and a part written under another is a miss. Nothing here throws into
// the page: without IndexedDB every read misses and nothing is kept.
//
// The shell's script is this file and then shell.ts, each with its types stripped on its own
// (strip.ts), as one module (package.sh): a name this one declared at its top, the shell's could
// not, and it declares none.

/** The database's stores: the items, their parts, and the format they were kept under. */
type StoreName = "items" | "parts" | "meta";

/** The stores one transaction opened, by name. */
type Stores = Record<StoreName, IDBObjectStore>;

/** An item as kept, under its key: its project's, then its own id. */
interface Item {
  key: string;
  project: string;
  kind: string;
  /** When it was last used (ms). */
  used: number;
  /** Its visits, each halving every `HALF_LIFE`, as of `used`. */
  score: number;
  /** When a visited page's first keep counted the visit that opened it, until that visit comes. */
  counted?: number | undefined;
  visited: boolean;
  pinned: boolean;
  /** The sum of its parts' sizes. */
  size: number;
}

/** A part of an item as kept, under its item's key and its own name. */
interface Part {
  key: string;
  format: number;
  etag: string;
  value: unknown;
  size: number;
  saved: number;
}

/** A part as `read` gives it. */
export interface Kept {
  value: unknown;
  etag: string;
  saved: number;
}

/** A read ahead of an item's part: its name and the address it is read from. */
export type Ahead = [part: string, url: string];

/** The store the page kinds reach as `symphonyCache` (apps/symphony-web/src/cache.ts). */
export interface Store {
  read(format: number, project: string, id: string, part: string): Promise<Kept | null>;
  keep(
    format: number,
    project: string,
    id: string,
    kind: string,
    part: string,
    etag: string,
    value: unknown,
    size: number | undefined,
    ahead: boolean,
  ): Promise<boolean | null>;
  ahead(
    fetch: typeof window.fetch,
    project: string,
    id: string,
    kind: string,
    reads: Ahead[],
  ): Promise<void>;
  visit(
    format: number,
    project: string,
    id: string,
    kind: string,
    pinned: boolean,
  ): Promise<boolean | null>;
  forget(format: number, project: string, id: string): Promise<boolean | null>;
}

declare const window: Window &
  typeof globalThis & { symphonyCache: Store; symphonyCacheOpenMs?: unknown };

window.symphonyCache = ((): Store => {
  const MB = 1e6,
    DAY = 864e5;
  // Each project's caps by kind of item, [items, bytes, whether visits weigh]: 8 MB a project
  // together. Its tasks are the ones opened last; its documents are weighed by their visits. The
  // board's reads are outside them (docs/analysis/client-cache.md, "Caps"), held by the origin's
  // bytes alone. Then the age past which an item goes.
  const CAPS: Record<string, [count: number, bytes: number, weighed: boolean]> = {
    task: [30, 2 * MB, false],
    document: [20, 6 * MB, true],
  };
  const ORIGIN = 32 * MB,
    AGE = 14 * DAY;
  // A visit counts one, halving every three days: the documents opened often and lately stay
  // longest.
  const HALF_LIFE = 3 * DAY;
  // How long a page waits for the database to open before it goes on without the cache, unless
  // the page names another (`symphonyCacheOpenMs`, a positive number of ms a browser test's
  // script sets, so a page tested with its cache has it however long a loaded machine takes to
  // open it), and within how long of a page's first keep its visit comes.
  const OPENING = 500,
    VISIT = 6e4;
  let opened: Promise<IDBDatabase | null> | undefined, named: number | undefined;
  let checked = new Map<number, Promise<IDBDatabase | null>>();
  // The transactions open now, and whether the page is away: left (`pagehide`, into its
  // back/forward cache or for good) or frozen (Chrome's `freeze`), and not shown or resumed
  // since. A page away runs none of its callbacks, so a transaction it left between two of them
  // would hold its locks on the stores for as long as it is kept, and Chrome evicts a kept page
  // as soon as another page of the origin asks for a lock it holds: Back would load it again
  // (the next page's first call asks for every store). So a page going away aborts every
  // transaction it has open and starts none until it is back, as it closes its feed's stream
  // (apps/symphony-web/src/feed.ts). What an aborted keep would have kept the next keep of it keeps; a page
  // back checks the format again, as a check cut short by its going, or asked while it was away,
  // gave it no store.
  const running = new Set<IDBTransaction>();
  let away = false;
  const turns: [EventTarget, string, string][] = [
    [window, "pagehide", "pageshow"],
    [document, "freeze", "resume"],
  ];
  for (const [target, leaving, returning] of turns) {
    target.addEventListener(leaving, () => {
      away = true;
      for (const transaction of running) {
        try {
          transaction.abort();
        } catch (_) {
          /* Committing: it needs the page no more. */
        }
      }
      running.clear();
    });
    target.addEventListener(returning, () => {
      if (!away) return;
      away = false;
      checked = new Map();
    });
  }

  // Each name with its length first, so no item's key begins another's: the parts of an item are
  // the keys that begin with its key, and a part's name begins with a digit.
  const key = (...names: string[]) => names.map((name) => `${name.length}:${name}`).join("");
  const decayed = (item: Item, now: number) =>
    item.score * 2 ** (-Math.max(0, now - item.used) / HALF_LIFE);

  const opening = () => {
    const named = window.symphonyCacheOpenMs;
    return typeof named === "number" && named > 0 ? named : OPENING;
  };

  function open(): Promise<IDBDatabase | null> {
    opened ??= new Promise<IDBDatabase | null>((resolve) => {
      let late = false;
      // biome-ignore lint/style/noRestrictedGlobals: no-timed-waits baseline: the store's deadline on its open, OPENING, in the shell's own script, which cannot take src/timing.ts's deadline
      const deadline = setTimeout(() => {
        late = true;
        resolve(null);
      }, opening());
      const request = indexedDB.open("symphony-cache", 1);
      request.onupgradeneeded = () => {
        for (const name of ["items", "parts"])
          request.result.createObjectStore(name, { keyPath: "key" });
        request.result.createObjectStore("meta");
      };
      request.onsuccess = () => {
        clearTimeout(deadline);
        const db = request.result;
        // A newer client's upgrade in another tab closes this one; the next call opens again.
        db.onversionchange = () => {
          db.close();
          opened = undefined;
          checked = new Map();
        };
        if (!late) return resolve(db);
        // Past the deadline the calls so far went on without the cache. The pages that
        // follow without a reload have it: the next call checks its format and uses it.
        // Nothing opens again meanwhile, so a database that never answers is asked once.
        opened = Promise.resolve(db);
        checked = new Map();
      };
      request.onerror = () => {
        clearTimeout(deadline);
        resolve(null);
      };
    }).catch(() => null);
    return opened;
  }

  // The store, once a store of another format is cleared. The format the page's own code named
  // last is the one a read ahead keeps under (`ahead`).
  function ready(format: number): Promise<IDBDatabase | null> {
    named = format;
    let store = checked.get(format);
    if (!store) {
      store = open().then(
        (db) =>
          db &&
          run<IDBDatabase>(db, ["meta", "items", "parts"], (stores, give) => {
            const stored = stores.meta.get("format");
            stored.onsuccess = () => {
              if (stored.result !== format) {
                stores.items.clear();
                stores.parts.clear();
                stores.meta.put(format, "format");
              }
              give(db);
            };
          }),
      );
      checked.set(format, store);
    }
    return store;
  }

  // `work` in one transaction over `names`: what it gives once that commits, or null when it
  // fails (a full disk aborts it, and so does the page going away) or the page is away.
  function run<T>(
    db: IDBDatabase,
    names: StoreName[],
    work: (stores: Stores, give: (value: T) => void) => void,
    mode: IDBTransactionMode = "readwrite",
  ): Promise<T | null> {
    if (away) return Promise.resolve(null);
    return new Promise<T | null>((resolve) => {
      let given: T | null = null;
      const transaction = db.transaction(names, mode);
      running.add(transaction);
      transaction.oncomplete = () => {
        running.delete(transaction);
        resolve(given);
      };
      transaction.onabort = () => {
        running.delete(transaction);
        resolve(null);
      };
      work(
        Object.fromEntries(names.map((name) => [name, transaction.objectStore(name)])) as Stores,
        (value) => {
          given = value;
        },
      );
    }).catch(() => null);
  }

  // `ready` and `run` never reject: each failure gives null.
  async function within<T>(
    format: number,
    names: StoreName[],
    work: (stores: Stores, give: (value: T) => void) => void,
    mode?: IDBTransactionMode,
  ): Promise<T | null> {
    const db = await ready(format);
    return db ? run(db, names, work, mode) : null;
  }

  function drop(stores: Stores, id: string) {
    stores.items.delete(id);
    // Every part of the item: the keys from `id` and a digit up to `id` and the code unit
    // after the digits.
    stores.parts.delete(IDBKeyRange.bound(`${id}0`, `${id}:`, false, true));
  }

  // The items to drop: any unused for AGE or larger alone than its caps, then, while a cap is
  // passed, the lowest ranked first. A page read ahead ranks below every visited one and an
  // entrypoint above them all. Among the rest a project keeps the tasks opened last and the
  // documents visited most (the least and least lately visited goes first), and the origin the
  // items used last. `items` come in key order and the sort is stable, so items ranked alike go
  // by key.
  function plan(items: Item[], now: number): Set<string> {
    const gone = new Set(
      items
        .filter((item) => now - item.used > AGE || item.size > (CAPS[item.kind]?.[1] ?? ORIGIN))
        .map((item) => item.key),
    );
    const ranked = (rank: (item: Item) => number) =>
      items
        .filter((item) => !gone.has(item.key))
        .map((item): [Item, number] => [item, rank(item)])
        .sort(
          ([a, x], [b, y]) =>
            +a.pinned - +b.pinned || +a.visited - +b.visited || x - y || a.used - b.used,
        )
        .map(([item]) => item);
    const recent = ranked(() => 0),
      visited = ranked((item) => decayed(item, now));
    const trim = (order: Item[], count: number, bytes: number, among: (item: Item) => boolean) => {
      const left = order.filter((item) => !gone.has(item.key) && among(item));
      let size = left.reduce((sum, item) => sum + item.size, 0);
      let n = left.length;
      for (const item of left) {
        if (n <= count && size <= bytes) break;
        gone.add(item.key);
        size -= item.size;
        n -= 1;
      }
    };
    for (const project of new Set(recent.map((item) => item.project))) {
      for (const [kind, [count, bytes, weighed]] of Object.entries(CAPS)) {
        trim(
          weighed ? visited : recent,
          count,
          bytes,
          (item) => item.project === project && item.kind === kind,
        );
      }
    }
    trim(recent, Infinity, ORIGIN, () => true);
    return gone;
  }

  // Drop what `plan` names, then give whether `id` is still kept. An item last used by a clock
  // ahead of this one is used now: it decays and ages from here.
  function evict(stores: Stores, now: number, id: string, give: (kept: boolean) => void) {
    const all = stores.items.getAll();
    all.onsuccess = () => {
      const items: Item[] = all.result;
      for (const item of items.filter((item) => item.used > now)) {
        item.used = now;
        stores.items.put(item);
      }
      const gone = plan(items, now);
      for (const dropped of gone) drop(stores, dropped);
      give(!gone.has(id));
    };
  }

  // A new item: one read ahead (`ahead`), unvisited, or a visited page's. A visited page's first
  // keep and its visit come in either order, so its item counts the visit that opened the page
  // from the first of them (`counted`), and ranks the same whichever that is.
  const fresh = (id: string, project: string, kind: string, now: number, ahead: boolean): Item => ({
    key: id,
    project,
    kind,
    used: now,
    score: ahead ? 0 : 1,
    counted: ahead ? undefined : now,
    visited: !ahead,
    pinned: false,
    size: 0,
  });

  // Keep `value`, answered with `etag`, as `part` of the item. `size` is its JSON's length,
  // measured here when not given. Gives whether the item is still kept once the caps hold. A read
  // ahead (`ahead`) never replaces what a visit kept: the page was opened meanwhile.
  function keep(
    format: number,
    project: string,
    id: string,
    kind: string,
    part: string,
    etag: string,
    value: unknown,
    size: number | undefined,
    ahead: boolean,
  ): Promise<boolean | null> {
    const now = Date.now();
    return within<boolean>(format, ["items", "parts"], (stores, give) => {
      // Inside the transaction's guard: a value with no JSON keeps nothing.
      const measured = size ?? JSON.stringify(value).length;
      const itemKey = key(project, id),
        partKey = itemKey + key(part);
      const item = stores.items.get(itemKey);
      item.onsuccess = () => {
        const found: Item | undefined = item.result;
        if (ahead && found?.visited) return give(true);
        const kept = found ?? fresh(itemKey, project, kind, now, ahead);
        const stored = stores.parts.get(partKey);
        stored.onsuccess = () => {
          const before: Part | undefined = stored.result;
          kept.size += measured - (before?.size ?? 0);
          const written: Part = { key: partKey, format, etag, value, size: measured, saved: now };
          stores.parts.put(written);
          stores.items.put(kept);
          evict(stores, now, itemKey, give);
        };
      };
    });
  }

  return {
    // `part` of `item` as kept: { value, etag, saved }, or null. A part saved by a clock ahead
    // of this one was saved now, as far as the page says.
    read: (format, project, id, part) =>
      within<Kept>(
        format,
        ["parts"],
        (stores, give) => {
          const stored = stores.parts.get(key(project, id, part));
          stored.onsuccess = () => {
            const kept: Part | undefined = stored.result;
            if (kept?.format === format)
              give({ value: kept.value, etag: kept.etag, saved: Math.min(kept.saved, Date.now()) });
          };
        },
        "readonly",
      ),
    keep,
    // Read the item ahead of its visit (shell.ts, `ahead`): each of `reads`, [part, url], with
    // `fetch` at the lowest priority, marked as a prefetch and naming the tag kept for its part,
    // so an unchanged one costs no body; what the server sends is kept unvisited. Nothing for
    // an item a visit kept, nor before the page's own code named its format. Every answer's
    // body is read to its end, a refusal's too: the browser ends a read only once its body is
    // read, and one left unread stays in flight for as long as the page is open.
    ahead: async (fetch, project, id, kind, reads) => {
      const format = named;
      if (format === undefined) return;
      const tags = await within<Record<string, string>>(
        format,
        ["items", "parts"],
        (stores, give) => {
          const itemKey = key(project, id),
            tags: Record<string, string> = {};
          const item = stores.items.get(itemKey);
          item.onsuccess = () => {
            const found: Item | undefined = item.result;
            if (found?.visited) return;
            let left = reads.length;
            for (const [part] of reads) {
              const stored = stores.parts.get(itemKey + key(part));
              stored.onsuccess = () => {
                const kept: Part | undefined = stored.result;
                tags[part] = kept?.format === format ? kept.etag : "";
                if (--left === 0) give(tags);
              };
            }
          };
        },
        "readonly",
      );
      if (!tags) return;
      for (const [part, url] of reads) {
        const headers: Record<string, string> = { Purpose: "prefetch" };
        const tag = tags[part];
        if (tag) headers["If-None-Match"] = tag;
        try {
          const response = await fetch(url, { priority: "low", cache: "no-store", headers });
          if (response.status === 200)
            await keep(
              format,
              project,
              id,
              kind,
              part,
              response.headers.get("ETag") || "",
              await response.json(),
              undefined,
              true,
            );
          else await response.arrayBuffer();
        } catch (_) {
          /* Read when opened, then. */
        }
      }
    },
    // The page was opened: one more visit, on the count decayed since the last. `pinned`
    // marks a project's entrypoint page, kept while its project has room.
    visit: (format, project, id, kind, pinned) => {
      const now = Date.now();
      return within<boolean>(format, ["items", "parts"], (stores, give) => {
        const itemKey = key(project, id);
        const item = stores.items.get(itemKey);
        item.onsuccess = () => {
          const kept: Item = item.result ?? fresh(itemKey, project, kind, now, false);
          // The visit a first keep counted a moment ago adds nothing more.
          const counted = kept.counted !== undefined && now - kept.counted < VISIT;
          delete kept.counted;
          Object.assign(kept, {
            score: decayed(kept, now) + (counted ? 0 : 1),
            used: now,
            visited: true,
            pinned: !!pinned,
          });
          stores.items.put(kept);
          evict(stores, now, itemKey, give);
        };
      });
    },
    // Drop the item and every part of it: one the client could not read, say.
    forget: (format, project, id) =>
      within<boolean>(format, ["items", "parts"], (stores, give) => {
        drop(stores, key(project, id));
        give(true);
      }),
  };
})();
