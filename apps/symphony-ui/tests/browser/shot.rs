//! `symphony shot` against the stand-in server (`site`): the workspace's own build of the document
//! CLI, run as an agent runs it, with this machine's Chrome. Its pictures are read back here: what
//! they show, their size, and what a failure leaves behind (no picture, no browser, no profile).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{binaries, chrome, site};

/// The workspace's `symphony`, built once for this run.
fn symphony() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| binaries::built("symphony"))
}

/// A folder of this case's own, removed when it ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(case: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "symphony-browser-shot-{case}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// How long these shots give each wait: a shared machine under load can take longer than the
/// default twenty seconds just to draw the board, and these cases test the pictures, not its speed.
/// A case that tests a timeout passes its own `--timeout`, which, coming later, wins.
const PATIENT: &str = "120";

/// `symphony shot ARGS` with `chrome` as its browser and [`PATIENT`] waits.
fn shot(chrome: &str, args: &[&str]) -> Command {
    let mut command = Command::new(symphony());
    command
        .args(["shot", "--timeout", PATIENT])
        .args(args)
        .env("SYMPHONY_CHROME", chrome);
    command
}

fn said(output: &Output) -> String {
    format!(
        "{}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// The JSON lines a run printed, in order.
fn printed(output: &Output) -> Vec<Value> {
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|error| panic!("{line}: {error}")))
        .collect()
}

/// A PNG's pixels, as RGBA: enough of the format to read what Chrome writes (8-bit RGB or RGBA,
/// not interlaced).
struct Picture {
    width: usize,
    height: usize,
    rgba: Vec<[u8; 4]>,
}

