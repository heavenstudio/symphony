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
    // Each project's caps by kind of item, [items, bytes]; the board's reads have none of their
    // own. Then each project's and the origin's bytes, and the age past which an item goes.
    const CAPS = { task: [30, 2 * MB], document: [20, 6 * MB] };
    const PROJECT = 8 * MB, ORIGIN = 32 * MB, AGE = 14 * DAY;
    // A visit counts one, halving every three days: what is opened often and lately stays longest.
    const HALF_LIFE = 3 * DAY;
    let opened;
    let checked = new Map();

    const key = (...names) => names.join("\n");
    const decayed = (item, now) => item.score * 2 ** (-Math.max(0, now - item.used) / HALF_LIFE);

    function open() {
        return opened ??= new Promise(resolve => {
            const request = indexedDB.open("symphony-cache", 1);
            request.onupgradeneeded = () => {
                for (const name of ["items", "parts"]) request.result.createObjectStore(name, { keyPath: "key" });
                request.result.createObjectStore("meta");
            };
            request.onsuccess = () => {
                // A newer client's upgrade in another tab closes this one; the next call opens again.
                request.result.onversionchange = () => {
                    request.result.close();
                    opened = undefined;
                    checked = new Map();
                };
                resolve(request.result);
            };
            request.onerror = () => resolve(null);
        }).catch(() => null);
    }

    // The store, once a store of another format is cleared.
    function ready(format) {
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
        // Every part of the item: the keys from `id` and a newline up to, not including, `id` and
        // the code unit after the newline.
        stores.parts.delete(IDBKeyRange.bound(key(id, ""), `${id}\v`, false, true));
    }

    // The items to drop: any unused for AGE or larger alone than its caps, then, while a cap is
    // passed, the lowest ranked first. A page read ahead ranks below every visited one and an
    // entrypoint above them all; among the rest the least and least lately visited goes first.
    // `items` come in key order and the sort is stable, so items ranked alike go by key.
    function plan(items, now) {
        const gone = new Set(items
            .filter(item => now - item.used > AGE || item.size > Math.min(CAPS[item.kind]?.[1] ?? PROJECT, PROJECT))
            .map(item => item.key));
        const order = items
            .filter(item => !gone.has(item.key))
            .map(item => [item, decayed(item, now)])
            .sort(([a, x], [b, y]) => a.pinned - b.pinned || a.visited - b.visited || x - y || a.used - b.used)
            .map(([item]) => item);
        const trim = (count, bytes, among) => {
            const left = order.filter(item => !gone.has(item.key) && among(item));
            let size = left.reduce((sum, item) => sum + item.size, 0);
            for (let at = 0, n = left.length; at < left.length && (n > count || size > bytes); at += 1, n -= 1) {
                gone.add(left[at].key);
                size -= left[at].size;
            }
        };
        for (const project of new Set(order.map(item => item.project))) {
            for (const [kind, [count, bytes]] of Object.entries(CAPS)) {
                trim(count, bytes, item => item.project === project && item.kind === kind);
            }
            trim(Infinity, PROJECT, item => item.project === project);
        }
        trim(Infinity, ORIGIN, () => true);
        return gone;
    }

    // Drop what `plan` names, then give whether `id` is still kept.
    function evict(stores, now, id, give) {
        stores.items.getAll().onsuccess = event => {
            const gone = plan(event.target.result, now);
            for (const dropped of gone) drop(stores, dropped);
            give(!gone.has(id));
        };
    }

    // The item `id` as kept, or a new one: a visit's, or one read ahead (`ahead`), unvisited.
    const item = (kept, id, project, kind, now, ahead) =>
        kept ?? { key: id, project, kind, used: now, score: 0, visited: !ahead, pinned: false, size: 0 };

    return {
        // `part` of `item` as kept: { value, etag, saved }, or null.
        read: (format, project, id, part) => within(format, ["parts"], (stores, give) => {
            stores.parts.get(key(project, id, part)).onsuccess = event => {
                const kept = event.target.result;
                if (kept?.format === format) give({ value: kept.value, etag: kept.etag, saved: kept.saved });
            };
        }, "readonly"),
        // Keep `value`, answered with `etag`, as `part` of the item. `size` is its JSON's length,
        // measured here when not given. Gives whether the item is still kept once the caps hold.
        keep: (format, project, id, kind, part, etag, value, size, ahead) => {
            const now = Date.now();
            return within(format, ["items", "parts"], (stores, give) => {
                // Inside the transaction's guard: a value with no JSON keeps nothing.
                size ??= JSON.stringify(value).length;
                const itemKey = key(project, id), partKey = key(itemKey, part);
                stores.items.get(itemKey).onsuccess = event => {
                    const kept = item(event.target.result, itemKey, project, kind, now, ahead);
                    stores.parts.get(partKey).onsuccess = event => {
                        kept.size += size - (event.target.result?.size ?? 0);
                        stores.parts.put({ key: partKey, format, etag, value, size, saved: now });
                        stores.items.put(kept);
                        evict(stores, now, itemKey, give);
                    };
                };
            });
        },
        // The page was opened: one more visit, on the count decayed since the last. `pinned`
        // marks a project's entrypoint page, kept while its project has room.
        visit: (format, project, id, kind, pinned) => {
            const now = Date.now();
            return within(format, ["items", "parts"], (stores, give) => {
                const itemKey = key(project, id);
                stores.items.get(itemKey).onsuccess = event => {
                    const kept = item(event.target.result, itemKey, project, kind, now, false);
                    Object.assign(kept, { score: decayed(kept, now) + 1, used: now, visited: true, pinned: !!pinned });
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
