const modules = {"board":{"script":"/assets/board-b5c0c760aa08e9e9.js","wasm":"/assets/symphony-ui_bg-dxh9455413722ab4ce6.wasm","styles":["/assets/board-dxh40e7bf6d4d5b04b.css","/assets/dx-components-theme-dxhdaca25e6b513f84.css","/assets/frame-dxh35459b5b23befbac.css","/assets/style-dxh31297847c4f3588b.css","/assets/style-dxh3fe636e55754edc9.css","/assets/style-dxhd3c648de4f1c371.css","/assets/trigger-dxh7f148f299e5f1e68.css"]},"tasks":{"script":"/assets/tasks-b015d25434fcad92.js","wasm":"/assets/symphony-ui_bg-dxhc28970a9b96f6591.wasm","styles":["/assets/board-dxh40e7bf6d4d5b04b.css","/assets/dx-components-theme-dxhdaca25e6b513f84.css","/assets/frame-dxh35459b5b23befbac.css","/assets/media-dxhfbd62de4ca841cd.css","/assets/style-dxh31297847c4f3588b.css","/assets/style-dxh3fe636e55754edc9.css","/assets/style-dxhd3c648de4f1c371.css","/assets/trigger-dxh7f148f299e5f1e68.css"]},"docs":{"script":"/assets/docs-0df77fd972b22db0.js","wasm":"/assets/symphony-ui_bg-dxh53e50649385afc9.wasm","styles":["/assets/board-dxh40e7bf6d4d5b04b.css","/assets/docs-dxhe9169068fc4251b.css","/assets/dx-components-theme-dxhdaca25e6b513f84.css","/assets/frame-dxh35459b5b23befbac.css","/assets/media-dxhfbd62de4ca841cd.css","/assets/style-dxh31297847c4f3588b.css","/assets/style-dxh3fe636e55754edc9.css","/assets/style-dxhd3c648de4f1c371.css","/assets/trigger-dxh7f148f299e5f1e68.css"]},"review":{"script":"/assets/review-01d5e6db91228e1d.js","wasm":"/assets/symphony-ui_bg-dxh8530e116fbd9516c.wasm","styles":["/assets/board-dxh40e7bf6d4d5b04b.css","/assets/dx-components-theme-dxhdaca25e6b513f84.css","/assets/frame-dxh35459b5b23befbac.css","/assets/media-dxhfbd62de4ca841cd.css","/assets/style-dxh31297847c4f3588b.css","/assets/style-dxh3fe636e55754edc9.css","/assets/style-dxhd3c648de4f1c371.css","/assets/trigger-dxh7f148f299e5f1e68.css"]},"queue":{"script":"/assets/queue-9ad1e1aff86e0f03.js","wasm":"/assets/symphony-ui_bg-dxh59fb3e2da3b6cec.wasm","styles":["/assets/board-dxh40e7bf6d4d5b04b.css","/assets/dx-components-theme-dxhdaca25e6b513f84.css","/assets/frame-dxh35459b5b23befbac.css","/assets/media-dxhfbd62de4ca841cd.css","/assets/queue-dxh9f7f3b636b40f4f.css","/assets/style-dxh31297847c4f3588b.css","/assets/style-dxh3fe636e55754edc9.css","/assets/style-dxhd3c648de4f1c371.css","/assets/trigger-dxh7f148f299e5f1e68.css"]},"panel":{"script":"/assets/panel-338e562f244fc8b2.js","wasm":"/assets/symphony-ui_bg-dxhb3662efc67e2a.wasm","styles":["/assets/board-dxh40e7bf6d4d5b04b.css","/assets/dx-components-theme-dxhdaca25e6b513f84.css","/assets/frame-dxh35459b5b23befbac.css","/assets/media-dxhfbd62de4ca841cd.css","/assets/style-dxh31297847c4f3588b.css","/assets/style-dxh3fe636e55754edc9.css","/assets/style-dxh459d38675f8d4312.css","/assets/style-dxh8b2c17c88a71228b.css","/assets/style-dxh9e3de2baffc3c971.css","/assets/style-dxhac6293182ffb122.css","/assets/style-dxhb6419663bbeef60.css","/assets/style-dxhc48d3ea1f423d982.css","/assets/style-dxhcd765922136dde65.css","/assets/style-dxhd3c648de4f1c371.css","/assets/trigger-dxh7f148f299e5f1e68.css"]},"overlay":{"script":"/assets/overlay-6589d8d02e451209.js","wasm":"/assets/symphony-ui_bg-dxhc078a1b293fa1477.wasm","styles":["/assets/board-dxh40e7bf6d4d5b04b.css","/assets/dx-components-theme-dxhdaca25e6b513f84.css","/assets/frame-dxh35459b5b23befbac.css","/assets/media-dxhfbd62de4ca841cd.css","/assets/style-dxh31297847c4f3588b.css","/assets/style-dxh3fe636e55754edc9.css","/assets/style-dxh459d38675f8d4312.css","/assets/style-dxh8b2c17c88a71228b.css","/assets/style-dxh9e3de2baffc3c971.css","/assets/style-dxhac6293182ffb122.css","/assets/style-dxhb6419663bbeef60.css","/assets/style-dxhc48d3ea1f423d982.css","/assets/style-dxhcd765922136dde65.css","/assets/style-dxhd3c648de4f1c371.css","/assets/trigger-dxh7f148f299e5f1e68.css"]}};
try {
const css = localStorage.getItem(stored("studio.cssVars"));
if (css) {
const style = document.createElement("style");
style.id = "studio-theme";
style.textContent = css;
document.head.append(style);
document.documentElement.setAttribute("data-studio-theme", "");
}
} catch (_) { /* Storage can be unavailable in a private browser. */ }
let active;
let requested = 0;
let switching = Promise.resolve();
const styleLoads = new Map();
const failed = new Set();
const preloaded = new Set();