impl Picture {
    fn read(path: &Path) -> Picture {
        let bytes =
            std::fs::read(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(
            &bytes[..8],
            b"\x89PNG\r\n\x1a\n",
            "{} is no PNG",
            path.display()
        );
        let (mut width, mut height, mut channels) = (0, 0, 0);
        let mut data = Vec::new();
        let mut at = 8;
        while at + 8 <= bytes.len() {
            let length = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            let kind = &bytes[at + 4..at + 8];
            let body = &bytes[at + 8..at + 8 + length];
            match kind {
                b"IHDR" => {
                    width = u32::from_be_bytes(body[0..4].try_into().unwrap()) as usize;
                    height = u32::from_be_bytes(body[4..8].try_into().unwrap()) as usize;
                    assert_eq!(body[8], 8, "bit depth");
                    channels = match body[9] {
                        2 => 3,
                        6 => 4,
                        other => panic!("colour type {other}"),
                    };
                    assert_eq!(body[12], 0, "interlace");
                }
                b"IDAT" => data.extend_from_slice(body),
                b"IEND" => break,
                _ => {}
            }
            at += 12 + length;
        }
        let mut raw = Vec::new();
        flate2::read::ZlibDecoder::new(data.as_slice())
            .read_to_end(&mut raw)
            .unwrap();
        let stride = width * channels;
        let mut rows: Vec<Vec<u8>> = Vec::with_capacity(height);
        for y in 0..height {
            let line = &raw[y * (stride + 1)..(y + 1) * (stride + 1)];
            let (filter, line) = (line[0], &line[1..]);
            let mut row = vec![0u8; stride];
            for x in 0..stride {
                let left = if x >= channels { row[x - channels] } else { 0 };
                let up = if y > 0 { rows[y - 1][x] } else { 0 };
                let corner = if y > 0 && x >= channels {
                    rows[y - 1][x - channels]
                } else {
                    0
                };
                let predicted = match filter {
                    0 => 0,
                    1 => left,
                    2 => up,
                    3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                    4 => {
                        let p = i16::from(left) + i16::from(up) - i16::from(corner);
                        let (a, b, c) = (
                            (p - i16::from(left)).abs(),
                            (p - i16::from(up)).abs(),
                            (p - i16::from(corner)).abs(),
                        );
                        if a <= b && a <= c {
                            left
                        } else if b <= c {
                            up
                        } else {
                            corner
                        }
                    }
                    other => panic!("filter {other}"),
                };
                row[x] = line[x].wrapping_add(predicted);
            }
            rows.push(row);
        }
        let rgba = rows
            .iter()
            .flat_map(|row| {
                row.chunks(channels)
                    .map(|pixel| [pixel[0], pixel[1], pixel[2], *pixel.get(3).unwrap_or(&255)])
            })
            .collect();
        Picture {
            width,
            height,
            rgba,
        }
    }

    /// The distinct colours in the rectangle `[x, y, width, height]` (CSS pixels at `scale`),
    /// clipped to the picture.
    fn colours(&self, rect: [f64; 4], scale: f64) -> usize {
        let clip = |from: f64, to: f64, most: usize| {
            ((from * scale).max(0.0) as usize).min(most)..((to * scale).max(0.0) as usize).min(most)
        };
        let mut seen = std::collections::BTreeSet::new();
        for y in clip(rect[1], rect[1] + rect[3], self.height) {
            for x in clip(rect[0], rect[0] + rect[2], self.width) {
                seen.insert(self.rgba[y * self.width + x]);
            }
        }
        seen.len()
    }

    /// How many pixels of the rectangle differ between this picture and `other`.
    fn changed(&self, other: &Picture, rect: [f64; 4]) -> usize {
        assert_eq!((self.width, self.height), (other.width, other.height));
        let mut changed = 0;
        for y in (rect[1].max(0.0) as usize)..((rect[1] + rect[3]) as usize).min(self.height) {
            for x in (rect[0].max(0.0) as usize)..((rect[0] + rect[2]) as usize).min(self.width) {
                changed +=
                    usize::from(self.rgba[y * self.width + x] != other.rgba[y * self.width + x]);
            }
        }
        changed
    }
}

fn rect(value: &Value) -> [f64; 4] {
    let numbers: Vec<f64> = value
        .as_array()
        .unwrap_or_else(|| panic!("no rectangle: {value}"))
        .iter()
        .map(|n| n.as_f64().unwrap())
        .collect();
    numbers.try_into().unwrap()
}

/// The board's columns: each one's `data-status` and its heading's rectangle, as
/// `[x, y, width, height]`.
const COLUMNS: &str = "[...document.querySelectorAll('.kanban section.column h2')].map(e => { const b = e.getBoundingClientRect(); return {status: e.closest('section.column').dataset.status, heading: [b.x, b.y, b.width, b.height]}; })";
/// The open status menu's rectangle.
const MENU: &str = "(() => { const b = document.querySelector('[role=listbox][data-state=open]').getBoundingClientRect(); return [b.x, b.y, b.width, b.height]; })()";

#[test]
fn the_tasks_page_is_captured_drawn_with_its_columns_and_a_click_records_the_open_menu() {
    let site = site();
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    let scratch = Scratch::new("menu");
    let (before, open, last) = (
        scratch.join("board.png"),
        scratch.join("open.png"),
        scratch.join("last.png"),
    );
    let output = shot(
        &chrome,
        &[
            &site.url("/tasks"),
            "--out",
            last.to_str().unwrap(),
            "--shot",
            before.to_str().unwrap(),
            "--eval",
            COLUMNS,
            "--click",
            "button[aria-label=\"Status\"]",
            "--shot",
            open.to_str().unwrap(),
            "--eval",
            MENU,
        ],
    )
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", said(&output));
    let lines = printed(&output);
    let [columns, menu] = lines.as_slice() else {
        panic!("two evaluations, one line each: {}", said(&output));
    };
    let board = Picture::read(&before);
    assert_eq!((board.width, board.height), (1440, 900));
    let everything = board.colours([0.0, 0.0, 1440.0, 900.0], 1.0);
    assert!(
        everything > 16,
        "the board is {everything} colour(s): not drawn"
    );
    let columns = columns.as_array().unwrap();
    let statuses: Vec<_> = columns.iter().map(|column| &column["status"]).collect();
    // The stand-in's board holds one ready task, and no machine reports its work: its ready
    // column and the one rail Build and Test are drawn as (docs/MACHINE_WORK.md).
    assert_eq!(
        statuses,
        [&json!("ready"), &json!("build-test")],
        "the board's columns: {columns:?}"
    );
    for column in columns {
        let heading = rect(&column["heading"]);
        assert!(heading[2] > 0.0 && heading[3] > 0.0, "{heading:?}");
        let drawn = board.colours(heading, 1.0);
        assert!(
            drawn > 1,
            "the column heading at {heading:?} is one flat colour in the picture"
        );
    }
    let menu = rect(menu);
    let opened = Picture::read(&open);
    let changed = opened.changed(&board, menu);
    let area = (menu[2] * menu[3]) as usize;
    assert!(
        changed * 10 > area,
        "only {changed} of the menu's {area} pixels differ from the board before the click"
    );
    assert!(
        opened.colours(menu, 1.0) > 2,
        "the open menu shows no options"
    );
    // The final capture follows the last step: the menu is still open in it.
    assert_eq!(
        Picture::read(&last).changed(&opened, [0.0, 0.0, 1440.0, 900.0]),
        0
    );
}

#[test]
fn a_phone_shot_is_a_phone_screen_at_three_times_and_the_report_says_what_the_page_did() {
    let site = site();
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    let scratch = Scratch::new("phone");
    let phone = scratch.join("nested/folder/phone.png");
    let output = shot(
        &chrome,
        &[
            &site.url("/tasks"),
            "--out",
            phone.to_str().unwrap(),
            "--phone",
            "--scheme",
            "dark",
            "--report",
            "--eval",
            "[innerWidth, devicePixelRatio, matchMedia('(prefers-color-scheme: dark)').matches]",
            "--type",
            "input[type=search]=Example",
            "--key",
            "Shift+Tab",
            "--eval",
            "[document.querySelector('input[type=search]').value, document.activeElement.matches('input[type=search]')]",
        ],
    )
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", said(&output));
    let lines = printed(&output);
    let [screen, typed, report] = lines.as_slice() else {
        panic!("two evaluations and a report: {}", said(&output));
    };
    assert_eq!(screen, &json!([390, 3, true]));
    // Typed into the search field, then Shift+Tab moved the focus back out of it.
    assert_eq!(typed, &json!(["Example", false]));
    let picture = Picture::read(&phone);
    assert_eq!((picture.width, picture.height), (390 * 3, 844 * 3));
    assert!(picture.colours([0.0, 0.0, 390.0, 844.0], 3.0) > 16);
    assert!(report["console_errors"].is_array(), "{report}");
    assert!(report["failed_requests"].is_array(), "{report}");
    assert_eq!(report["viewport_width"], json!(390), "{report}");
    let scroll = report["scroll_width"].as_f64().unwrap();
    assert_eq!(
        report["horizontal_overflow"],
        json!(scroll > 390.0),
        "{report}"
    );
}

#[test]
fn a_full_page_shot_takes_the_whole_document() {
    let site = site();
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    let scratch = Scratch::new("full");
    let full = scratch.join("full.png");
    let output = shot(
        &chrome,
        &[
            &site.url("/tasks"),
            "--out",
            full.to_str().unwrap(),
            "--size",
            "800x300",
            "--full-page",
            "--eval",
            "[document.documentElement.scrollWidth, document.documentElement.scrollHeight]",
        ],
    )
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", said(&output));
    let lines = printed(&output);
    let size = lines[0].as_array().unwrap();
    let height = size[1].as_f64().unwrap();
    assert!(height > 300.0, "the fixture board fits 300 px: {size:?}");
    let picture = Picture::read(&full);
    let width = size[0].as_f64().unwrap();
    assert!(
        (picture.width as f64 - width).abs() <= 1.0,
        "a page {width} px wide (the window less its scrollbar) captured {} px wide",
        picture.width
    );
    assert!(
        (picture.height as f64 - height).abs() <= 1.0,
        "a full page of {height} px captured {} px tall",
        picture.height
    );
}

#[test]
fn an_escape_the_page_does_not_take_leaves_it_drawn_and_in_front_and_a_wait_scrolls_nothing() {
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    // A page with no key handler of its own, so the Escape goes to the browser, which then counts
    // the page hidden and draws it no frames until it is brought back in front; and an element
    // below the fold, which waiting for leaves where it is.
    let site = site();
    site.set_file(
        "/escape.html",
        b"<!doctype html><main style=\"height: 3000px\">drawn</main><p id=\"below\">end</p>"
            .to_vec(),
        "text/html",
    );
    let scratch = Scratch::new("escape");
    let out = scratch.join("escape.png");
    let output = shot(
        &chrome,
        &[
            &site.url("/escape.html"),
            "--out",
            out.to_str().unwrap(),
            "--wait-for",
            "#below",
            "--eval",
            "scrollY",
            "--key",
            "Escape",
            "--eval",
            "document.visibilityState",
            "--key",
            "Escape",
        ],
    )
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", said(&output));
    assert_eq!(printed(&output), [json!(0), json!("visible")]);
    assert_eq!(Picture::read(&out).width, 1440);
}

/// A page whose click target, a folded list's summary far below the fold, moves down 240 px on the
/// frames after it is scrolled into view, with the page's scroll position unchanged: an element
/// above it grows on the frame after the first scroll (+60), and three placeholders above it are
/// filled once they come within 600 px of the view (+60 each), as the client's `NearViewport` fills
/// its own. Every pointer event is recorded with the element it reached, and the summary's top as
/// the first scroll left it.
const SETTLING: &str = r#"<!doctype html>
<meta charset="utf-8">
<style>
  body { margin: 0; font: 16px/1.4 sans-serif; }
  .filler { height: 2400px; }
  #grows { height: 0; }
  .slot { height: 20px; }
  .slot.mounted { height: 80px; }
</style>
<main>
  <div class="filler">top</div>
  <div id="grows"></div>
  <div class="slot"></div><div class="slot"></div><div class="slot"></div>
  <details id="finished"><summary>3 finished</summary><p>one, two, three</p></details>
  <div class="filler">bottom</div>
</main>
<script>
  const summary = document.querySelector('summary');
  window.landed = [];
  for (const kind of ['pointerdown', 'pointerup', 'click']) {
    addEventListener(kind, event => landed.push([kind, event.target.tagName]), true);
  }
  addEventListener('scroll', () => {
    window.scrolledTo = summary.getBoundingClientRect().top;
    requestAnimationFrame(() => { document.getElementById('grows').style.height = '60px'; });
  }, {once: true});
  const near = new IntersectionObserver(entries => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      entry.target.classList.add('mounted');
      entry.target.textContent = 'a row';
      near.unobserve(entry.target);
    }
  }, {rootMargin: '600px'});
  for (const slot of document.querySelectorAll('.slot')) near.observe(slot);
