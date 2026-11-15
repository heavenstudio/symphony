// Make a scratch project for the comparison: a git repository in a temporary folder with a fixed
// document set and its own board, filled through the board's own CLI. Every run of a preset makes
// the same documents and the same tasks, threads and statuses (a seeded generator), so both clients
// are measured on the same data.
//
//   node fixture/make.mjs PRESET FOLDER
//
// PRESET: `bench` (120 tasks, a task of 80 messages, the documents), `big500`, `big2000` (that many
// tasks, short briefs, the same documents), `plugins` (12 tasks, the same documents, and two of a
// project's own blocks with their modules, `docs/PLUGINS.md` and `docs/PROBE.md`, for the plugins
// section of the comparison). FOLDER must not exist. The environment names the tools:
// TASKS_BIN (the board's binary), SERVER_BIN (`symphony-server`), SYMPHONY_SURREAL (the pinned
// SurrealDB) and SYMPHONY_MACHINE_DIR (a temporary machine folder): never the live board, which
// this script cannot reach, as its board is the new repository's own.
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const [preset, folder] = process.argv.slice(2);
const PRESETS = {
  bench: { tasks: 120, thread: 80, briefs: "long" },
  big500: { tasks: 500, thread: 20, briefs: "short" },
  big2000: { tasks: 2000, thread: 20, briefs: "short" },
  plugins: { tasks: 12, thread: 4, briefs: "short" },
};
if (!PRESETS[preset] || !folder) {
  console.error("usage: node fixture/make.mjs bench|big500|big2000|plugins FOLDER");
  process.exit(2);
}
for (const name of ["TASKS_BIN", "SERVER_BIN", "SYMPHONY_SURREAL", "SYMPHONY_MACHINE_DIR"]) {
  if (!process.env[name]) {
    console.error(`make.mjs: ${name} is not set`);
    process.exit(2);
  }
}
if (fs.existsSync(folder)) {
  console.error(`make.mjs: ${folder} exists`);
  process.exit(2);
}
const shape = PRESETS[preset];
const repo = path.resolve(folder);

// A small seeded generator (mulberry32), so every run writes the same words.
let seed = 0x5eed;
function random() {
  seed |= 0;
  seed = (seed + 0x6d2b79f5) | 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
}
const pick = (list) => list[Math.floor(random() * list.length)];
const WORDS = (
  "board task column card feed stream server client page module brief thread message review " +
  "verdict landing branch commit build test release budget cache ghost section outline document " +
  "register decision owner agent orchestrator executor reviewer worktree schema migration " +
  "snapshot version reload theme header panel chat activity queue reports timeline gates"
).split(" ");
function sentence(min = 8, max = 18) {
  const n = min + Math.floor(random() * (max - min));
  const words = Array.from({ length: n }, () => pick(WORDS));
  words[0] = words[0][0].toUpperCase() + words[0].slice(1);
  return words.join(" ") + ".";
}
const paragraph = (sentences = 4) => Array.from({ length: sentences }, () => sentence()).join(" ");

// The documents: a README, a register of 24 cards (12 open across three groups), and a long guide
// of 60 sections for scrolling.
function documents() {
  const docs = path.join(repo, "docs");
  fs.mkdirSync(docs, { recursive: true });
  fs.writeFileSync(path.join(repo, "symphony.toml"), `[project]\nid = "${preset}"\nname = "Bench"\n`);
  fs.writeFileSync(
    path.join(docs, "README.md"),
    `# Bench\n\nThe comparison's scratch project. [The register](DECISIONS.md) and [the guide](GUIDE.md).\n`,
  );
  let register = "# Decision register\n\nThe bench project's decisions, one card each.\n\n<!-- register:start -->\n## Needs your decision\n\n";
  const groups = ["Board", "Task page", "Documents"];
  let card = 0;
  for (const group of groups) {
    register += `### ${group}\n\n`;
    for (let i = 0; i < 4; i++) {
      card++;
      register += `- [ ] **bench-card-${card}** — ${paragraph(2)} A: ${sentence(5, 9)} B: ${sentence(5, 9)} Recommended: A. Approve and say A or B in the note, or comment. [[GUIDE#Section 1]]\n`;
      register += `  [2026-11-13 10:0${i}] agent — Posted for the board task \`bench/card-${card}\`.\n\n    ${paragraph(3)}\n\n    ${paragraph(2)}\n\n`;
    }
  }
  register += "## Resolved\n\n";
  for (const group of groups) {
    register += `### ${group}\n\n`;
    for (let i = 0; i < 4; i++) {
      card++;
      register += `- [x] **bench-card-${card}** — ${paragraph(2)} Recommended: A.\n`;
      register += `  [2026-11-12 09:0${i}] agent — Posted.\n  [2026-11-12 10:0${i}] owner · approved — A.\n  [2026-11-12 10:1${i}] agent · resolved\n\n`;
    }
  }
  register += "<!-- register:end -->\n";
  fs.writeFileSync(path.join(docs, "DECISIONS.md"), register);
  let guide = "# Guide\n\nA long document the comparison scrolls through: sixty sections of prose, lists, code and tables.\n\n";
  for (let s = 1; s <= 60; s++) {
    guide += `## Section ${s}\n\n${paragraph(5)}\n\n${paragraph(4)}\n\n`;
    if (s % 3 === 0) guide += `- ${sentence()}\n- ${sentence()}\n- ${sentence()}\n\n`;
    if (s % 4 === 0) guide += "```rust\nfn section_" + s + "() -> u32 {\n    " + s + "\n}\n```\n\n";
    if (s % 5 === 0) guide += `| Part | Bytes | Note |\n|---|---|---|\n| board | ${s * 1000} | ${pick(WORDS)} |\n| task | ${s * 900} | ${pick(WORDS)} |\n\n`;
    if (s % 6 === 0) guide += `### Section ${s} detail\n\n${paragraph(3)}\n\n`;
  }
  fs.writeFileSync(path.join(docs, "GUIDE.md"), guide);
  if (preset === "plugins") plugins(docs);
}

