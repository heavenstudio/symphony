//! The client comparison's measurements: each client served by a Symphony server of its own on the
//! same scratch project, each run in a fresh headless Chrome (the workspace's driver,
//! `crates/chrome-devtools`) on its own profile. One JSON line per run and client on stdout; the
//! README names the commands and `summarize.mjs` makes the tables.
//!
//!   client-bench SCENARIO --client NAME=URL... [--runs N] [--cpu 1|4] [--fixture DIR --handle H]
//!
//! Scenarios: `visit-board`, `visit-task`, `visit-doc` (a cold visit, a warm one, and from the
//! board a click to the task page), `feed` (priority changes and moves made on the board, timed
//! from their event's arrival to the frame that draws them), `type` (keys in the task's message
//! box), `scroll` (the long document scrolled to its end, and any sections a stalled frame carried
//! the scroll past left waiting), `load-more` (the ready column paged to
//! its end) and `memory` (the board open for `--minutes` of changes, every client at once).
//! `--cpu 4` slows the CPU four times; `--screen phone` draws on a phone's screen, a 1440 x 900
//! window otherwise.

use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use symphony_chrome::{Chrome, Leash, Request};

/// What every page loads before its own scripts: when the watched selectors first draw (checked
/// at each frame, timed when the frame's callbacks run, after the page's own work and before paint), the long tasks, first contentful paint, each feed event's arrival (a listener
/// that runs before the page's own), and each key or click's time to the next frame.
const INIT: &str = r#"(() => {
  if (window.__bench) return;
  const b = (window.__bench = { longtasks: [], seen: {}, watch: {}, deltas: [], inputs: [], frames: null, fcp: null, lcp: null });
  b.expect = (name, selector) => { b.watch[name] = selector; delete b.seen[name]; };
  Object.assign(b.watch, __WATCH__);
  b.errors = [];
  // The DOM's changes, a count the machine's load does not move: each node added or removed,
  // each attribute and each text changed.
  b.mutations = 0;
  new MutationObserver((list) => {
    for (const r of list) b.mutations += r.type === "childList" ? r.addedNodes.length + r.removedNodes.length : 1;
  }).observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
  for (const name of ["instantiate", "instantiateStreaming"]) {
    const original = WebAssembly[name];
    if (!original) continue;
    WebAssembly[name] = async function (...args) {
      const result = await original.apply(this, args);
      const instance = result.instance || result;
      for (const value of Object.values(instance.exports || {})) if (value instanceof WebAssembly.Memory) (b.memories ||= []).push(value);
      return result;
    };
  }
  addEventListener("error", (e) => b.errors.push(String(e.message || e.error)));
  addEventListener("unhandledrejection", (e) => b.errors.push("rejection: " + String(e.reason && (e.reason.stack || e.reason))));
  const report = console.error;
  console.error = (...args) => { b.errors.push(args.map(String).join(" ").slice(0, 300)); report.apply(console, args); };
  const po = (type, fn) => { try { new PerformanceObserver((list) => list.getEntries().forEach(fn)).observe({ type, buffered: true }); } catch (_) {} };
  po("longtask", (e) => b.longtasks.push([Math.round(e.startTime), Math.round(e.duration)]));
  po("paint", (e) => { if (e.name === "first-contentful-paint") b.fcp = e.startTime; });
  po("largest-contentful-paint", (e) => { b.lcp = e.startTime; });
  const frame = (t) => {
    const now = performance.now();
    for (const name in b.watch) {
      const w = b.watch[name];
      if (!(name in b.seen) && (typeof w === "function" ? w() : document.querySelector(w))) b.seen[name] = now;
    }
    if (b.frames) b.frames.push(t);
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
  const next = (since, kind) => requestAnimationFrame(() => {
    const channel = new MessageChannel();
    channel.port1.onmessage = () => b.inputs.push([kind, Math.round((performance.now() - since) * 10) / 10]);
    channel.port2.postMessage(0);
  });
  addEventListener("keydown", (e) => next(e.timeStamp, "key"), true);
  addEventListener("click", (e) => next(e.timeStamp, "click"), true);
  const add = EventSource.prototype.addEventListener;
  EventSource.prototype.addEventListener = function (type, fn, options) {
    if (!this.__bench) { this.__bench = true; add.call(this, "delta", () => b.deltas.push(performance.now())); }
    return add.call(this, type, fn, options);
  };
})();"#;