</script>"#;

#[test]
fn a_click_aims_at_its_target_once_what_the_scroll_mounted_above_it_has_moved_it() {
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    let site = site();
    site.set_file("/settling.html", SETTLING.as_bytes().to_vec(), "text/html");
    let scratch = Scratch::new("settling");
    let out = scratch.join("settling.png");
    let output = shot(
        &chrome,
        &[
            &site.url("/settling.html"),
            "--out",
            out.to_str().unwrap(),
            "--click",
            "#finished summary",
            "--eval",
            "[document.getElementById('finished').open, landed, summary.getBoundingClientRect().top - scrolledTo, document.querySelectorAll('.slot.mounted').length]",
        ],
    )
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", said(&output));
    let lines = printed(&output);
    let [after] = lines.as_slice() else {
        panic!("one evaluation: {}", said(&output));
    };
    // The summary moved 240 px after the scroll, every placeholder filled: a click at the middle
    // the scroll first left it at lands on what grew above it.
    assert_eq!(after[2], json!(240), "{after}");
    assert_eq!(after[3], json!(3), "{after}");
    // The press, the release and so the click all reached the summary, which opened its list.
    assert_eq!(
        after[1],
        json!([
            ["pointerdown", "SUMMARY"],
            ["pointerup", "SUMMARY"],
            ["click", "SUMMARY"]
        ]),
        "{after}"
    );
    assert_eq!(after[0], json!(true), "{after}");
}

