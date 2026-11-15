// The tables of the comparison, from the bench's JSON lines (one file or several):
//
//   node bench/summarize.mjs RESULTS.jsonl... [--assets NAME=URL...] [--batch N]
//
// A table per scenario, fixture, CPU and screen, a column per client: the median of its runs with
// the lowest and highest in brackets, then each client's median over the first client's (the
// ratio, which a busy machine moves much less than a time), with the same ratio for each batch of
// N rounds (5 by default) after it. The clients take turns within a round, so a batch's rounds
// shared the machine's load; a ratio whose batches fall on both sides of 1 is marked
// "undecided". `--assets` also fetches, from each client's server, the static files a cold visit
// loaded (the page and everything under /assets/), and prints their bodies compressed as gzip at
// level 9 and as Brotli at quality 11.
import { readFileSync } from "node:fs";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";

const files = [];
const servers = {};
let batch = 5;
for (let i = 2; i < process.argv.length; i++) {
  const arg = process.argv[i];
  if (arg === "--assets") {
    const [name, url] = process.argv[++i].split(/=(.*)/s);
    servers[name] = url.replace(/\/$/, "");
  } else if (arg === "--batch") batch = Number(process.argv[++i]);
  else files.push(arg);
}

const records = files.flatMap((file) =>
  readFileSync(file, "utf8")
    .split("\n")
    .filter((line) => line.startsWith("{"))
    .map((line) => JSON.parse(line)),
);

const finite = (values) => values.filter((v) => Number.isFinite(v));
const median = (values) => {
  const sorted = finite(values).sort((a, b) => a - b);
  if (!sorted.length) return NaN;
  const mid = sorted.length >> 1;
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
};
const quantile = (values, q) => {
  const sorted = finite(values).sort((a, b) => a - b);
  return sorted.length ? sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))] : NaN;
};
const round = (v, digits = 0) => (Number.isFinite(v) ? Number(v.toFixed(digits)).toLocaleString("en-US") : "–");
const cell = (values, digits = 0) => {
  const list = finite(values);
  if (!list.length) return "–";
  const m = median(list);
  if (list.length === 1 || Math.min(...list) === Math.max(...list)) return round(m, digits);
  return `${round(m, digits)} (${round(Math.min(...list), digits)}–${round(Math.max(...list), digits)})`;
};
const kb = (bytes) => bytes / 1000;
const sum = (list) => list.reduce((a, b) => a + b, 0);
const longTotal = (tasks) => sum((tasks ?? []).map(([, d]) => d));
const keys = (r) => r.inputs.filter(([k]) => k === "key").map(([, ms]) => ms);
const oneMinute = (r) => Number(String(r.load).replace(/[{}]/g, "").trim().split(/\s+/)[0]);

const VISIT = [
  ["first contentful paint, cold (ms)", (r) => r.cold.fcp],
  ["usable, cold (ms)", (r) => r.cold.usable],
  ["usable, warm (ms)", (r) => r.warm.usable],
  ["last read answered, cold (ms)", (r) => r.cold.reads_done],
  ["first visit (kB on the wire, as the budget counts)", (r) => kb(r.cold.bytes)],
  ["return visit (kB on the wire)", (r) => kb(r.warm.bytes)],
  ["warmed at idle, outside the visit (kB on the wire)", (r) => kb(r.cold.prefetched)],
  ["requests, cold", (r) => r.cold.requests.length],
  ["script loaded, decoded: JS (kB)", (r) => kb(r.cold.code?.js ?? 0)],
  ["script loaded, decoded: wasm (kB)", (r) => kb(r.cold.code?.wasm ?? 0)],
  ["style sheets loaded, decoded (kB)", (r) => kb(r.cold.code?.css ?? 0)],
  ["DOM changes over the cold visit", (r) => r.cold.mutations],
  ["DOM nodes held after the cold visit (attached or not)", (r) => r.cold.metrics.Nodes],
  ["JS heap after the cold visit (MB)", (r) => r.cold.metrics.JSHeapUsedSize / 1e6],
  ["script time, cold (ms)", (r) => 1000 * r.cold.metrics.ScriptDuration],
  ["long tasks, cold (count)", (r) => r.cold.longtasks.length],
  ["long tasks, cold (total ms)", (r) => longTotal(r.cold.longtasks)],
];