/// The pages measured, each with the selector that says it is usable on both clients.
struct Page {
    path: &'static str,
    usable: &'static str,
}

const BOARD: Page = Page {
    path: "/tasks",
    usable: ".taskcard",
};
const TASK: Page = Page {
    path: "/task/bench/task-page",
    usable: "textarea, .thread .entry",
};
const DOC: Page = Page {
    path: "/docs/GUIDE.md",
    usable: ".doc-section .blocks, .doc-section p",
};

/// The page's usable selector, overridable for the helpers' answers (`--usable-task`, `--usable-doc`).
struct Selectors {
    task: String,
    doc: String,
}

struct Args {
    scenario: String,
    clients: Vec<(String, String)>,
    runs: usize,
    cpu: u32,
    fixture: Option<PathBuf>,
    handle: Option<String>,
    minutes: u64,
    phone: bool,
    selectors: Selectors,
}

fn args() -> Args {
    let mut raw = std::env::args().skip(1);
    let scenario = raw.next().unwrap_or_default();
    let mut args = Args {
        scenario,
        clients: Vec::new(),
        runs: 5,
        cpu: 1,
        fixture: None,
        handle: None,
        minutes: 10,
        phone: false,
        selectors: Selectors {
            task: TASK.usable.into(),
            doc: DOC.usable.into(),
        },
    };
    while let Some(flag) = raw.next() {
        let value = raw.next().unwrap_or_else(|| panic!("{flag} takes a value"));
        match flag.as_str() {
            "--client" => {
                let (name, url) = value.split_once('=').expect("--client NAME=URL");
                args.clients
                    .push((name.into(), url.trim_end_matches('/').into()));
            }
            "--runs" => args.runs = value.parse().expect("--runs N"),
            "--cpu" => args.cpu = value.parse().expect("--cpu 1|4"),
            "--fixture" => args.fixture = Some(value.into()),
            "--handle" => args.handle = Some(value),
            "--minutes" => args.minutes = value.parse().expect("--minutes N"),
            "--screen" => args.phone = value == "phone",
            "--usable-task" => args.selectors.task = value,
            "--usable-doc" => args.selectors.doc = value,
            _ => panic!("unknown flag {flag}"),
        }
    }
    assert!(
        !args.clients.is_empty(),
        "name at least one --client NAME=URL"
    );
    args
}

/// A fresh Chrome on its own profile, `watch` installed in every page, at the run's CPU and screen.
fn chrome(args: &Args, watch: &Value) -> Chrome {
    let (cpu, phone) = (args.cpu, args.phone);
    let binary = symphony_chrome::find().expect("no Chrome: name it with SYMPHONY_CHROME");
    let program = std::env::current_exe().expect("this program's path");
    let mut chrome =
        Chrome::launch(&binary, &Leash::new(program, &[]).quiet()).expect("Chrome starts");
    let source = INIT.replace("__WATCH__", &watch.to_string());
    chrome
        .call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": source }),
        )
        .expect("the init script");
    chrome
        .call("Performance.enable", json!({}))
        .expect("Performance.enable");
    if cpu > 1 {
        chrome
            .call("Emulation.setCPUThrottlingRate", json!({ "rate": cpu }))
            .expect("throttle");
    }
    if phone {
        chrome.phone_screen().expect("a phone's screen");
    } else {
        chrome
            .call(
                "Emulation.setDeviceMetricsOverride",
                json!({ "width": 1440, "height": 900, "deviceScaleFactor": 1, "mobile": false }),
            )
            .expect("a wide window");
    }
    chrome
}