#[test]
fn a_click_on_a_target_that_never_holds_still_fails_naming_it_and_presses_nothing() {
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    // A button that moves down a pixel on every frame, its middle always in view (a cycle of 400
    // frames, longer than the five seconds the click waits), on a page that asks for `/pressed` on
    // any press anywhere.
    let site = site();
    site.set_file(
        "/runaway.html",
        br#"<!doctype html>
<style>body { margin: 0; } #runaway { position: relative; width: 200px; height: 40px; }</style>
<main style="height: 3000px"><button id="runaway">run</button></main>
<script>
  addEventListener('pointerdown', () => fetch('/pressed'), true);
  const runaway = document.getElementById('runaway');
  let frame = 0;
  requestAnimationFrame(function step() {
    runaway.style.top = (frame++ % 400) + 'px';
    requestAnimationFrame(step);
  });
</script>"#
            .to_vec(),
        "text/html",
    );
    let scratch = Scratch::new("runaway");
    let out = scratch.join("runaway.png");
    let output = shot(
        &chrome,
        &[
            &site.url("/runaway.html"),
            "--out",
            out.to_str().unwrap(),
            "--timeout",
            "5",
            "--click",
            "#runaway",
        ],
    )
    .output()
    .unwrap();
    assert_eq!(output.status.code(), Some(1), "{}", said(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.trim_end(),
        "symphony shot: --click #runaway kept moving: no two reads in a row, each in an animation \
         frame, found its box in one place within 5s",
        "{}",
        said(&output)
    );
    assert!(
        !site.requests().iter().any(|path| path == "/pressed"),
        "the shot pressed somewhere: {:?}",
        site.requests()
    );
    assert!(!out.exists(), "a failed shot left a picture");
}

/// What a page that records where the pointer lands keeps: each press, release and click with
/// the id of the element it reached (`landed`), what is on top at the target's middle (`on_top()`),
/// and, once every row above the target has mounted, what was on top there a frame later
/// (`pushed`): where an aim that trusted a box held still in the viewport would press.
const LANDING: &str = r#"
  const target = document.getElementById('target');
  const on_top = () => { const b = target.getBoundingClientRect(); const at = document.elementFromPoint(b.x + b.width / 2, b.y + b.height / 2); return at && at.id; };
  window.landed = [];
  for (const kind of ['pointerdown', 'pointerup', 'click']) {
    addEventListener(kind, event => landed.push([kind, event.target.id]), true);
  }
  const rows = document.querySelectorAll('.slot');
  const near = new IntersectionObserver(entries => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      entry.target.classList.add('mounted');
      near.unobserve(entry.target);
    }
    if (document.querySelectorAll('.slot.mounted').length === rows.length && !('pushed' in window)) {
      window.pushed = undefined;
      requestAnimationFrame(() => { window.pushed = on_top(); });
    }
  }, {rootMargin: '600px'});
  for (const row of rows) near.observe(row);