// A project's own blocks (src/custom.jsx): a chart of the comparison's own first-visit bytes, and
// the probe, in a document of its own as it posts an owner's approve each time it is drawn in the
// page. The modules are the project's files, served beside its documents.
function plugins(docs) {
  const blocks = path.join(docs, "blocks");
  fs.mkdirSync(blocks);
  for (const name of ["chart.js", "probe.js"]) fs.copyFileSync(new URL(`blocks/${name}`, import.meta.url), path.join(blocks, name));
  const block = (value) => "```custom\n" + JSON.stringify(value) + "\n```\n";
  const bars = [
    ["the board, Dioxus", 307.3],
    ["the board, Preact", 43.5],
    ["a task's page, Dioxus", 768.0],
    ["a task's page, Preact", 48.4],
    ["the guide, Dioxus", 491.7],
    ["the guide, Preact", 59.6],
  ].map(([label, value], index) => ({ label, value, accent: index % 2 === 0 }));
  fs.writeFileSync(
    path.join(docs, "PLUGINS.md"),
    "# A project's own block\n\nThe chart below is drawn by the project's module, `docs/blocks/chart.js`, from the JSON in the block.\n\n" +
      "## First visits\n\n" +
      block({ kind: "chart", module: "/docs/blocks/chart.js", data: { title: "A first visit, kB on the wire (the comparison's Size table)", unit: "kB", bars } }) +
      `\n## After it\n\n${paragraph(3)}\n`,
  );
  fs.writeFileSync(
    path.join(docs, "PROBE.md"),
    "# What a module reaches\n\nThe probe below tries what a project's module could do from where it is drawn. It posts an owner's approve on a card of the register.\n\n" +
      block({ kind: "probe", module: "/docs/blocks/probe.js", data: { read: "/api/board", write: { path: "docs/DECISIONS.md", slug: "bench-card-12" } } }),
  );
}

function git(...args) {
  execFileSync("git", ["-C", repo, ...args], { stdio: ["ignore", "ignore", "inherit"] });
}