const WAIT: Duration = Duration::from_secs(120);
/// How long, in the page's CPU time, a scroll's sections may take to draw once it ends before the
/// run counts those still waiting as skipped.
const SKIPPED: Duration = Duration::from_secs(20);

fn wait(chrome: &mut Chrome, name: &str) -> f64 {
    let expression = format!(
        "window.__bench && window.__bench.seen[{}] !== undefined",
        json!(name)
    );
    if !chrome.wait_for(&expression, WAIT).expect("the wait") {
        let state = chrome.eval(
            "JSON.stringify({ path: location.pathname, bench: !!window.__bench, watch: window.__bench && Object.fromEntries(Object.entries(__bench.watch).map(([k, v]) => [k, String(v)])), seen: window.__bench && __bench.seen, frames: window.__bench && __bench.frames && __bench.frames.length, y: scrollY, height: document.documentElement.scrollHeight, pending: [...document.querySelectorAll('[data-pending=true]')].length, parts: [...document.querySelectorAll('.doc-part > [data-pending=true]')].slice(0, 6).map((e) => e.parentElement.id + '@' + Math.round(e.getBoundingClientRect().top + scrollY) + ':' + e.parentElement.className), errors: window.__bench && __bench.errors, main: document.querySelector('main')?.outerHTML.slice(0, 500), which: [...document.querySelectorAll('[data-pending=true]')].slice(0, 8).map((e) => (e.parentElement?.id || e.parentElement?.className || e.tagName) + '@' + Math.round(e.getBoundingClientRect().top + scrollY)) })",
        );
        panic!("{name} never drew: {state:?}");
    }
    chrome
        .eval(&format!("window.__bench.seen[{}]", json!(name)))
        .expect("seen")
        .as_f64()
        .unwrap_or(f64::NAN)
}

/// Wait until no request but the feed is open, twice a second apart: the page has read what it
/// reads on its own.
fn quiet(chrome: &mut Chrome) -> Vec<Request> {
    let open = |requests: &[Request]| {
        requests
            .iter()
            .filter(|r| {
                !r.finished && r.failed.is_none() && r.kind != "EventSource" && r.kind != "Image"
            })
            .count()
    };
    let mut last = usize::MAX;
    for _ in 0..240 {
        let requests = chrome.requests().expect("requests");
        if open(&requests) == 0 && requests.len() == last {
            return requests;
        }
        last = if open(&requests) == 0 {
            requests.len()
        } else {
            usize::MAX
        };
        chrome.pump(Duration::from_millis(1000)).expect("pump");
    }
    panic!("the page's reads never settled: {:#?}", chrome.requests());
}

/// The ping frame the server sends, `event: ping\ndata: \n\n`, left out as the budget test does.
const PING: f64 = 19.0;

/// Bytes a visit needed, as the client's budget test counts them: every response on the wire
/// (head and body), but pictures, cache hits, idle prefetches and the feed's keep-alive pings.
fn needed(requests: &[Request]) -> f64 {
    requests
        .iter()
        .filter(|r| r.kind != "Image" && !r.from_cache && !r.prefetch)
        .map(|r| r.bytes - r.pings as f64 * PING)
        .sum()
}

fn prefetched(requests: &[Request]) -> f64 {
    requests
        .iter()
        .filter(|r| r.prefetch && !r.from_cache)
        .map(|r| r.bytes)
        .sum()
}

fn listing(requests: &[Request]) -> Value {
    requests
        .iter()
        .filter(|r| !r.from_cache)
        .map(|r| {
            json!({ "url": r.url.split_once("//").map_or(r.url.as_str(), |(_, rest)| rest.split_once('/').map_or(rest, |(_, path)| path)),
                    "kind": r.kind, "status": r.status, "bytes": r.bytes, "prefetch": r.prefetch, "pings": r.pings })
        })
        .collect()
}