"#;

/// A page whose click target sits in a scrolling container, below three placeholders that grow by
/// 120 px each once they come into the container's view, as a `NearViewport` fills its own: the
/// scroll that brings the target to the container's middle mounts them, pushing it past the
/// container's lower edge while its middle stays in the window's view, over `#below`.
fn contained() -> String {
    format!(
        r#"<!doctype html>
<meta charset="utf-8">
<style>
  body {{ margin: 0; font: 16px/1.4 sans-serif; }}
  #top {{ height: 100px; }}
  #box {{ height: 300px; overflow: auto; }}
  .filler {{ height: 1200px; }}
  .slot {{ height: 20px; }}
  .slot.mounted {{ height: 140px; }}
  #below {{ height: 2000px; }}
</style>
<div id="top">top</div>
<div id="box">
  <div class="filler">inner top</div>
  <div class="slot"></div><div class="slot"></div><div class="slot"></div>
  <button id="target">target</button>
  <div class="filler">inner bottom</div>
</div>
<div id="below">below the container</div>
<script>{LANDING}</script>"#
    )
}

/// A page whose click target sits below a sticky header 120 px tall and four placeholders that
/// shrink by 90 px each once they come within 600 px of the view: the scroll that brings the
/// target to the middle of the view mounts them, pulling it up 360 px, under the header.
fn sticky() -> String {
    format!(
        r#"<!doctype html>
<meta charset="utf-8">
<style>
  body {{ margin: 0; font: 16px/1.4 sans-serif; }}
  header {{ position: sticky; top: 0; height: 120px; background: #ccc; z-index: 1; }}
  .filler {{ height: 2400px; }}
  .slot {{ height: 130px; }}
  .slot.mounted {{ height: 40px; }}
</style>
<header id="header">sticky header</header>
<div class="filler">top</div>
<div class="slot"></div><div class="slot"></div><div class="slot"></div><div class="slot"></div>
<button id="target">target</button>
<div class="filler">bottom</div>
<script>{LANDING}</script>"#
    )
}

/// `--click #target` on `page`, then where the pointer landed, as [`LANDING`] keeps it:
/// `[landed, pushed, rows mounted, on_top()]`.
fn click_landing(name: &str, page: String) -> Value {
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return Value::Null;
    };
    let site = site();
    let path = format!("/{name}.html");
    site.set_file(&path, page.into_bytes(), "text/html");
    let scratch = Scratch::new(name);
    let out = scratch.join(format!("{name}.png").as_str());
    let output = shot(
        &chrome,
        &[
            &site.url(&path),
            "--out",
            out.to_str().unwrap(),
            "--click",
            "#target",
            "--eval",
            "[landed, pushed, document.querySelectorAll('.slot.mounted').length, on_top()]",
        ],
    )
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", said(&output));
    assert_eq!(Picture::read(&out).width, 1440);
    let lines = printed(&output);
    let [after] = lines.as_slice() else {
        panic!("one evaluation: {}", said(&output));
    };
    after.clone()
}

/// The press, the release and the click, each on the target.
fn on_the_target() -> Value {
    json!([
        ["pointerdown", "target"],
        ["pointerup", "target"],
        ["click", "target"]
    ])
}

#[test]
fn a_click_on_a_target_pushed_out_of_its_scrolling_container_scrolls_it_back_and_lands_on_it() {
    let after = click_landing("contained", contained());
    if after.is_null() {
        return;
    }
    // Every placeholder mounted, and once they had, what was on top at the target's middle, held
    // still in the window's view, was the page below the container.
    assert_eq!(after[2], json!(3), "{after}");
    assert_eq!(after[1], json!("below"), "{after}");
    // Scrolled again within its container, the target took the press, the release and the click.
    assert_eq!(after[0], on_the_target(), "{after}");
    assert_eq!(after[3], json!("target"), "{after}");
}

#[test]
fn a_click_on_a_target_pushed_under_a_sticky_header_scrolls_it_clear_and_lands_on_it() {
    let after = click_landing("sticky", sticky());
    if after.is_null() {
        return;
    }
    assert_eq!(after[2], json!(4), "{after}");
    assert_eq!(after[1], json!("header"), "{after}");
    assert_eq!(after[0], on_the_target(), "{after}");
    assert_eq!(after[3], json!("target"), "{after}");
}

/// A page whose button and text field lie under a fixed veil that never lifts, a bare `div` in a
/// `section` with an id and four classes, and whose other button is fixed above the top of the
/// view, where no scroll brings it. Any pointer movement or press asks for `/pointed`.
const VEILED: &[u8] = br#"<!doctype html>
<meta charset="utf-8">
<style>
  body { margin: 0; font: 16px/1.4 sans-serif; }
  main { height: 3000px; }
  #veil > div { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.4); }
  #away { position: fixed; top: -200px; }
</style>
<main><button id="target">target</button> <input id="field"> <button id="away">away</button></main>
<section id="veil" class="modal dim backdrop open"><div></div></section>
<script>
  for (const kind of ['pointermove', 'pointerdown']) addEventListener(kind, () => fetch('/pointed'), true);
</script>"#;

/// `symphony shot` on [`VEILED`] with `--timeout 5` and `steps`.
fn veiled(name: &str, steps: &[&str]) -> Option<(Output, PathBuf, Vec<String>, Scratch)> {
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return None;
    };
    let site = site();
    site.set_file("/veiled.html", VEILED.to_vec(), "text/html");
    let scratch = Scratch::new(name);
    let out = scratch.join(format!("{name}.png").as_str());
    let output = shot(
        &chrome,
        &[
            &[
                site.url("/veiled.html").as_str(),
                "--out",
                out.to_str().unwrap(),
                "--timeout",
                "5",
            ][..],
            steps,
        ]
        .concat(),
    )
    .output()
    .unwrap();
    Some((output, out, site.requests(), scratch))
}