/** A scenario's rows, as `[label, (record) => number or numbers]`. */
const TABLES = {
  "visit-board": [
    ...VISIT,
    ["task page after a card click: usable (ms)", (r) => r.second?.usable_after_click],
    ["task page after a card click: bytes (kB)", (r) => kb(r.second?.bytes ?? NaN)],
  ],
  "visit-task": VISIT,
  "visit-doc": VISIT,
  feed: [
    ["priority change: event to drawn (ms)", (r) => r.updates.filter((u) => u.kind === "priority").map((u) => u.cost)],
    ["card move: event to drawn (ms)", (r) => r.updates.filter((u) => u.kind === "move").map((u) => u.cost)],
    ["priority change: DOM changes to draw it", (r) => r.updates.filter((u) => u.kind === "priority").map((u) => u.mutations_drawn)],
    ["priority change: DOM changes with the refresh after", (r) => r.updates.filter((u) => u.kind === "priority").map((u) => u.mutations)],
    ["card move: DOM changes to draw it", (r) => r.updates.filter((u) => u.kind === "move").map((u) => u.mutations_drawn)],
    ["card move: DOM changes with the refresh after", (r) => r.updates.filter((u) => u.kind === "move").map((u) => u.mutations)],
    ["long tasks over the run (count)", (r) => r.longtasks.length],
  ],
  type: [
    ["key to next frame, median (ms)", (r) => median(keys(r))],
    ["key to next frame, p95 (ms)", (r) => quantile(keys(r), 0.95)],
    ["key to next frame, worst (ms)", (r) => Math.max(...keys(r))],
    ["DOM changes per key", (r) => (r.mutations ?? NaN) / Math.max(1, keys(r).length)],
    ["long tasks while typing (count)", (r) => r.longtasks.length],
  ],
  scroll: [
    ["frames over the scroll", (r) => r.frames],
    ["frame interval, median (ms)", (r) => median(r.gaps)],
    ["frame interval, p95 (ms)", (r) => quantile(r.gaps, 0.95)],
    ["frames over 50 ms", (r) => r.gaps.filter((g) => g > 50).length],
    ["long tasks (count)", (r) => r.longtasks.length],
    ["long tasks (total ms)", (r) => longTotal(r.longtasks)],
    ["DOM changes over the scroll", (r) => r.mutations],
    ["DOM nodes at the end", (r) => r.metrics?.Nodes],
    ["every section drawn, after the scroll began (ms)", (r) => r.drawn_after ?? NaN],
    ["sections left waiting", (r) => r.left_waiting ?? 0],
  ],
  "load-more": [
    ["usable, cold (ms)", (r) => r.cold.usable],
    ["first visit (kB on the wire)", (r) => kb(r.cold.bytes)],
    ["pages loaded", (r) => r.clicks.length],
    ["click to rows drawn, every page (ms)", (r) => r.clicks.map((c) => c.ms)],
    ["answer to rows drawn, every page (ms)", (r) => r.clicks.map((c) => c.after_answer)],
    ["answer to rows drawn, last page (ms)", (r) => r.clicks.at(-1)?.after_answer],
    ["DOM changes per page of rows", (r) => r.clicks.map((c) => c.mutations)],
    ["rows before the last click", (r) => r.clicks.at(-1)?.rows_before],
    ["long tasks over the run (count)", (r) => r.longtasks.length],
  ],
  memory: [
    ["JS heap used, start (MB)", (r) => r.start.heap.usedSize / 1e6],
    ["JS heap used, end (MB)", (r) => r.end.heap.usedSize / 1e6],
    ["wasm linear memory, start (MB)", (r) => sum(r.start.wasm ?? []) / 1e6],
    ["wasm linear memory, end (MB)", (r) => sum(r.end.wasm ?? []) / 1e6],
    ["embedder heap, end (MB)", (r) => (r.end.heap.embedderHeapUsedSize ?? NaN) / 1e6],
    ["DOM nodes, start", (r) => r.start.dom.nodes],
    ["DOM nodes, end", (r) => r.end.dom.nodes],
    ["event listeners, end", (r) => r.end.dom.jsEventListeners],
    ["feed events received", (r) => r.deltas],
    ["board changes made", (r) => r.changes],
  ],
};

const take = (row, rows) =>
  rows.flatMap((r) => {
    try {
      const v = row(r);
      return Array.isArray(v) ? v : [v];
    } catch {
      return [NaN];
    }
  });

