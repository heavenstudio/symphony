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

/// `symphony shot` on `page`, served by a stand-in of its own, with `steps`: what it printed, its
/// picture's path, the paths the page asked for, and the folder that holds the picture.
fn on_page(
    name: &str,
    page: &[u8],
    steps: &[&str],
) -> Option<(Output, PathBuf, Vec<String>, Scratch)> {
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return None;
    };
    let site = site();
    let path = format!("/{name}.html");
    site.set_file(&path, page.to_vec(), "text/html");
    let scratch = Scratch::new(name);
    let out = scratch.join(format!("{name}.png").as_str());
    let output = shot(
        &chrome,
        &[
            &[site.url(&path).as_str(), "--out", out.to_str().unwrap()][..],
            steps,
        ]
        .concat(),
    )
    .output()
    .unwrap();
    Some((output, out, site.requests(), scratch))
}

/// `symphony shot` on [`VEILED`] with `--timeout 5` and `steps`.
fn veiled(name: &str, steps: &[&str]) -> Option<(Output, PathBuf, Vec<String>, Scratch)> {
    on_page(name, VEILED, &[&["--timeout", "5"][..], steps].concat())
}

/// `symphony shot` on `page` with `--timeout 5` and `flag CSS` fails with `message` after
/// `symphony shot: `, having moved no pointer and pressed nothing (the page asks for `/pointed`
/// on either) and left no picture. `false` when this machine has no Chrome.
fn refused(name: &str, page: &[u8], flag: &str, css: &str, message: &str) -> bool {
    let Some((output, out, requests, _scratch)) =
        on_page(name, page, &["--timeout", "5", flag, css])
    else {
        return false;
    };
    assert_eq!(
        output.status.code(),
        Some(1),
        "{flag} {css}: {}",
        said(&output)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim_end(),
        format!("symphony shot: {message}"),
        "{}",
        said(&output)
    );
    assert!(
        !requests.iter().any(|path| path == "/pointed"),
        "{flag} {css} moved the pointer or pressed: {requests:?}"
    );
    assert!(!out.exists(), "{flag} {css}: a failed shot left a picture");
    true
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

/// Records each press and click with the id of the element it reached (`landed`), and asks for
/// `/pointed` on any pointer movement or press.
const POINTED: &str = r#"<script>
  window.landed = [];
  for (const kind of ['pointerdown', 'click']) addEventListener(kind, event => landed.push([kind, event.target.id]), true);
  for (const kind of ['pointermove', 'pointerdown']) addEventListener(kind, () => fetch('/pointed'), true);
</script>"#;

/// Checkboxes under labels: one hidden in its label as screen-reader-only inputs are, one under
/// its label's styled track, one under a label that names it by `for`; then one under a label that
/// names another, and one under a link inside its own label, whose press follows the link. Then,
/// each hidden in its label: one under a button in a component's shadow root, one under an object,
/// and a second checkbox over its label's text, the label's control being the first; and an icon
/// with `pointer-events: none` in a label that names another checkbox. Then, each hidden in its
/// label: one under a span slotted into a component's shadow button, one under a closed shadow
/// root's button, and one under a closed shadow root of decoration only; and an icon with
/// `pointer-events: none` slotted into a component's shadow label, whose control is a checkbox in
/// that shadow root. Then, each hidden in its label: one under a button in a closed shadow root on
/// a plain span, and one under a closed shadow root of decoration only on a plain span.
fn labels() -> Vec<u8> {
    format!(
        r##"<!doctype html>
<meta charset="utf-8">
<style>
  body {{ margin: 0; padding: 20px; font: 16px/1.4 sans-serif; }}
  p {{ position: relative; margin: 0 0 40px; }}
  .toggle {{ position: relative; display: inline-flex; gap: 8px; align-items: center; }}
  .track {{ display: inline-block; width: 40px; height: 24px; background: #ccc; }}
  .hidden {{ position: absolute; left: 12px; top: 8px; width: 1px; height: 1px; margin: 0; padding: 0; border: 0; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; }}
  #under {{ position: absolute; left: 0; top: 0; margin: 0; z-index: -1; }}
  .over {{ position: absolute; inset: 0; background: #eef; }}
</style>
<p><label class="toggle"><input type="checkbox" id="hidden" class="hidden"><span class="track" id="hidden-track"></span> hidden in its label</label></p>
<p><label class="toggle"><input type="checkbox" id="under"><span class="track" id="under-track"></span> under its label's track</label></p>
<p><input type="checkbox" id="named"><label for="named" id="named-label" class="over">over it, naming it</label></p>
<p><input type="checkbox" id="unnamed"><label for="named" id="another" class="over">over it, naming another</label></p>
<p><label class="toggle"><input type="checkbox" id="linked"><a href="#terms" id="terms" class="over">the terms, over it</a></label></p>
<p><label class="toggle"><input type="checkbox" id="shadowed" class="hidden"><x-press id="press"></x-press> a button in a shadow root, over it</label></p>
<p><label class="toggle"><input type="checkbox" id="objected" class="hidden"><object id="object" class="track" type="text/plain"></object> an object, over it</label></p>
<p><label class="toggle" id="pair"><input type="checkbox" id="first"> its label's text over it<input type="checkbox" id="second" class="hidden" style="left: 70px"></label></p>
<p><input type="checkbox" id="other"> <label for="other" id="other-label"><svg id="mark" width="24" height="24" style="pointer-events: none"><rect width="24" height="24" fill="#08f"/></svg> a mark in another's label</label></p>
<p><label class="toggle"><input type="checkbox" id="slotted" class="hidden"><x-wrap id="wrap"><span id="slotted-span" class="track"></span></x-wrap> a span slotted into a shadow button, over it</label></p>
<p><label class="toggle"><input type="checkbox" id="closed" class="hidden"><x-closed id="closed-host"></x-closed> a closed root's button, over it</label></p>
<p><label class="toggle"><input type="checkbox" id="adorned" class="hidden"><x-closed-deco id="deco-host"></x-closed-deco> a closed root's decoration, over it</label></p>
<p><x-check id="check-host"><svg id="checkmark" width="24" height="24" style="pointer-events: none"><rect width="24" height="24" fill="#a0a"/></svg></x-check> a mark in a component's label</p>
<p><label class="toggle"><input type="checkbox" id="spanned" class="hidden"><span id="span-host"></span> a closed root's button on a span, over it</label></p>
<p><label class="toggle"><input type="checkbox" id="plain" class="hidden"><span id="plain-host"></span> a closed root's decoration on a span, over it</label></p>
<script>
  const component = (name, mode, html) => customElements.define(name, class extends HTMLElement {{
    constructor() {{ super(); this.attachShadow({{mode}}).innerHTML = html; }}
  }});
  const track = 'display: block; width: 40px; height: 24px';
  component('x-press', 'open', `<button type="button" id="inner" style="${{track}}"></button>`);
  component('x-wrap', 'open', '<button type="button" id="wrap-button" style="padding: 0; border: 0"><slot></slot></button>');
  component('x-closed', 'closed', `<button type="button" id="closed-button" style="${{track}}"></button>`);
  component('x-closed-deco', 'closed', `<span style="${{track}}; background: #bbb"></span>`);
  component('x-check', 'open', '<label id="shadow-label"><input type="checkbox" id="shadow-check"><slot></slot></label>');
  document.getElementById('span-host').attachShadow({{mode: 'closed'}}).innerHTML = `<button type="button" style="${{track}}"></button>`;
  document.getElementById('plain-host').attachShadow({{mode: 'closed'}}).innerHTML = `<span style="${{track}}; background: #bbb"></span>`;
</script>
{POINTED}"##
    )
    .into_bytes()
}

#[test]
fn a_click_on_a_checkbox_its_own_label_covers_lands_on_the_label_and_checks_it() {
    let Some((output, out, _requests, _scratch)) = on_page(
        "labels",
        &labels(),
        &[
            "--click",
            "#hidden",
            "--click",
            "#under",
            "--click",
            "#named",
            "--click",
            "#plain",
            "--eval",
            "[landed, ['hidden', 'under', 'named', 'unnamed', 'linked', 'plain'].map(id => document.getElementById(id).checked)]",
        ],
    ) else {
        return;
    };
    assert!(output.status.success(), "{}", said(&output));
    let lines = printed(&output);
    let [after] = lines.as_slice() else {
        panic!("one evaluation: {}", said(&output));
    };
    // Each press landed on the label, or on its track, and the label clicked its control. The
    // last, on a closed root's decoration, the browser's own hit confirmed: no control there.
    assert_eq!(
        after[0],
        json!([
            ["pointerdown", "hidden-track"],
            ["click", "hidden-track"],
            ["click", "hidden"],
            ["pointerdown", "under-track"],
            ["click", "under-track"],
            ["click", "under"],
            ["pointerdown", "named-label"],
            ["click", "named-label"],
            ["click", "named"],
            ["pointerdown", "plain-host"],
            ["click", "plain-host"],
            ["click", "plain"]
        ]),
        "{after}"
    );
    assert_eq!(
        after[1],
        json!([true, true, true, false, false, true]),
        "{after}"
    );
    assert_eq!(Picture::read(&out).width, 1440);
}

#[test]
fn a_click_on_a_checkbox_under_another_controls_label_or_a_link_in_its_own_stays_covered() {
    // The label on top clicks another control; the link on top takes the press, which never
    // reaches the label: either is a cover, named.
    for (name, css, cover) in [
        ("label-another", "#unnamed", "label#another.over"),
        ("label-link", "#linked", "a#terms.over"),
    ] {
        let message = format!(
            "--click {css} stayed covered: {cover} was on top at its middle after every scroll \
             within 5s"
        );
        if !refused(name, &labels(), "--click", css, &message) {
            return;
        }
    }
}

#[test]
fn a_click_on_a_checkbox_under_a_shadow_roots_button_or_an_object_in_its_label_stays_covered() {
    // The press lands on the button inside the component, or in the object's own document: neither
    // activates the label, so either is a cover, the button named in its host's shadow root.
    for (name, css, cover) in [
        (
            "label-shadow",
            "#shadowed",
            "button#inner in x-press#press's shadow root",
        ),
        ("label-object", "#objected", "object#object.track"),
    ] {
        let message = format!(
            "--click {css} stayed covered: {cover} was on top at its middle after every scroll \
             within 5s"
        );
        if !refused(name, &labels(), "--click", css, &message) {
            return;
        }
    }
}

#[test]
fn a_click_on_a_checkbox_under_what_is_slotted_into_a_shadow_button_or_a_closed_root_stays_covered()
{
    // A press on the span reaches the shadow button the span is slotted into, as the press travels
    // the flat tree, so the label never activates. Inside a custom element's closed shadow root the
    // page cannot see whether a control takes the press, so it is never taken as the label's: a
    // button there, or only decoration. On a plain span the browser's own hit finds the button in
    // its closed root, named in its host's.
    for (name, css, cover) in [
        ("label-slotted", "#slotted", "span#slotted-span.track"),
        ("label-closed", "#closed", "x-closed#closed-host"),
        ("label-closed-deco", "#adorned", "x-closed-deco#deco-host"),
        (
            "label-closed-span",
            "#spanned",
            "button in span#span-host's shadow root",
        ),
    ] {
        let message = format!(
            "--click {css} stayed covered: {cover} was on top at its middle after every scroll \
             within 5s"
        );
        if !refused(name, &labels(), "--click", css, &message) {
            return;
        }
    }
}

#[test]
fn a_target_whose_label_clicks_another_control_says_so_and_offers_no_label_to_target() {
    // The label that holds each one clicks another checkbox: named for what a press there does,
    // never as the thing to target. The second checkbox's own clip hides it, the icon takes no
    // pointer.
    for (name, css, message) in [
        (
            "label-pair",
            "#second",
            "--click #second stayed clipped by its own clip: label#pair.toggle, which holds it, \
             was on top at its middle after every scroll within 5s; a press there clicks \
             input#first (by its label), not it",
        ),
        (
            "label-mark",
            "#mark",
            "--click #mark takes no pointer at its middle (pointer-events: none): \
             label#other-label, which holds it, was on top there after every scroll within 5s; a \
             press there clicks input#other (by its label), not it",
        ),
        // The control in a component's shadow root, named in its host's, as a cover there is.
        (
            "label-shadow-control",
            "#checkmark",
            "--click #checkmark takes no pointer at its middle (pointer-events: none): \
             x-check#check-host, which holds it, was on top there after every scroll within 5s; a \
             press there clicks input#shadow-check in x-check#check-host's shadow root (by its \
             label), not it",
        ),
    ] {
        if !refused(name, &labels(), "--click", css, message) {
            return;
        }
    }
}

/// An icon with `pointer-events: none` in its button; a disabled button with `pointer-events:
/// none` in its row; an SVG ring around an empty middle; and a button under a cover of zero
/// opacity. Then a toolbar of two buttons with neither id nor class, each holding an icon with
/// `pointer-events: none`; a button its visibility hides; and a link in a panel folded to no
/// height. Then a button its box's `clip-path` cuts away at its middle; a checkbox its own
/// `clip-path` hides; and an icon with `pointer-events: none` straight in the body. Then an icon in
/// a span of its button, the span with `pointer-events: none` and a rounded `clip-path` that keeps
/// its middle; an SVG ring in a group a `clip-path` clips, its middle unpainted; an avatar its own
/// round `clip-path` keeps at its middle, painted under its card's background; and a button
/// outside the circle its box's `clip-path` keeps, inside that box. Then, in a box mirrored by
/// `scaleX(-1)` and in one turned by `rotate(180deg)`, a box with `pointer-events: none` whose
/// inset keeps its right half, holding a checkbox its own `clip` hides in that kept half.
fn ancestors() -> Vec<u8> {
    format!(
        r##"<!doctype html>
<meta charset="utf-8">
<style>
  body {{ margin: 0; padding: 20px; font: 16px/1.4 sans-serif; }}
  body > div, nav, main, section {{ margin: 0 0 40px; }}
  #cut {{ clip-path: inset(0 0 60% 0); height: 80px; background: #eef; }}
  #cut-button {{ margin-top: 50px; }}
  #sr-row {{ position: relative; height: 30px; }}
  #sr {{ position: absolute; width: 1px; height: 1px; margin: 0; clip-path: inset(50%); }}
  #icon, #off, .glyph {{ pointer-events: none; }}
  .wrap {{ position: relative; display: inline-block; }}
  .clear {{ position: absolute; inset: 0; opacity: 0; background: #000; }}
  #panel {{ max-height: 0; overflow: hidden; }}
  .kept {{ pointer-events: none; clip-path: inset(0 0 0 50%); width: 300px; height: 60px; position: relative; }}
  .own-clip {{ pointer-events: auto; position: absolute; left: 200px; top: 20px; width: 1px; height: 1px; margin: 0; clip: rect(0, 0, 0, 0); }}
</style>
<div><button id="go"><svg id="icon" width="24" height="24"><rect width="24" height="24" fill="#08f"/></svg> go</button></div>
<div id="row" class="row"><button id="off" disabled>off</button></div>
<div><svg id="rings" width="100" height="100"><circle id="ring" cx="50" cy="50" r="40" fill="none" stroke="#00f" stroke-width="8"/></svg></div>
<div><span class="wrap"><button id="faded">under a clear cover</button><span id="veil" class="clear"></span></span></div>
<nav class="tools"><button onclick="window.pressed = 'bold'"><svg class="glyph" width="24" height="24"><rect width="24" height="24" fill="#0a0"/></svg></button><button onclick="window.pressed = 'italic'"><svg class="glyph" id="italic" width="24" height="24"><rect width="24" height="24" fill="#a0a"/></svg></button></nav>
<div id="shelf"><button id="unseen" style="visibility: hidden">unseen</button> its visibility hidden</div>
<main id="board"><div id="panel"><p><a id="folded" href="#folded-followed">folded away</a></p></div></main>
<section id="cut-row"><div id="cut"><button id="cut-button">its box's clip-path cuts it away</button></div></section>
<section id="sr-row"><input type="checkbox" id="sr"> its own clip-path hides it</section>
<svg class="glyph" id="loose" width="20" height="20"><rect width="20" height="20" fill="#888"/></svg>
<div><button id="deco-button"><span id="deco" style="display: inline-block; clip-path: inset(0 round 6px); pointer-events: none; padding: 4px; background: #def"><svg id="decorated" width="24" height="24"><rect width="24" height="24" fill="#08f"/></svg></span> go</button></div>
<div><svg id="framed" width="100" height="100"><defs><clipPath id="frame"><rect width="100" height="100"/></clipPath></defs><g clip-path="url(#frame)"><circle id="framed-ring" cx="50" cy="50" r="40" fill="none" stroke="#00f" stroke-width="8"/></g></svg></div>
<div id="card" style="background: #fff; padding: 8px"><span id="avatar" style="display: inline-block; position: relative; z-index: -1; width: 40px; height: 40px; background: #f80; clip-path: circle(50%)"></span> under its card</div>
<section id="round-row"><div id="round" style="clip-path: circle(20px at 20px 20px); width: 300px; height: 60px; background: #fee"><button id="round-button" style="margin: 20px 0 0 120px">outside its box's circle</button></div></section>
<section><div id="mirrored" style="transform: scaleX(-1); width: 300px; height: 60px; background: #fee"><div class="kept"><input type="checkbox" id="mirrored-check" class="own-clip"></div></div></section>
<section><div id="turned" style="transform: rotate(180deg); width: 300px; height: 60px; background: #efe"><div class="kept"><input type="checkbox" id="turned-check" class="own-clip"></div></div></section>
{POINTED}"##
    )
    .into_bytes()
}

#[test]
fn a_target_with_no_pointer_in_its_button_fails_naming_the_button_which_then_takes_the_click() {
    for flag in ["--click", "--hover"] {
        let message = format!(
            "{flag} #icon takes no pointer at its middle (pointer-events: none): button#go, which \
             holds it, was on top there after every scroll within 5s; target button#go instead"
        );
        if !refused(
            &format!("icon{flag}"),
            &ancestors(),
            flag,
            "#icon",
            &message,
        ) {
            return;
        }
    }
    // Targeted as the failure says, the button takes the press and the click.
    let Some((output, _out, _requests, _scratch)) = on_page(
        "icon-button",
        &ancestors(),
        &["--click", "button#go", "--eval", "landed"],
    ) else {
        return;
    };
    assert!(output.status.success(), "{}", said(&output));
    assert_eq!(
        printed(&output),
        [json!([["pointerdown", "go"], ["click", "go"]])]
    );
}

#[test]
fn a_ring_a_disabled_button_with_no_pointer_and_a_clear_cover_still_refuse_the_click() {
    // What the hit test stopped pressing in its target's place: what holds the target, named as
    // such, and a cover, named as one.
    for (name, css, message) in [
        (
            "ring",
            "#ring",
            "--click #ring takes no pointer at its middle: svg#rings, which holds it, was on top \
             there after every scroll within 5s",
        ),
        (
            "disabled",
            "#off",
            "--click #off takes no pointer at its middle (pointer-events: none): div#row.row, \
             which holds it, was on top there after every scroll within 5s; target div#row.row \
             instead",
        ),
        (
            "clear-cover",
            "#faded",
            "--click #faded stayed covered: span#veil.clear was on top at its middle after every \
             scroll within 5s",
        ),
    ] {
        if !refused(name, &ancestors(), "--click", css, message) {
            return;
        }
    }
}

#[test]
fn a_target_in_one_of_buttons_alike_is_offered_a_selector_that_finds_its_button_alone() {
    // Its button's name finds the first button too: the selector offered pins it among its kin.
    let message = "--click #italic takes no pointer at its middle (pointer-events: none): nav.tools \
                   > button, which holds it, was on top there after every scroll within 5s; target \
                   nav.tools > button:nth-of-type(2) instead";
    if !refused("toolbar", &ancestors(), "--click", "#italic", message) {
        return;
    }
    // Targeted as the failure says, the second button takes the press, not the first.
    let Some((output, _out, _requests, _scratch)) = on_page(
        "toolbar-button",
        &ancestors(),
        &[
            "--click",
            "nav.tools > button:nth-of-type(2)",
            "--eval",
            "window.pressed",
        ],
    ) else {
        return;
    };
    assert!(output.status.success(), "{}", said(&output));
    assert_eq!(printed(&output), [json!("italic")]);
}

#[test]
fn a_target_its_visibility_hides_or_a_box_around_it_clips_away_fails_saying_so_offering_nothing() {
    // Neither takes no pointer: each says its own cause, and names nothing to target instead.
    for (name, css, message) in [
        (
            "unseen",
            "#unseen",
            "--click #unseen stayed hidden within 5s: its visibility is hidden, so no press \
             reaches it",
        ),
        (
            "folded",
            "#folded",
            "--click #folded stayed clipped: div#panel clips it away at its middle after every \
             scroll within 5s",
        ),
    ] {
        if !refused(name, &ancestors(), "--click", css, message) {
            return;
        }
    }
}

#[test]
fn a_target_a_clip_path_cuts_away_fails_as_clipped_offering_nothing() {
    // Not "takes no pointer": a clip-path cuts the press away from it, its box's or its own, and
    // what holds it is no substitute.
    for (name, css, message) in [
        (
            "cut",
            "#cut-button",
            "--click #cut-button stayed clipped: div#cut clips it away at its middle after every \
             scroll within 5s",
        ),
        (
            "cut-own",
            "#sr",
            "--click #sr stayed clipped by its own clip: section#sr-row, which holds it, was on top \
             at its middle after every scroll within 5s",
        ),
        // Inside its box, outside the circle the box's clip-path keeps.
        (
            "cut-round",
            "#round-button",
            "--click #round-button stayed clipped: div#round clips it away at its middle after \
             every scroll within 5s",
        ),
    ] {
        if !refused(name, &ancestors(), "--click", css, message) {
            return;
        }
    }
}

#[test]
fn a_target_no_clip_path_cuts_at_its_middle_is_never_called_clipped() {
    // Each misses its click for another cause than the clip-path around it or its own: the span's
    // inherited pointer-events, the ring's unpainted middle (what holds it named, a press on it
    // offered for nothing), a card painted over the avatar, a checkbox's own clip.
    for (name, css, message) in [
        (
            "uncut-icon",
            "#decorated",
            "--click #decorated takes no pointer at its middle (pointer-events: none): \
             button#deco-button, which holds it, was on top there after every scroll within 5s; \
             target button#deco-button instead",
        ),
        (
            "uncut-ring",
            "#framed-ring",
            "--click #framed-ring takes no pointer at its middle: svg#framed, which holds it, was \
             on top there after every scroll within 5s",
        ),
        // Mirrored or half-turned with its box, a clip-path keeps the point its own box's
        // coordinates would cut: never tested there, the checkbox's own clip is what is named.
        (
            "uncut-mirrored",
            "#mirrored-check",
            "--click #mirrored-check stayed clipped by its own clip: div#mirrored, which holds it, \
             was on top at its middle after every scroll within 5s",
        ),
        (
            "uncut-turned",
            "#turned-check",
            "--click #turned-check stayed clipped by its own clip: div#turned, which holds it, was \
             on top at its middle after every scroll within 5s",
        ),
        (
            "uncut-avatar",
            "#avatar",
            "--click #avatar stayed covered: div#card was on top at its middle after every scroll \
             within 5s",
        ),
    ] {
        if !refused(name, &ancestors(), "--click", css, message) {
            return;
        }
    }
}

#[test]
fn a_target_with_no_pointer_straight_in_the_body_is_offered_nothing_to_target() {
    let message = "--click #loose takes no pointer at its middle (pointer-events: none): html > \
                   body, which holds it, was on top there after every scroll within 5s; html > \
                   body is the page itself, nothing to target instead";
    refused("loose", &ancestors(), "--click", "#loose", message);
}

/// A link wrapped over two lines, its first line ending a line a spacer fills and its second
/// starting one another spacer ends, so the middle of its bounding box falls on a spacer; and,
/// further down, a link wrapped over twenty lines, taller than a view 300 px high; and further
/// down still, one wrapped over eight lines (192 px) in a scrolling box 72 px high. The words are
/// boxes of their own, so no font moves a line's break.
fn wrapped() -> Vec<u8> {
    let words = r#"<span class="word"></span>"#;
    format!(
        r##"<!doctype html>
<meta charset="utf-8">
<style>
  body {{ margin: 0; padding: 0 20px; font: 16px/24px sans-serif; }}
  p {{ margin: 0; }}
  .word, .spacer {{ display: inline-block; height: 24px; vertical-align: top; }}
  .word {{ width: 40px; background: #08f; }}
  .spacer {{ width: 250px; background: #eee; }}
  #short {{ width: 300px; }}
  #long {{ width: 60px; }}
  .filler {{ height: 600px; }}
  #box {{ height: 72px; width: 300px; overflow: auto; }}
  #box p {{ width: 60px; }}
</style>
<p id="short"><span class="spacer" id="before"></span><a id="wrapped" href="#wrapped-followed">{words} {words}</a><span class="spacer" id="after"></span></p>
<div class="filler"></div>
<p id="long"><a id="tall" href="#tall-followed">{}</a></p>
<div class="filler"></div>
<div id="box"><p><a id="boxed" href="#boxed-followed">{}</a></p></div>
<div class="filler"></div>
<script>
  window.reached = id => {{ const e = document.getElementById(id), b = e.getBoundingClientRect(); const at = document.elementFromPoint(b.x + b.width / 2, b.y + b.height / 2); return [!!at && e.contains(at), at && at.className]; }};
  window.lines = id => document.getElementById(id).getClientRects().length;
</script>
{POINTED}"##,
        [words; 20].join(" "),
        [words; 8].join(" ")
    )
    .into_bytes()
}

#[test]
fn a_click_on_a_link_wrapped_over_lines_aims_at_its_first_line_in_view_and_follows_it() {
    let Some((output, _out, _requests, _scratch)) = on_page(
        "wrapped",
        &wrapped(),
        &[
            "--size",
            "800x300",
            "--eval",
            "[lines('wrapped'), reached('wrapped')]",
            "--click",
            "#wrapped",
            "--eval",
            "location.hash",
            "--click",
            "#tall",
            "--eval",
            "(e => [lines('tall'), e.getBoundingClientRect().height, (r => r.y + r.height / 2)(e.getClientRects()[0]), location.hash])(document.getElementById('tall'))",
        ],
    ) else {
        return;
    };
    assert!(output.status.success(), "{}", said(&output));
    let lines = printed(&output);
    let [short, first, tall] = lines.as_slice() else {
        panic!("three evaluations: {}", said(&output));
    };
    // Over two lines, the middle of its bounding box on a spacer (the one before it or the one
    // after, by which line its height counts in): none of it there.
    assert_eq!(short, &json!([2, [false, "spacer"]]), "{short}");
    assert_eq!(first, &json!("#wrapped-followed"));
    // Twenty lines taller than the view, so scrolled to the middle its first line's middle is above
    // the view: aimed at its first line in view, followed.
    assert_eq!(tall[0], json!(20), "{tall}");
    assert!(tall[1].as_f64().unwrap() > 300.0, "{tall}");
    assert!(tall[2].as_f64().unwrap() < 0.0, "{tall}");
    assert_eq!(tall[3], json!("#tall-followed"), "{tall}");
}

#[test]
fn a_click_on_a_link_taller_than_its_scrolling_box_aims_at_a_line_the_box_shows_and_follows_it() {
    let Some((output, _out, _requests, _scratch)) = on_page(
        "boxed",
        &wrapped(),
        &[
            "--size",
            "800x300",
            "--click",
            "#boxed",
            "--eval",
            "(e => { const box = document.getElementById('box').getBoundingClientRect(), first = (r => r.y + r.height / 2)(e.getClientRects()[0]); return [lines('boxed'), first < box.y, first >= 0, location.hash]; })(document.getElementById('boxed'))",
        ],
    ) else {
        return;
    };
    assert!(output.status.success(), "{}", said(&output));
    // Scrolled to the middle of its box, its first line's middle is above the box, which clips it
    // away there, yet in the view: aimed at a line the box shows, and followed.
    assert_eq!(
        printed(&output),
        [json!([8, true, true, "#boxed-followed"])],
        "{}",
        said(&output)
    );
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