#[test]
fn a_click_or_a_hover_on_a_target_that_stays_covered_fails_naming_the_cover_and_typing_reaches_it()
{
    for flag in ["--click", "--hover"] {
        let Some((output, out, requests, _scratch)) =
            veiled(&format!("veiled{flag}"), &[flag, "#target"])
        else {
            return;
        };
        assert_eq!(output.status.code(), Some(1), "{flag}: {}", said(&output));
        assert_eq!(
            String::from_utf8_lossy(&output.stderr).trim_end(),
            format!(
                "symphony shot: {flag} #target stayed covered: section#veil.modal.dim.backdrop > \
                 div was on top at its middle after every scroll within 5s"
            ),
            "{}",
            said(&output)
        );
        assert!(
            !requests.iter().any(|path| path == "/pointed"),
            "{flag} moved the pointer or pressed: {requests:?}"
        );
        assert!(!out.exists(), "{flag}: a failed shot left a picture");
    }
    // Typing takes no aim: it focuses the field by script, and the veil takes none of the text.
    let Some((output, out, requests, _scratch)) = veiled(
        "veiled-type",
        &[
            "--type",
            "#field=under the veil",
            "--eval",
            "document.getElementById('field').value",
        ],
    ) else {
        return;
    };
    assert!(output.status.success(), "{}", said(&output));
    assert_eq!(printed(&output), [json!("under the veil")]);
    assert!(
        !requests.iter().any(|path| path == "/pointed"),
        "{requests:?}"
    );
    assert_eq!(Picture::read(&out).width, 1440);
}