const PERSISTENT = {
panel: { root: "quick-root", ready: "reviewPanelReady", said: "Conversation could not be loaded. ", retry: "Retry loading conversation" },
overlay: { root: "overlay-root", ready: "reviewOverlayReady", said: "Menus and pictures could not be loaded. ", retry: "Retry loading menus" },
};
const persistent = new Map();

function alarm(id, said, retry, again) {
let notice = document.getElementById(id);
if (!notice) {
notice = document.createElement("div");
notice.id = id;
notice.className = "shell-notice";
notice.setAttribute("role", "alert");
document.getElementById("shell-notices").append(notice);
}
const button = document.createElement("button");
button.textContent = retry;
button.onclick = () => { notice.remove(); again(); };
notice.replaceChildren(said, button);
}

async function initialize(kind) {
const spec = modules[kind];
if (!preloaded.has(spec.wasm)) {
preloaded.add(spec.wasm);
document.head.insertAdjacentHTML("beforeend", `<link rel="preload" as="fetch" crossorigin href="${spec.wasm}">`);
}
const module = await import(spec.script);
try {
if (failed.has(kind)) await module.default({ module_or_path: spec.wasm });
else await window[`reviewModuleReady_${kind}`];
failed.delete(kind);
return window[`reviewModule_${kind}`];
} catch (error) { failed.add(kind); throw error; }
}

function mountFor(kind) {
const shared = PERSISTENT[kind];
const id = shared ? shared.root : `review-root-${kind}`;
let mount = document.getElementById(id);
if (!mount) {
mount = document.createElement("div");
mount.id = id;
mount.hidden = !shared;
(shared ? document.body : document.getElementById("main")).append(mount);
}
return mount;
}

function styles(spec) {
const pending = [];
for (const href of spec.styles) {
let loaded = styleLoads.get(href);
if (!loaded) {
let link = [...document.querySelectorAll("link[rel=stylesheet]")].find(link => link.getAttribute("href") === href);
if (link?.sheet) loaded = Promise.resolve();
else {
const fresh = !link;
link ||= document.createElement("link");
loaded = new Promise((resolve, reject) => {
link.addEventListener("load", resolve, { once: true });
link.addEventListener("error", () => {
styleLoads.delete(href);
link.remove();
reject(new Error(`Styles could not load: ${href}`));
}, { once: true });
});
if (fresh) { link.rel = "stylesheet"; link.href = href; document.head.insertBefore(link, document.head.querySelector("[data-symphony-theme]")); }
}
styleLoads.set(href, loaded);
}
pending.push(loaded);
}
return Promise.all(pending);
}

function ensure(kind) {
const shared = PERSISTENT[kind];
const failure = `${kind}-error`;
if (!persistent.has(kind)) {
persistent.set(kind, (async () => {
mountFor(kind);
const [imported] = await Promise.all([initialize(kind), styles(modules[kind])]);
await imported.run();
document.getElementById(failure)?.remove();
window[shared.ready] = true;
dispatchEvent(new Event(`review:${kind}-ready`));
return { imported };
})().catch(error => {
persistent.delete(kind);
console.error(shared.said, error);
if (kind === "panel") loadingSheets();
alarm(failure, shared.said, shared.retry, () => ensure(kind));
}));
}
if (kind === "panel") loadingSheets();
return persistent.get(kind);
}

