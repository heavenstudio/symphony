// The client cache (docs/analysis/client-cache.md): what a page drew, kept on this device in
// IndexedDB, so a return visit draws at once and then asks the server only whether it changed.
// The page kinds reach it as `symphonyCache` (src/cache.rs): no kind's wasm carries IndexedDB.
//
// An item is what one visit opens (the board, a task, a document) and goes whole; its parts are
// the reads it drew, each kept with the tag the server answered it with. Every call names the
// format its client was built with (symphony_api::CACHE_FORMAT): a store written under another is
// cleared when it opens, and a part written under another is a miss. Nothing here throws into
// the page: without IndexedDB every read misses and nothing is kept.
window.symphonyCache = (() => {
    const MB = 1e6, DAY = 864e5;
    // Each project's caps by kind of item, [items, bytes, whether visits weigh]: 8 MB a project
    // together. Its tasks are the ones opened last; its documents are weighed by their visits. The
    // board's reads are outside them (docs/analysis/client-cache.md, "Caps"), held by the origin's
    // bytes alone. Then the age past which an item goes.
    const CAPS = { task: [30, 2 * MB, false], document: [20, 6 * MB, true] };
    const ORIGIN = 32 * MB, AGE = 14 * DAY;
    // A visit counts one, halving every three days: the documents opened often and lately stay
    // longest.
    const HALF_LIFE = 3 * DAY;
    // How long a page waits for the database to open before it goes on without the cache, and
    // within how long of a page's first keep its visit comes.
    const OPENING = 500, VISIT = 6e4;
    let opened, named;
    let checked = new Map();

    // Each name with its length first, so no item's key begins another's: the parts of an item are
    // the keys that begin with its key, and a part's name begins with a digit.
    const key = (...names) => names.map(name => `${name.length}:${name}`).join("");
    const decayed = (item, now) => item.score * 2 ** (-Math.max(0, now - item.used) / HALF_LIFE);

    function open() {
        return opened ??= new Promise(resolve => {
            let late = false;
            const deadline = setTimeout(() => {
                late = true;
                resolve(null);
            }, OPENING);
            const request = indexedDB.open("symphony-cache", 1);
            request.onupgradeneeded = () => {
                for (const name of ["items", "parts"]) request.result.createObjectStore(name, { keyPath: "key" });
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
    }

    // The store, once a store of another format is cleared. The format the page's own code named
    // last is the one a read ahead keeps under (`ahead`).
    function ready(format) {
        named = format;
        if (!checked.has(format)) {
            checked.set(format, open().then(db => db && run(db, ["meta", "items", "parts"], (stores, give) => {
                stores.meta.get("format").onsuccess = event => {
                    if (event.target.result !== format) {
                        stores.items.clear();
                        stores.parts.clear();
                        stores.meta.put(format, "format");
                    }
                    give(db);
                };
            })));
        }
        return checked.get(format);
    }

    // `work` in one transaction over `names`: what it gives once that commits, or null when it
    // fails (a full disk aborts it).
    function run(db, names, work, mode = "readwrite") {
        return new Promise(resolve => {
            let given = null;
            const transaction = db.transaction(names, mode);
            transaction.oncomplete = () => resolve(given);
            transaction.onabort = () => resolve(null);
            work(Object.fromEntries(names.map(name => [name, transaction.objectStore(name)])), value => { given = value; });
        }).catch(() => null);
    }

    // `ready` and `run` never reject: each failure gives null.
    async function within(format, names, work, mode) {
        const db = await ready(format);
        return db ? run(db, names, work, mode) : null;
    }

    function drop(stores, id) {
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
    function plan(items, now) {
        const gone = new Set(items
            .filter(item => now - item.used > AGE || item.size > (CAPS[item.kind]?.[1] ?? ORIGIN))
            .map(item => item.key));
        const ranked = rank => items
            .filter(item => !gone.has(item.key))
            .map(item => [item, rank(item)])
            .sort(([a, x], [b, y]) => a.pinned - b.pinned || a.visited - b.visited || x - y || a.used - b.used)
            .map(([item]) => item);
        const recent = ranked(() => 0), visited = ranked(item => decayed(item, now));
        const trim = (order, count, bytes, among) => {
            const left = order.filter(item => !gone.has(item.key) && among(item));
            let size = left.reduce((sum, item) => sum + item.size, 0);
            for (let at = 0, n = left.length; at < left.length && (n > count || size > bytes); at += 1, n -= 1) {
                gone.add(left[at].key);
                size -= left[at].size;
            }
        };
        for (const project of new Set(recent.map(item => item.project))) {
            for (const [kind, [count, bytes, weighed]] of Object.entries(CAPS)) {
                trim(weighed ? visited : recent, count, bytes, item => item.project === project && item.kind === kind);
            }
        }
        trim(recent, Infinity, ORIGIN, () => true);
        return gone;
    }

    // Drop what `plan` names, then give whether `id` is still kept. An item last used by a clock
    // ahead of this one is used now: it decays and ages from here.
    function evict(stores, now, id, give) {
        stores.items.getAll().onsuccess = event => {
            const items = event.target.result;
            for (const item of items.filter(item => item.used > now)) {
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
    const fresh = (id, project, kind, now, ahead) => ({
        key: id, project, kind, used: now, score: ahead ? 0 : 1, counted: ahead ? undefined : now,
        visited: !ahead, pinned: false, size: 0,
    });

    // Keep `value`, answered with `etag`, as `part` of the item. `size` is its JSON's length,
    // measured here when not given. Gives whether the item is still kept once the caps hold. A read
    // ahead (`ahead`) never replaces what a visit kept: the page was opened meanwhile.
    function keep(format, project, id, kind, part, etag, value, size, ahead) {
        const now = Date.now();
        return within(format, ["items", "parts"], (stores, give) => {
            // Inside the transaction's guard: a value with no JSON keeps nothing.
            size ??= JSON.stringify(value).length;
            const itemKey = key(project, id), partKey = itemKey + key(part);
            stores.items.get(itemKey).onsuccess = event => {
                if (ahead && event.target.result?.visited) return give(true);
                const kept = event.target.result ?? fresh(itemKey, project, kind, now, ahead);
                stores.parts.get(partKey).onsuccess = event => {
                    kept.size += size - (event.target.result?.size ?? 0);
                    stores.parts.put({ key: partKey, format, etag, value, size, saved: now });
                    stores.items.put(kept);
                    evict(stores, now, itemKey, give);
                };
            };
        });
    }

    return {
        // `part` of `item` as kept: { value, etag, saved }, or null. A part saved by a clock ahead
        // of this one was saved now, as far as the page says.
        read: (format, project, id, part) => within(format, ["parts"], (stores, give) => {
            stores.parts.get(key(project, id, part)).onsuccess = event => {
                const kept = event.target.result;
                if (kept?.format === format) give({ value: kept.value, etag: kept.etag, saved: Math.min(kept.saved, Date.now()) });
            };
        }, "readonly"),
        keep,
        // Read the item ahead of its visit (shell.js, `ahead`): each of `reads`, [part, url], with
        // `fetch` at the lowest priority, marked as a prefetch and naming the tag kept for its part,
        // so an unchanged one costs no body; what the server sends is kept unvisited. Nothing for
        // an item a visit kept, nor before the page's own code named its format.
        ahead: async (fetch, project, id, kind, reads) => {
            const format = named;
            const tags = format === undefined ? null : await within(format, ["items", "parts"], (stores, give) => {
                const itemKey = key(project, id), tags = {};
                stores.items.get(itemKey).onsuccess = event => {
                    if (event.target.result?.visited) return;
                    let left = reads.length;
                    for (const [part] of reads) {
                        stores.parts.get(itemKey + key(part)).onsuccess = event => {
                            const kept = event.target.result;
                            tags[part] = kept?.format === format ? kept.etag : "";
                            if (--left === 0) give(tags);
                        };
                    }
                };
            }, "readonly");
            if (!tags) return;
            for (const [part, url] of reads) {
                const headers = { Purpose: "prefetch" };
                if (tags[part]) headers["If-None-Match"] = tags[part];
                try {
                    const response = await fetch(url, { priority: "low", cache: "no-store", headers });
                    if (response.status === 200) await keep(format, project, id, kind, part, response.headers.get("ETag") || "", await response.json(), undefined, true);
                } catch (_) { /* Read when opened, then. */ }
            }
        },
        // The page was opened: one more visit, on the count decayed since the last. `pinned`
        // marks a project's entrypoint page, kept while its project has room.
        visit: (format, project, id, kind, pinned) => {
            const now = Date.now();
            return within(format, ["items", "parts"], (stores, give) => {
                const itemKey = key(project, id);
                stores.items.get(itemKey).onsuccess = event => {
                    const kept = event.target.result ?? fresh(itemKey, project, kind, now, false);
                    // The visit a first keep counted a moment ago adds nothing more.
                    const counted = now - kept.counted < VISIT;
                    delete kept.counted;
                    Object.assign(kept, { score: decayed(kept, now) + (counted ? 0 : 1), used: now, visited: true, pinned: !!pinned });
                    stores.items.put(kept);
                    evict(stores, now, itemKey, give);
                };
            });
        },
        // Drop the item and every part of it: one the client could not read, say.
        forget: (format, project, id) => within(format, ["items", "parts"], (stores, give) => {
            drop(stores, key(project, id));
            give(true);
        }),
    };
})();