#[test]
fn a_target_that_stays_out_of_view_fails_naming_it_not_the_settle_after_its_last_scroll() {
    // Each scroll of the fixed button changes nothing, and the page settles after each one, so the
    // wait most often runs out in a settle: the failure still says what the button's last read found.
    let Some((output, out, requests, _scratch)) = veiled("away", &["--click", "#away"]) else {
        return;
    };
    assert_eq!(output.status.code(), Some(1), "{}", said(&output));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim_end(),
        "symphony shot: --click #away stayed out of view: its middle was outside the viewport \
         after every scroll within 5s",
        "{}",
        said(&output)
    );
    assert!(
        !requests.iter().any(|path| path == "/pointed"),
        "{requests:?}"
    );
    assert!(!out.exists(), "a failed shot left a picture");
}

/// A stand-in for Chrome that records its process id and arguments in `SHOT_RECORDS`, one file
/// per launch named by its id, then becomes the real Chrome (`SHOT_REAL_CHROME`) in place.
fn recording_chrome(scratch: &Scratch) -> (PathBuf, PathBuf) {
    let records = scratch.join("records");
    std::fs::create_dir(&records).unwrap();
    let wrapper = scratch.join("chrome");
    std::fs::write(
        &wrapper,
        "#!/bin/sh\nprintf '%s\\n' \"$$\" \"$@\" > \"$SHOT_RECORDS/$$.part\"\nmv \"$SHOT_RECORDS/$$.part\" \"$SHOT_RECORDS/$$\"\nexec \"$SHOT_REAL_CHROME\" \"$@\"\n",
    )
    .unwrap();
    std::fs::set_permissions(
        &wrapper,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    (wrapper, records)
}

/// The launches recorded so far: each browser's process id and profile.
fn launches(records: &Path) -> Vec<(u32, PathBuf)> {
    let mut found: Vec<(u32, PathBuf)> = std::fs::read_dir(records)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_none())
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            let mut lines = text.lines();
            let pid = lines.next().unwrap().parse().unwrap();
            let profile = lines
                .find_map(|line| line.strip_prefix("--user-data-dir="))
                .unwrap_or_else(|| panic!("no profile in {text}"));
            (pid, PathBuf::from(profile))
        })
        .collect();
    found.sort();
    found
}

/// The recorded browser's whole process group is gone, and its profile with it.
fn assert_gone(pid: u32, profile: &Path) {
    // SAFETY: signal zero only asks whether the group the recorded browser led still exists.
    let alive = unsafe { libc::kill(-libc::pid_t::try_from(pid).unwrap(), 0) } == 0;
    assert!(!alive, "the browser {pid}'s group outlived its shot");
    assert!(!profile.exists(), "{} outlived its shot", profile.display());
}

fn shot_with(wrapper: &Path, records: &Path, chrome: &str, args: &[&str]) -> Command {
    let mut command = shot(wrapper.to_str().unwrap(), args);
    command
        .env("SHOT_RECORDS", records)
        .env("SHOT_REAL_CHROME", chrome);
    command
}