function loadingSheets() {
document.getElementById("panel-loading")?.remove();
if (!persistent.has("panel") || window.reviewPanelReady) return;
const sheet = (open, id, label, inner, what) => open ? `<aside id="${id}" aria-label="${label}" aria-busy="true">${inner}<p role="status">Loading ${what}…</p></aside>` : "";
const sheets = sheet(window.reviewActivityOpen === true, "activity-panel", "Recent activity", `<div class="panel-heading"><h2>Recent activity</h2></div>`, "activity")
+ sheet(window.reviewQuickDraft?.open === true, "quick-sheet", "Chat or add a task", "", "conversation");
if (sheets) document.body.insertAdjacentHTML("beforeend", `<div id="panel-loading">${sheets}</div>`);
}
addEventListener("review:panel-ready", () => document.getElementById("panel-loading")?.remove());

window.reviewOverlay = request => {
if (request.kind === "close") {
if (window.reviewOverlayRequest?.owner !== request.owner) return;
window.reviewOverlayRequest = null;
dispatchEvent(new Event("review:overlay"));
return;
}
window.reviewOverlayRequest = request;
ensure("overlay").then(() => dispatchEvent(new Event("review:overlay")));
};

let warmed = false;
function warm() {
const link = navigator.connection, kinds = Object.keys(PERSISTENT);
if (warmed || link?.saveData || /2g/.test(link?.effectiveType)) return;
warmed = true;
(window.requestIdleCallback || setTimeout)(function next() {
const kind = kinds.shift();
if (!kind) return;
if (persistent.has(kind)) return persistent.get(kind).then(next);
const { script, wasm, styles } = modules[kind];
for (const href of [wasm, script, ...styles]) {
if (!document.querySelector(`link[href="${href}"]`)) document.head.insertAdjacentHTML("beforeend", `<link rel="prefetch" href="${href}">`);
}
const last = document.querySelector(`link[rel=prefetch][href="${wasm}"]`);
if (last) last.onload = last.onerror = next; else next();
}, { timeout: 5000 });
}

const SYMPHONY_ROUTES = ["tasks", "task", "agents", "queue", "api", "assets", "static", "thumb", "x", "artifacts/uploads", "app", "doc", "history", "diff", "wasm", "_dioxus", "favicon.ico"];

function within(route, path) {
return route === "" || path === route || path.startsWith(`${route}/`);
}

function project() {
try { return JSON.parse(document.getElementById("symphony-project")?.textContent || "{}"); } catch { return {}; }
}

function stored(key) {
const id = (project().base || "").replace(/^\/+/, "");
return id ? `${id}:${key}` : key;
}

function here(path) {
const base = project().base || "";
return !base || path === base || path.startsWith(`${base}/`) ? path : base + path;
}