fn metrics(chrome: &mut Chrome) -> Value {
    let reply = chrome
        .call("Performance.getMetrics", json!({}))
        .expect("metrics");
    let mut out = serde_json::Map::new();
    for metric in reply["metrics"].as_array().into_iter().flatten() {
        let name = metric["name"].as_str().unwrap_or_default();
        if matches!(
            name,
            "TaskDuration"
                | "ScriptDuration"
                | "LayoutDuration"
                | "RecalcStyleDuration"
                | "JSHeapUsedSize"
                | "Nodes"
        ) {
            out.insert(name.into(), metric["value"].clone());
        }
    }
    Value::Object(out)
}

fn bench(chrome: &mut Chrome) -> Value {
    chrome
        .eval("({ fcp: __bench.fcp, lcp: __bench.lcp, seen: __bench.seen, longtasks: __bench.longtasks, inputs: __bench.inputs, mutations: __bench.mutations, reads: Math.max(0, ...performance.getEntriesByType('resource').filter((e) => e.initiatorType === 'fetch').map((e) => e.responseEnd)), files: performance.getEntriesByType('resource').map((e) => [new URL(e.name).pathname, e.decodedBodySize]).filter(([p]) => /\\.(js|wasm|css)$/.test(p)) })")
        .expect("the page's record")
}

/// The code and style the page itself loaded, its bodies as decoded (no idle prefetch): what the
/// browser parsed and compiled, which the machine's load does not move.
fn code(record: &Value, requests: &[Request]) -> Value {
    let prefetched: Vec<&str> = requests
        .iter()
        .filter(|r| r.prefetch)
        .map(|r| r.url.as_str())
        .collect();
    let mut sums = serde_json::Map::new();
    for file in record["files"].as_array().into_iter().flatten() {
        let path = file[0].as_str().unwrap_or_default();
        if prefetched.iter().any(|url| url.ends_with(path)) {
            continue;
        }
        let kind = path.rsplit('.').next().unwrap_or_default().to_string();
        let size = file[1].as_f64().unwrap_or(0.0);
        let entry = sums.entry(kind).or_insert(json!(0.0));
        *entry = json!(entry.as_f64().unwrap_or(0.0) + size);
    }
    Value::Object(sums)
}

/// One visit to `url`: when it drew first, when it was usable, what it read and the main thread's
/// work, once its reads are done.
fn visit(chrome: &mut Chrome, url: &str) -> Value {
    let before = metrics(chrome);
    let began = Instant::now();
    chrome.navigate(url, WAIT).expect("navigate");
    let usable = wait(chrome, "usable");
    let requests = quiet(chrome);
    let settled = began.elapsed().as_secs_f64() * 1000.0;
    let record = bench(chrome);
    json!({
        "usable": usable, "fcp": record["fcp"], "lcp": record["lcp"], "reads_done": record["reads"], "settled_wall": settled,
        "longtasks": record["longtasks"], "bytes": needed(&requests), "prefetched": prefetched(&requests),
        "code": code(&record, &requests), "mutations": record["mutations"],
        "requests": listing(&requests), "metrics_before": before, "metrics": metrics(chrome),
    })
}