#[test]
fn a_selector_that_never_appears_fails_within_the_timeout_naming_it_and_leaves_no_browser() {
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    // A page with nothing to load, so that, of the five seconds each wait has, the selector's
    // wait is the one that runs out however busy the machine (a board page on a loaded one can take
    // them just to draw); timed from the page's request, as how long the browser takes to start is
    // no part of the shot's waits.
    let site = site();
    site.set_file(
        "/still.html",
        b"<!doctype html><main>drawn</main>".to_vec(),
        "text/html",
    );
    let scratch = Scratch::new("never");
    let (wrapper, records) = recording_chrome(&scratch);
    let out = scratch.join("never.png");
    let mut running = shot_with(
        &wrapper,
        &records,
        &chrome,
        &[
            &site.url("/still.html"),
            "--out",
            out.to_str().unwrap(),
            "--timeout",
            "5",
            "--wait-for",
            "#never-drawn",
        ],
    )
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
    let asked = loop {
        if site.requests().iter().any(|path| path == "/still.html") {
            break Instant::now();
        }
        if running.try_wait().unwrap().is_some() {
            let output = running.wait_with_output().unwrap();
            panic!(
                "the shot ended before it asked for its page: {}",
                said(&output)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let output = running.wait_with_output().unwrap();
    let took = asked.elapsed();
    assert_eq!(output.status.code(), Some(1), "{}", said(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--wait-for #never-drawn did not appear within 5s"),
        "{}",
        said(&output)
    );
    // The wait ran its five seconds out rather than failing early, and they, neither the default
    // twenty nor the helper's patient waits, are what it waited; how long a loaded machine then
    // takes to stop its browser is no part of the wait.
    assert!(
        took >= Duration::from_secs(5),
        "failed {took:?} after asking for its page, before its timeout"
    );
    // Nor did it run on past them: the page's load and the selector's wait, five seconds each,
    // every call to the browser within them included, and its browser's stop.
    assert!(
        took < Duration::from_secs(40),
        "failed {took:?} after asking for its page, long after its two five-second waits"
    );
    assert!(!out.exists(), "a failed shot left a picture");
    let launched = launches(&records);
    assert_eq!(launched.len(), 1, "{launched:?}");
    for (pid, profile) in &launched {
        assert_gone(*pid, profile);
    }
}

#[test]
fn a_page_that_refuses_the_connection_fails_cleanly() {
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    let refused = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}/tasks", listener.local_addr().unwrap())
    };
    let scratch = Scratch::new("refused");
    let (wrapper, records) = recording_chrome(&scratch);
    let out = scratch.join("refused.png");
    let output = shot_with(
        &wrapper,
        &records,
        &chrome,
        &[&refused, "--out", out.to_str().unwrap()],
    )
    .output()
    .unwrap();
    assert_eq!(output.status.code(), Some(1), "{}", said(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with(&format!("symphony shot: {refused} did not load: ")),
        "{}",
        said(&output)
    );
    assert!(
        stderr.contains("ERR_CONNECTION_REFUSED"),
        "{}",
        said(&output)
    );
    assert_eq!(
        stderr.matches(&refused).count(),
        1,
        "the URL named once: {stderr}"
    );
    assert_eq!(stderr.lines().count(), 1, "one line says why: {stderr}");
    assert!(output.stdout.is_empty());
    assert!(!out.exists(), "a failed shot left a picture");
    for (pid, profile) in &launches(&records) {
        assert_gone(*pid, profile);
    }
}

#[test]
fn two_shots_at_once_each_have_a_profile_and_a_port_of_their_own() {
    let site = site();
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    let scratch = Scratch::new("parallel");
    let (wrapper, records) = recording_chrome(&scratch);
    let url = site.url("/tasks");
    let outs = [scratch.join("one.png"), scratch.join("two.png")];
    struct Running(std::process::Child);
    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    // Each holds its browser open for a while once its page has drawn, so both are seen at once.
    let mut runs: Vec<Running> = outs
        .iter()
        .map(|out| {
            Running(
                shot_with(
                    &wrapper,
                    &records,
                    &chrome,
                    &[&url, "--out", out.to_str().unwrap(), "--pause", "4000"],
                )
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
            )
        })
        .collect();
    let deadline = Instant::now() + Duration::from_secs(60);
    let ports = loop {
        let launched = launches(&records);
        let ports: Vec<Option<String>> = launched
            .iter()
            .map(|(_, profile)| {
                std::fs::read_to_string(profile.join("DevToolsActivePort"))
                    .ok()
                    .filter(|text| text.lines().count() == 2)
                    .and_then(|text| text.lines().next().map(str::to_string))
            })
            .collect();
        if launched.len() == 2 && ports.iter().all(Option::is_some) {
            // SAFETY: signal zero only asks whether each recorded browser is still running.
            let both = launched.iter().all(
                |(pid, _)| unsafe { libc::kill(libc::pid_t::try_from(*pid).unwrap(), 0) } == 0,
            );
            assert!(
                both,
                "the two browsers never ran at the same time: {launched:?}"
            );
            break ports.into_iter().map(Option::unwrap).collect::<Vec<_>>();
        }
        assert!(
            Instant::now() < deadline,
            "two browsers never started: {launched:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    let launched = launches(&records);
    assert_ne!(
        launched[0].1, launched[1].1,
        "the two shots shared a profile"
    );
    assert_ne!(ports[0], ports[1], "the two shots shared a DevTools port");
    for run in &mut runs {
        let mut stderr = String::new();
        run.0
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        let status = run.0.wait().unwrap();
        assert!(status.success(), "{status}: {stderr}");
    }
    for out in &outs {
        assert_eq!(Picture::read(out).width, 1440);
    }
    for (pid, profile) in &launched {
        assert_gone(*pid, profile);
    }
}