function tasks(args) {
  return execFileSync(process.env.TASKS_BIN, args, { cwd: repo, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

/** Run `jobs` (argument lists) `width` at a time. */
async function pool(jobs, width = 4) {
  let next = 0;
  const run = async () => {
    while (next < jobs.length) {
      const job = jobs[next++];
      await new Promise((resolve, reject) => {
        const child = spawn(process.env.TASKS_BIN, job, { cwd: repo, stdio: ["ignore", "ignore", "pipe"] });
        let err = "";
        child.stderr.on("data", (d) => (err += d));
        child.on("exit", (code) => (code === 0 ? resolve() : reject(new Error(`${job.join(" ")}: ${err}`))));
      });
    }
  };
  await Promise.all(Array.from({ length: width }, run));
}

/** The statuses tasks end in, by share of the board, and the moves each takes from ready. */
const ROUTES = {
  ready: [],
  backlog: ["backlog"],
  concept: ["concept"],
  design: ["design"],
  blocked: ["blocked"],
  cancelled: ["cancelled"],
  in_progress: ["in_progress"],
  review: ["in_progress", "review"],
  done: ["in_progress", "review"],
};
const SHARES = {
  bench: { ready: 30, backlog: 20, done: 30, in_progress: 12, review: 8, concept: 6, design: 6, blocked: 4, cancelled: 4 },
  big: { ready: 0.4, backlog: 0.2, done: 0.1, in_progress: 0.1, review: 0.05, concept: 0.05, design: 0.05, blocked: 0.03, cancelled: 0.02 },
};

function brief(long) {
  if (!long) return paragraph(2);
  return `${paragraph(4)}\n\n- ${sentence()}\n- ${sentence()}\n- ${sentence()}\n\n${paragraph(3)}\n\n\`\`\`sh\njust tasks show bench/x --as h\n\`\`\`\n\n${paragraph(2)}`;
}

async function main() {
  const started = Date.now();
  fs.mkdirSync(repo, { recursive: true });
  git("init", "-q", "-b", "main");
  documents();
  git("add", "-A");
  git("-c", "user.email=bench@example.invalid", "-c", "user.name=bench", "commit", "-qm", "The bench project");
  // The first command makes the board; the server then holds its database for the rest.
  const orchestrator = tasks(["register", "--kind", "orchestrator"]).split("\n").pop();
  const worker = tasks(["register", "--kind", "worker"]).split("\n").pop();
  const reviewer = tasks(["register", "--kind", "worker"]).split("\n").pop();
  const server = spawn(process.env.SERVER_BIN, ["--port", "0", "--root", repo], { cwd: repo, stdio: ["ignore", "pipe", "pipe"] });
  await new Promise((resolve, reject) => {
    let out = "";
    server.stdout.on("data", (d) => {
      out += d;
      if (/Symphony at http/.test(out)) resolve();
    });
    server.on("exit", (code) => reject(new Error(`the server ended (${code}) before it served: ${out}`)));
  });
  try {
    const share = preset === "bench" ? SHARES.bench : Object.fromEntries(Object.entries(SHARES.big).map(([s, f]) => [s, Math.round(f * shape.tasks)]));
    const plan = [];
    let n = 0;
    for (const [status, count] of Object.entries(share)) {
      for (let i = 0; i < count && n < shape.tasks; i++) plan.push({ slug: `${status.replace("_", "-")}/task-${++n}`, status });
    }
    while (plan.length < shape.tasks) plan.push({ slug: `ready/task-${++n}`, status: "ready" });
    // The task the task page is measured on, in progress with its long thread.
    plan[0] = { slug: "bench/task-page", status: "in_progress", thread: shape.thread };
    const long = shape.briefs === "long";
    await pool(
      plan.map((task, i) => {
        const args = ["create", task.slug, sentence(4, 10).slice(0, -1), "--body", brief(long), "--priority", String(i % 5), "--as", orchestrator];
        if (i % 3 === 0) args.push("--link", `docs/GUIDE.md#Section ${1 + (i % 60)}`);
        return args;
      }),
    );
    console.log(`created ${plan.length} tasks in ${((Date.now() - started) / 1000).toFixed(0)} s`);
    // Each route's moves in order: every task's first move, then every second move. Work in
    // progress is the worker's claim, and it is the worker who moves it to review.
    for (let step = 0; step < 2; step++) {
      const moves = plan
        .filter((t) => ROUTES[t.status][step])
        .map((t) =>
          ROUTES[t.status][step] === "in_progress"
            ? ["claim", t.slug, "--as", worker]
            : ["move", t.slug, ROUTES[t.status][step], "--as", ROUTES[t.status][step] === "review" ? worker : orchestrator],
        );
      await pool(moves);
    }
    const reviews = plan.filter((t) => t.status === "done" || t.status === "review").map((t) => ["role", t.slug, "reviewer", "--as", reviewer]);
    await pool(reviews);
    await pool(plan.filter((t) => t.status === "done").map((t) => ["verdict", t.slug, "approve", "--as", reviewer]));
    console.log(`moved in ${((Date.now() - started) / 1000).toFixed(0)} s`);
    // Threads: the measured task's long one, and a line or two on a quarter of the rest.
    const lines = [];
    for (let m = 0; m < shape.thread; m++) {
      const text = m % 4 === 3 ? `${paragraph(2)}\n\n- ${sentence()}\n- ${sentence()}\n\n\`code ${m}\`` : paragraph(1 + (m % 3));
      lines.push(["say", "bench/task-page", text, "--as", m % 2 ? worker : orchestrator]);
    }
    // In order, one at a time, so the thread reads as written.
    await pool(lines, 1);
    await pool(plan.filter((_, i) => i > 0 && i % 4 === 0).map((t) => ["say", t.slug, paragraph(1), "--as", orchestrator]));
    console.log(`done in ${((Date.now() - started) / 1000).toFixed(0)} s: ${preset} at ${repo}`);
    fs.writeFileSync(path.join(repo, "..", `${path.basename(repo)}.handles`), JSON.stringify({ orchestrator, worker, reviewer }) + "\n");
  } finally {
    server.kill("SIGTERM");
    await new Promise((resolve) => server.on("exit", resolve));
  }
}

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