fn load() -> String {
    let out = Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .expect("sysctl");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn emit(args: &Args, client: &str, run: usize, mut record: Value) {
    record["scenario"] = json!(args.scenario);
    record["client"] = json!(client);
    record["cpu"] = json!(args.cpu);
    record["screen"] = json!(if args.phone { "phone" } else { "wide" });
    record["run"] = json!(run);
    record["fixture"] = json!(
        args.fixture
            .as_ref()
            .and_then(|f| f.file_name())
            .map(|f| f.to_string_lossy().to_string())
    );
    record["load"] = json!(load());
    println!("{record}");
}

/// A board change made through the fixture's own board CLI, as an agent makes one.
fn tasks(args: &Args, words: &[&str]) {
    let fixture = args.fixture.as_ref().expect("--fixture DIR");
    let bin = std::env::var("TASKS_BIN").expect("TASKS_BIN");
    let handle = args.handle.as_deref().expect("--handle H");
    let status = Command::new(bin)
        .args(words)
        .args(["--as", handle])
        .current_dir(fixture)
        .stdout(std::process::Stdio::null())
        .status()
        .expect("the board's CLI");
    assert!(status.success(), "tasks {words:?} failed");
}

fn main() {
    if let Some(code) = symphony_chrome::leash_main() {
        std::process::exit(code);
    }
    let args = args();
    let scenario = args.scenario.clone();
    match scenario.as_str() {
        "visit-board" | "visit-task" | "visit-doc" => {
            let (path, usable) = match scenario.as_str() {
                "visit-board" => (BOARD.path, BOARD.usable.to_string()),
                "visit-task" => (TASK.path, args.selectors.task.clone()),
                _ => (DOC.path, args.selectors.doc.clone()),
            };
            for run in 0..args.runs {
                for (client, base) in &args.clients {
                    let mut chrome = chrome(&args, &json!({ "usable": usable }));
                    let url = format!("{base}{path}");
                    let cold = visit(&mut chrome, &url);
                    chrome.navigate("about:blank", WAIT).expect("blank");
                    let warm = visit(&mut chrome, &url);
                    let mut record = json!({ "cold": cold, "warm": warm });
                    // On a phone the task's card is in a column off the first screen, which neither
                    // client reads until it is scrolled to, so there is no card to click.
                    if scenario == "visit-board" && !args.phone {
                        record["second"] = second_page(&mut chrome, &args.selectors.task);
                    }
                    emit(&args, client, run, record);
                }
            }
        }
        "feed" => {
            for run in 0..args.runs {
                for (client, base) in &args.clients {
                    let mut chrome = chrome(&args, &json!({ "usable": BOARD.usable }));
                    chrome
                        .navigate(&format!("{base}{}", BOARD.path), WAIT)
                        .expect("navigate");
                    wait(&mut chrome, "usable");
                    quiet(&mut chrome);
                    chrome.eval("__bench.longtasks = []; true").expect("reset");
                    let mut updates = Vec::new();
                    // A card's priority, p0 to p1 and back, and a card moved between two columns
                    // on the first screen and back.
                    let steps: [(&[&str], &str, &str); 4] = [
                        (
                            &["priority", "ready/task-6", "1"],
                            "priority",
                            r#"[data-slug="ready/task-6"].p1"#,
                        ),
                        (
                            &["priority", "ready/task-6", "0"],
                            "priority",
                            r#"[data-slug="ready/task-6"].p0"#,
                        ),
                        (
                            &["move", "concept/task-101", "design"],
                            "move",
                            r#"[data-status="design"] [data-slug="concept/task-101"]"#,
                        ),
                        (
                            &["move", "concept/task-101", "concept"],
                            "move",
                            r#"[data-status="concept"] [data-slug="concept/task-101"]"#,
                        ),
                    ];
                    for (words, kind, selector) in steps {
                        let name = format!("u{}", updates.len());
                        chrome
                            .eval(&format!(
                                "__bench.expect({}, {})",
                                json!(name),
                                json!(selector)
                            ))
                            .expect("expect");
                        let arrived_before = chrome
                            .eval("__bench.deltas.length")
                            .expect("deltas")
                            .as_u64()
                            .unwrap_or(0);
                        let changed_before = chrome
                            .eval("__bench.mutations")
                            .expect("mutations")
                            .as_f64()
                            .unwrap_or(f64::NAN);
                        tasks(&args, words);
                        let seen = wait(&mut chrome, &name);
                        let arrival = chrome
                            .eval(&format!("__bench.deltas[{arrived_before}]"))
                            .expect("arrival")
                            .as_f64()
                            .unwrap_or(f64::NAN);
                        let changed_drawn = chrome
                            .eval("__bench.mutations")
                            .expect("mutations")
                            .as_f64()
                            .unwrap_or(f64::NAN);
                        // Let the refresh the change starts (the counts, 750 ms on) finish first.
                        chrome.pump(Duration::from_millis(1500)).expect("pump");
                        quiet(&mut chrome);
                        let changed_after = chrome
                            .eval("__bench.mutations")
                            .expect("mutations")
                            .as_f64()
                            .unwrap_or(f64::NAN);
                        updates.push(json!({
                            "kind": kind, "cost": seen - arrival,
                            "mutations_drawn": changed_drawn - changed_before, "mutations": changed_after - changed_before,
                        }));
                    }
                    let record = bench(&mut chrome);
                    emit(
                        &args,
                        client,
                        run,
                        json!({ "updates": updates, "longtasks": record["longtasks"] }),
                    );
                }
            }
        }
        "type" => {
            for run in 0..args.runs {
                for (client, base) in &args.clients {
                    let mut chrome = chrome(&args, &json!({ "usable": args.selectors.task }));
                    chrome
                        .navigate(&format!("{base}{}", TASK.path), WAIT)
                        .expect("navigate");
                    wait(&mut chrome, "usable");
                    quiet(&mut chrome);
                    chrome.eval("(() => { const box = document.querySelector('textarea'); box.scrollIntoView({block: 'center'}); box.focus(); __bench.inputs = []; __bench.longtasks = []; __bench.mutations = 0; })()").expect("focus");
                    let text = "The quick brown fox jumps over the lazy dog.";
                    for ch in text.chars() {
                        let s = ch.to_string();
                        chrome.call("Input.dispatchKeyEvent", json!({ "type": "keyDown", "text": s, "key": s, "unmodifiedText": s })).expect("key");
                        chrome
                            .call(
                                "Input.dispatchKeyEvent",
                                json!({ "type": "keyUp", "key": s }),
                            )
                            .expect("key");
                        chrome.pump(Duration::from_millis(60)).expect("pump");
                    }
                    chrome.pump(Duration::from_millis(500)).expect("pump");
                    let typed = chrome
                        .eval("document.querySelector('textarea').value")
                        .expect("value");
                    let record = bench(&mut chrome);
                    emit(
                        &args,
                        client,
                        run,
                        json!({ "inputs": record["inputs"], "longtasks": record["longtasks"], "typed": typed, "mutations": record["mutations"] }),
                    );
                }
            }
        }
        "scroll" => {
            for run in 0..args.runs {
                for (client, base) in &args.clients {
                    let mut chrome = chrome(&args, &json!({ "usable": args.selectors.doc }));
                    chrome
                        .navigate(&format!("{base}{}", DOC.path), WAIT)
                        .expect("navigate");
                    wait(&mut chrome, "usable");
                    quiet(&mut chrome);
                    chrome.eval("__bench.frames = []; __bench.longtasks = []; __bench.mutations = 0; true").expect("frames");
                    let began = Instant::now();
                    let height = chrome
                        .eval("document.documentElement.scrollHeight")
                        .expect("height")
                        .as_f64()
                        .unwrap_or(0.0);
                    let width = chrome
                        .eval("innerWidth")
                        .expect("width")
                        .as_i64()
                        .unwrap_or(390);
                    chrome
                        .call(
                            "Input.synthesizeScrollGesture",
                            json!({ "x": width / 2, "y": 400, "yDistance": -(height as i64), "speed": 4000, "gestureSourceType": "mouse", "repeatCount": 0 }),
                        )
                        .expect("scroll");
                    let gesture = began.elapsed().as_secs_f64() * 1000.0;
                    chrome.eval("__bench.expect('drawn', '.document-page:not(:has(.doc-part > [data-pending=true]))')").expect("expect");
                    // Both clients latch a section once its sentinel, 600 px around it, is seen
                    // intersecting; a frame long enough to carry the scroll past a section whole
                    // leaves it waiting until the reader scrolls back. Such a run is recorded with
                    // the sections left waiting, not dropped.
                    let expression = "window.__bench && window.__bench.seen.drawn !== undefined";
                    let drawn = if chrome.wait_for(expression, SKIPPED).expect("the wait") {
                        chrome
                            .eval("window.__bench.seen.drawn")
                            .expect("seen")
                            .as_f64()
                    } else {
                        None
                    };
                    let record = chrome
                        .eval("({ frames: __bench.frames, longtasks: __bench.longtasks, mutations: __bench.mutations, heap: performance.memory && performance.memory.usedJSHeapSize, at: scrollY, height: document.documentElement.scrollHeight, now: performance.now(), waiting: document.querySelectorAll('.doc-part > [data-pending=true]').length })")
                        .expect("frames");
                    let frames: Vec<f64> = record["frames"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_f64)
                        .collect();
                    let gaps: Vec<f64> = frames.windows(2).map(|w| w[1] - w[0]).collect();
                    emit(
                        &args,
                        client,
                        run,
                        json!({
                            "gesture_wall": gesture, "frames": frames.len(), "gaps": gaps, "longtasks": record["longtasks"],
                            "drawn_after": drawn.map(|drawn| drawn - frames.first().copied().unwrap_or(drawn)), "left_waiting": record["waiting"],
                            "at": record["at"], "height": record["height"],
                            "mutations": record["mutations"], "metrics": metrics(&mut chrome),
                        }),
                    );
                }
            }
        }
        "load-more" => {
            for run in 0..args.runs {
                for (client, base) in &args.clients {
                    let mut chrome = chrome(&args, &json!({ "usable": BOARD.usable }));
                    let cold = visit(&mut chrome, &format!("{base}{}", BOARD.path));
                    let mut clicks = Vec::new();
                    for click in 0..40 {
                        let count = chrome
                            .eval("document.querySelectorAll('[data-status=\"ready\"] .viewport-slot[id^=\"row-\"]').length")
                            .expect("rows")
                            .as_u64()
                            .unwrap_or(0);
                        let more = chrome
                            .eval("(() => { const b = [...document.querySelectorAll('[data-status=\"ready\"] button')].find(b => b.textContent.includes('Load more')); if (!b) return false; b.scrollIntoView({block: 'center'}); return true; })()")
                            .expect("more");
                        if more != json!(true) {
                            break;
                        }
                        chrome.pump(Duration::from_millis(300)).expect("pump");
                        quiet(&mut chrome);
                        let changed_before = chrome
                            .eval("__bench.mutations")
                            .expect("mutations")
                            .as_f64()
                            .unwrap_or(f64::NAN);
                        let name = format!("m{click}");
                        chrome
                            .eval(&format!(
                                "__bench.expect({}, () => {{ const rows = document.querySelectorAll('[data-status=\"ready\"] [id^=\"row-\"]'); return rows.length > {} && rows[{}].querySelector('.taskcard'); }}); window.__clicked = performance.now(); [...document.querySelectorAll('[data-status=\"ready\"] button')].find(b => b.textContent.includes('Load more')).click(); true",
                                json!(name),
                                count,
                                count
                            ))
                            .expect("click");
                        let seen = wait(&mut chrome, &name);
                        let clicked = chrome
                            .eval("window.__clicked")
                            .expect("clicked")
                            .as_f64()
                            .unwrap_or(f64::NAN);
                        // When the page's answer came: the rest is the client's own work.
                        let answered = chrome
                            .eval("Math.max(0, ...performance.getEntriesByType('resource').filter((e) => e.initiatorType === 'fetch' && e.name.includes('cursor=')).map((e) => e.responseEnd))")
                            .expect("answered")
                            .as_f64()
                            .unwrap_or(f64::NAN);
                        quiet(&mut chrome);
                        let changed_after = chrome
                            .eval("__bench.mutations")
                            .expect("mutations")
                            .as_f64()
                            .unwrap_or(f64::NAN);
                        clicks.push(json!({ "rows_before": count, "ms": seen - clicked, "after_answer": seen - answered, "mutations": changed_after - changed_before }));
                    }
                    let record = bench(&mut chrome);
                    emit(
                        &args,
                        client,
                        run,
                        json!({ "cold": cold, "clicks": clicks, "longtasks": record["longtasks"] }),
                    );
                }
            }
        }
        "memory" => memory(&args),
        other => panic!("unknown scenario {other}"),
    }
}

/// From the board, the task page by a click on its card: how long until it is usable, and the
/// bytes it read that the board had not.
fn second_page(chrome: &mut Chrome, usable: &str) -> Value {
    let before = chrome.requests().expect("requests").len();
    chrome
        .eval(&format!(
            "__bench.expect('second', {}); window.__clicked = performance.now(); document.querySelector('a.taskcard-title[href$=\"/task/bench/task-page\"]').click(); true",
            json!(usable)
        ))
        .expect("click");
    let seen = wait(chrome, "second");
    let clicked = chrome
        .eval("window.__clicked")
        .expect("clicked")
        .as_f64()
        .unwrap_or(f64::NAN);
    let requests = quiet(chrome);
    let new: Vec<Request> = requests.into_iter().skip(before).collect();
    json!({ "usable_after_click": seen - clicked, "bytes": needed(&new), "requests": listing(&new) })
}

/// Every client's board open at once, in a Chrome each, while the board changes every three
/// seconds for `--minutes`: the heap (and the buffers behind it, where a wasm module's memory is)
/// after a collection, and the page's DOM, at the start and at the end.
fn memory(args: &Args) {
    let mut open: Vec<(String, Chrome, Value)> = Vec::new();
    for (client, base) in &args.clients {
        let mut chrome = chrome(args, &json!({ "usable": BOARD.usable }));
        chrome
            .navigate(&format!("{base}{}", BOARD.path), WAIT)
            .expect("navigate");
        wait(&mut chrome, "usable");
        quiet(&mut chrome);
        let start = heap(&mut chrome);
        open.push((client.clone(), chrome, start));
    }
    let began = Instant::now();
    let mut step = 0usize;
    while began.elapsed() < Duration::from_secs(args.minutes * 60) {
        let slug = format!("ready/task-{}", [6, 11, 16, 21][step % 4]);
        match step % 3 {
            0 => tasks(
                args,
                &[
                    "say",
                    &slug,
                    &format!("update {step}: the bench changes the board"),
                ],
            ),
            1 => tasks(args, &["priority", &slug, &((step / 3) % 3).to_string()]),
            _ => tasks(
                args,
                &[
                    "move",
                    "concept/task-101",
                    if step.is_multiple_of(2) {
                        "design"
                    } else {
                        "concept"
                    },
                ],
            ),
        }
        step += 1;
        for (_, chrome, _) in &mut open {
            chrome.pump(Duration::from_millis(1000)).expect("pump");
        }
    }
    for (_, chrome, _) in &mut open {
        chrome.pump(Duration::from_millis(3000)).expect("pump");
    }
    for (client, mut chrome, start) in open {
        let end = heap(&mut chrome);
        let deltas = chrome.eval("__bench.deltas.length").expect("deltas");
        emit(
            args,
            &client,
            0,
            json!({ "start": start, "end": end, "changes": step, "deltas": deltas, "minutes": args.minutes }),
        );
    }
}

fn heap(chrome: &mut Chrome) -> Value {
    chrome
        .call("HeapProfiler.collectGarbage", json!({}))
        .expect("gc");
    let usage = chrome
        .call("Runtime.getHeapUsage", json!({}))
        .expect("heap");
    let dom = chrome
        .call("Memory.getDOMCounters", json!({}))
        .expect("dom");
    json!({ "heap": usage, "dom": dom, "metrics": metrics(chrome), "wasm": chrome.eval("(__bench.memories || []).map((m) => m.buffer.byteLength)").expect("wasm memory") })
}