function route(path) {
try { path = decodeURIComponent(path); } catch { return null; }
const base = project().base || "";
if (base && (path === base || path.startsWith(`${base}/`))) path = path.slice(base.length) || "/";
path = path.replace(/\/{2,}/g, "/").replace(/^\//, "").replace(/^app\//, "");
if (path.startsWith("@")) {
const slash = path.indexOf("/");
if (slash < 0) return null;
path = path.slice(slash + 1).replace(/^app\//, "");
}
path = path.replace(/^doc\//, "");
const clean = path.replace(/\/$/, "");
if (clean === "tasks" || clean === "task") return "board";
if (path.startsWith("task/") || clean === "agents") return "tasks";
if (clean === "queue") return "queue";
const view = project();
const pages = (view.nav || []).map(item => item.href.replace(/^\/+|\/+$/g, "")).filter(Boolean);
if (pages.includes(clean)) return "docs";
const urls = view.entrypoints?.length ? view.entrypoints : ["docs"];
const name = clean.split("/").pop();
if (!SYMPHONY_ROUTES.concat(pages).some(taken => within(taken, clean))
&& urls.some(url => within(url, clean))
&& (path.endsWith("/") || urls.includes(clean) || !name.includes(".") || name.endsWith(".md"))) return "docs";
return clean === "" ? "review" : null;
}

function scrollAnchor(preserveScroll) {
let id;
try { id = decodeURIComponent(location.hash.slice(1)); } catch { return; }
if (!id) { if (!preserveScroll) scrollTo(0, 0); return; }
const target = document.getElementById(id);
if (!target) return;
for (let parent = target; parent; parent = parent.parentElement) {
if (parent instanceof HTMLDetailsElement) parent.open = true;
}
target.scrollIntoView();
}

async function show(generation, scroll) {
if (generation !== requested) return;
const kind = route(location.pathname);
if (!kind) { location.reload(); return; }
if (active?.kind !== kind) {
const spec = modules[kind];
const mount = mountFor(kind);
const [imported] = await Promise.all([initialize(kind), styles(spec)]);
if (generation !== requested) return;
if (active) {
await active.imported.stop();
active.mount.hidden = true;
}
mount.hidden = false;
await imported.run();
active = { kind, imported, mount };
}
document.getElementById("module-error")?.remove();
const root = document.scrollingElement;
const position = {
x: scrollX, y: scrollY,
maxX: Math.max(0, root.scrollWidth - root.clientWidth),
maxY: Math.max(0, root.scrollHeight - root.clientHeight),
};
dispatchEvent(new Event("review:navigate"));
requestAnimationFrame(() => requestAnimationFrame(() => {
if (generation !== requested) return;
const maxX = Math.max(0, root.scrollWidth - root.clientWidth);
const maxY = Math.max(0, root.scrollHeight - root.clientHeight);
const x = maxX < position.maxX ? Math.min(position.x, maxX) : position.x;
const y = maxY < position.maxY ? Math.min(position.y, maxY) : position.y;
const moved = x !== scrollX || y !== scrollY;
const restored = scroll.initial && (scroll.x !== 0 || scroll.y !== 0);
scrollAnchor(moved || restored);
document.documentElement.dataset.navigation = "ready";
warm();
}));
}

function navigate() {
const overlay = window.reviewOverlayRequest;
if (overlay) {
window.reviewOverlayRequest = null;
dispatchEvent(new Event("review:overlay"));
overlay.callback?.(null);
}
const generation = ++requested;
const scroll = { x: scrollX, y: scrollY, initial: !active };
document.documentElement.dataset.navigation = "loading";
switching = switching.catch(() => {}).then(() => show(generation, scroll)).catch(error => {
if (generation === requested) document.documentElement.dataset.navigation = "failed";
console.error("Page could not be loaded", error);
alarm("module-error", "This page could not be loaded. ", "Retry", navigate);
});
}

document.addEventListener("click", event => {
if (event.defaultPrevented || event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
const link = event.target instanceof Element && event.target.closest("a[href]");
if (!link || link.hasAttribute("download") || link.dataset.native === "true" || (link.target && link.target !== "_self")) return;
const url = new URL(link.href, location.href);
if (url.origin !== location.origin) return;
url.pathname = here(url.pathname);
if (!route(url.pathname)) return;
event.preventDefault();
if (document.querySelector("[data-posting=true]")) return;
history.pushState(null, "", url);
navigate();
});
addEventListener("popstate", navigate);
addEventListener("review:quick", () => { if (window.reviewQuickDraft?.open) ensure("panel"); else loadingSheets(); });
addEventListener("review:activity", () => { if (window.reviewActivityOpen) ensure("panel"); else loadingSheets(); });
addEventListener("review:activity-arrived", () => ensure("panel"));
async function pendingStep(step) {
await ensure("panel");
requestAnimationFrame(() => requestAnimationFrame(() => {
document.querySelector(`[data-pending-action=${step}]`)?.click();
}));
}
document.addEventListener("click", event => {
const step = event.target instanceof Element && event.target.closest("[data-pending-step]")?.dataset.pendingStep;
if (step) { event.preventDefault(); pendingStep(step); }
});
addEventListener("keydown", event => {
if (window.reviewPanelReady || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
if (event.target instanceof Element && event.target.closest("input,textarea,select,[contenteditable],dialog[open],[role=dialog],[role=listbox],[role=menu]")) return;
const step = event.key === "[" ? "prev" : event.key === "]" ? "next" : null;
if (step) { event.preventDefault(); pendingStep(step); }
});
addEventListener("beforeunload", event => {
if (document.querySelector("[data-posting=true]")) { event.preventDefault(); event.returnValue = ""; }
});
navigate();

// Notice styles: 14b51b133c208438