const groups = new Map();
for (const record of records) {
  const key = `${record.scenario} · ${record.fixture ?? "bench"} · CPU ${record.cpu}x · ${record.screen ?? "wide"}`;
  if (!groups.has(key)) groups.set(key, []);
  groups.get(key).push(record);
}

for (const [key, rows] of groups) {
  const table = TABLES[rows[0].scenario];
  if (!table) continue;
  const clients = [...new Set(rows.map((r) => r.client))];
  const [base, ...others] = clients;
  const batches = [...new Set(rows.map((r) => Math.floor(r.run / batch)))].sort((a, b) => a - b);
  const inBatch = (b) => rows.filter((r) => Math.floor(r.run / batch) === b);
  const loads = batches.map((b) => {
    const ones = inBatch(b).map(oneMinute);
    return `${round(Math.min(...ones), 0)}–${round(Math.max(...ones), 0)}`;
  });
  const rounds = Math.max(...clients.map((c) => rows.filter((r) => r.client === c).length));
  const several = batches.length > 1;
  console.log(`\n### ${key}\n`);
  console.log(`${rounds} rounds${several ? ` in ${batches.length} batches of ${batch}` : ""}; 1-minute load ${loads.join(", ")}${several ? " by batch" : ""}.\n`);
  const ratioHead = others.map((c) => `${c} ÷ ${base}${several ? " (by batch)" : ""}`);
  console.log(`| | ${clients.join(" | ")} | ${ratioHead.join(" | ")} |`);
  console.log(`|---|${clients.map(() => "---:").join("|")}|${others.map(() => "---:").join("|")}|`);
  for (const [label, row] of table) {
    const digits = label.includes("(kB") || label.includes("(MB)") || label.includes("per key") ? 1 : 0;
    const cells = clients.map((client) => cell(take(row, rows.filter((r) => r.client === client)), digits));
    if (cells.every((c) => c === "–")) continue;
    const ratios = others.map((client) => {
      const of = (part, who) => median(take(row, part.filter((r) => r.client === who)));
      const whole = of(rows, client) / of(rows, base);
      if (!Number.isFinite(whole)) return "–";
      if (!several) return round(whole, 2);
      const each = batches.map((b) => of(inBatch(b), client) / of(inBatch(b), base));
      const flips = finite(each).some((v) => v < 1) && finite(each).some((v) => v > 1);
      return `${round(whole, 2)} (${each.map((v) => round(v, 2)).join(", ")})${flips ? " undecided" : ""}`;
    });
    console.log(`| ${label} | ${cells.join(" | ")} | ${ratios.join(" | ")} |`);
  }
}

// The static files each page's cold visit loaded, compressed both ways.
if (Object.keys(servers).length) {
  console.log("\n### Static files of a cold visit, compressed (kB)\n");
  console.log("| page | client | files | on the wire (gzip, heads included) | gzip -9 | Brotli 11 | raw |");
  console.log("|---|---|---:|---:|---:|---:|---:|");
  const seen = new Set();
  for (const record of records.filter((r) => r.scenario.startsWith("visit-") && r.cpu === 1 && (r.fixture ?? "bench") === "bench")) {
    const page = record.scenario.slice(6);
    const id = `${page}/${record.client}`;
    if (seen.has(id) || !servers[record.client]) continue;
    seen.add(id);
    const statics = record.cold.requests.filter((r) => !r.url.startsWith("api/") && r.kind !== "EventSource" && r.kind !== "Fetch" && !r.prefetch);
    let wire = 0;
    let gz = 0;
    let br = 0;
    let raw = 0;
    for (const request of statics) {
      const response = await fetch(`${servers[record.client]}/${request.url}`, { headers: { "accept-encoding": "identity" } });
      const body = Buffer.from(await response.arrayBuffer());
      wire += request.bytes;
      raw += body.length;
      gz += gzipSync(body, { level: 9 }).length;
      br += brotliCompressSync(body, { params: { [constants.BROTLI_PARAM_QUALITY]: 11, [constants.BROTLI_PARAM_SIZE_HINT]: body.length } }).length;
    }
    console.log(`| ${page} | ${record.client} | ${statics.length} | ${round(kb(wire), 1)} | ${round(kb(gz), 1)} | ${round(kb(br), 1)} | ${round(kb(raw), 1)} |`);
  }
}
