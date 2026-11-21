//! `symphony shot` against the stand-in server (`site`): the workspace's own build of the document
//! CLI, run as an agent runs it, with this machine's Chrome. Its pictures are read back here: what
//! they show, their size, and what a failure leaves behind (no picture, no browser, no profile).

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

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
/// The open finished-work menu's rectangle.
const MENU: &str = "(() => { const b = document.querySelector('[role=listbox][data-state=open]').getBoundingClientRect(); return [b.x, b.y, b.width, b.height]; })()";

#[test]
fn the_tasks_page_is_captured_drawn_with_its_columns_and_a_click_records_the_open_menu() {
    let site = site();
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    let scratch = Scratch::new("menu");
    let (before, right, open, last) = (
        scratch.join("board.png"),
        scratch.join("right.png"),
        scratch.join("open.png"),
        scratch.join("last.png"),
    );
    let output = shot(
        &chrome,
        &[
            &site.url("/tasks"),
            "--out",
            last.to_str().unwrap(),
            "--click",
            ".board-filters-toggle",
            "--shot",
            before.to_str().unwrap(),
            "--eval",
            COLUMNS,
            "--eval",
            "document.querySelector('.waiting-columns').scrollLeft = 10000",
            "--shot",
            right.to_str().unwrap(),
            "--eval",
            COLUMNS,
            "--eval",
            "document.querySelector('.waiting-columns').scrollLeft = 0",
            "--click",
            "button[aria-label=\"Finished tasks\"]",
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
    let [columns, across, shifted, back, menu] = lines.as_slice() else {
        panic!("five evaluations, one line each: {}", said(&output));
    };
    assert!(across.as_f64().unwrap() > 0.0);
    assert_eq!(back, &json!(0));
    let board = Picture::read(&before);
    let scrolled = Picture::read(&right);
    assert_eq!((board.width, board.height), (1440, 900));
    let everything = board.colours([0.0, 0.0, 1440.0, 900.0], 1.0);
    assert!(
        everything > 16,
        "the board is {everything} colour(s): not drawn"
    );
    let columns = columns.as_array().unwrap();
    let statuses: Vec<_> = columns.iter().map(|column| &column["status"]).collect();
    // Lifecycle and machine columns all draw, including those reached by scrolling.
    assert_eq!(
        statuses,
        [
            &json!("concept"),
            &json!("design"),
            &json!("ready"),
            &json!("in_progress"),
            &json!("build"),
            &json!("test"),
            &json!("review"),
            &json!("done")
        ],
        "the board's columns: {columns:?}"
    );
    for (column, moved) in columns.iter().zip(shifted.as_array().unwrap()) {
        assert_eq!(column["status"], moved["status"]);
        let initial = rect(&column["heading"]);
        let (picture, heading) = if initial[0] >= 0.0 && initial[0] + initial[2] <= 1440.0 {
            (&board, initial)
        } else {
            (&scrolled, rect(&moved["heading"]))
        };
        assert!(heading[2] > 0.0 && heading[3] > 0.0, "{heading:?}");
        assert!(
            heading[0] >= 0.0 && heading[0] + heading[2] <= 1440.0,
            "{heading:?}"
        );
        let drawn = picture.colours(heading, 1.0);
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
            // A phone's board keeps its search behind this button.
            "--click",
            ".board-filters-toggle",
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

/// What a page reads of its clock: now, a Date made with no argument, one made from a time, Date
/// called as a function, whether a Date is still a Date, and the browser's own parse and UTC.
const CLOCK_READS: &str = "[Date.now(), new Date().getTime(), new Date(5).getTime(), \
    Date() === new Date(Date.now()).toString(), new Date() instanceof Date, \
    Date.parse('2026-03-02T09:30:00Z'), Date.UTC(1970, 0, 2), new Date().toISOString()]";

#[test]
fn a_shot_with_a_clock_holds_the_pages_now_still_and_leaves_every_other_date_alone() {
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    let scratch = Scratch::new("clock");
    let picture = scratch.join("clock.png");
    // The page writes its own now as it loads, as a page's first script would read it; a frame of
    // its own origin reads the same clock.
    let page = "data:text/html,<p id=at></p><iframe srcdoc=\"<script>parent.inFrame = Date.now()</script>\"></iframe>\
        <script>document.getElementById('at').textContent = Date.now()</script>";
    let output = shot(
        &chrome,
        &[
            page,
            "--out",
            picture.to_str().unwrap(),
            "--clock",
            "1772443800",
            "--eval",
            CLOCK_READS,
            "--pause",
            "40",
            "--eval",
            "[Date.now(), Number(document.getElementById('at').textContent), window.inFrame]",
        ],
    )
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", said(&output));
    let lines = printed(&output);
    let held = 1_772_443_800_000_u64;
    assert_eq!(
        lines[0],
        json!([
            held,
            held,
            5,
            true,
            true,
            held,
            86_400_000,
            "2026-03-02T09:30:00.000Z"
        ]),
        "{}",
        said(&output)
    );
    // Still that moment after a pause, as it was for the page's own script and its frame's.
    assert_eq!(lines[1], json!([held, held, held]), "{}", said(&output));
    assert!(picture.is_file());

    // Without one the page's clock is the machine's: this run's, within a day.
    let output = shot(
        &chrome,
        &[page, "--out", picture.to_str().unwrap(), "--eval", "Date.now()"],
    )
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", said(&output));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as f64;
    let read = printed(&output)[0].as_f64().unwrap();
    assert!((now - read).abs() < 86_400_000.0, "now {now}, the page's {read}");
}

/// A page whose region on the screen, in a box that clips it, says it is busy (`aria-busy`, as a
/// Symphony page's ghost does until its reads are in) for ninety frames after it loads, with
/// nothing in flight meanwhile; three regions busy off the screen, none ever done: one below the
/// window, one beside it and one scrolled out of the box that clips it.
const BUSY: &str = r#"<!doctype html><title>busy</title>
<div style="overflow: hidden"><main id="ghost" aria-busy="true"><p>Loading…</p></main></div>
<div style="height: 100px; overflow: auto"><div style="height: 400px"></div><section aria-busy="true">Scrolled out of its box</section></div>
<div style="height: 2000px"></div>
<section aria-busy="true">Below the window</section>
<section aria-busy="true" style="position: absolute; top: 0; left: 3000px">Beside the window</section>
<script>
window.__frames = 0;
(function frame() {
    if (++window.__frames < 90) requestAnimationFrame(frame);
    else document.getElementById("ghost").setAttribute("aria-busy", "false");
})();
</script>"#;

/// The shot counts a page drawn only once no region on the screen says it is busy, one in a box
/// that clips it among them, however long that takes with nothing in flight, as a loaded machine's
/// board stays under its ghost a while after its first read answers; a region busy off the screen,
/// below the window, beside it or out of its box, which draws only once the reader goes to it, it
/// does not wait for.
#[test]
fn a_page_busy_on_the_screen_is_waited_for_and_one_busy_off_it_is_not() {
    let site = site();
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    site.set_file("/busy.html", BUSY.as_bytes().to_vec(), "text/html");
    let scratch = Scratch::new("busy");
    let out = scratch.join("busy.png");
    let output = shot(
        &chrome,
        &[
            &site.url("/busy.html"),
            "--out",
            out.to_str().unwrap(),
            "--size",
            "800x300",
            "--eval",
            "[document.getElementById('ghost').getAttribute('aria-busy'), window.__frames, document.querySelectorAll('[aria-busy=true]').length]",
        ],
    )
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", said(&output));
    let lines = printed(&output);
    assert_eq!(
        (
            &lines[0][0],
            lines[0][1].as_u64().unwrap_or(0) >= 90,
            &lines[0][2]
        ),
        (&json!("false"), true, &json!(3)),
        "the region on the screen done after its ninety frames, the three off it still busy: {lines:?}"
    );
}

/// A page whose first read failed is drawn saying so, never busy: the runs, a run's page and
/// Reports, each read refused, are shot saying they could not be read (`data-ready=failed`), with
/// no region busy left on the page, which would hold the shot to its timeout.
#[test]
fn a_page_whose_first_read_failed_is_shot_saying_so() {
    let site = site();
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    for path in [
        symphony_api::RUNS_PATH,
        symphony_api::RUN_PATH,
        symphony_api::REPORTS_PATH,
    ] {
        site.fail_route(path, true);
    }
    let scratch = Scratch::new("failed");
    let run = format!("/runs?run={:032x}", 1);
    for (n, (page, said_so)) in [
        ("/runs", "The runs could not be read"),
        (run.as_str(), "The run could not be read"),
        ("/reports", "The reports could not be read"),
    ]
    .into_iter()
    .enumerate()
    {
        let out = scratch.join(&format!("failed-{n}.png"));
        let eval = format!(
            "[document.querySelectorAll('[data-ready=failed]').length, \
             document.querySelectorAll('[aria-busy=true]').length, \
             document.body.innerText.includes({})]",
            json!(said_so)
        );
        let output = shot(
            &chrome,
            &[
                &site.url(page),
                "--out",
                out.to_str().unwrap(),
                "--eval",
                &eval,
            ],
        )
        .output()
        .unwrap();
        assert!(output.status.success(), "{page}: {}", said(&output));
        assert_eq!(
            printed(&output)[0],
            json!([1, 0, true]),
            "{page}: drawn failed, nothing busy, saying {said_so:?}"
        );
    }
}

/// `--attach` stores each picture the run saved, by its content, in the uploads of the main
/// checkout of the repository the shot runs in, and prints the line a thread post carries for it,
/// in the order taken; the pictures stay where they were saved.
#[test]
fn an_attached_shot_stores_each_picture_it_saved_and_prints_its_line_in_the_order_taken() {
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    let site = site();
    site.set_file(
        "/attach.html",
        b"<!doctype html><title>attach</title><h1>Before</h1>".to_vec(),
        "text/html",
    );
    let scratch = Scratch::new("attach");
    // A checkout's top folder as the board's lookup reads it, with no git process.
    let checkout = scratch.join("checkout");
    std::fs::create_dir_all(checkout.join(".git")).unwrap();
    std::fs::write(checkout.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    let output = shot(
        &chrome,
        &[
            &site.url("/attach.html"),
            "--out",
            "evidence/after.png",
            "--shot",
            "evidence/before.png",
            "--eval",
            "document.querySelector('h1').textContent = 'After'",
            "--attach",
        ],
    )
    .current_dir(&checkout)
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", said(&output));
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    let [evaluated, before, after] = lines.as_slice() else {
        panic!("the evaluation and a line a picture: {}", said(&output));
    };
    assert_eq!(*evaluated, "\"After\"");
    let mut stored = Vec::new();
    for (line, name) in [(before, "before.png"), (after, "after.png")] {
        let file = line
            .strip_prefix(&format!("![{name}](/artifacts/uploads/"))
            .and_then(|rest| rest.strip_suffix(')'))
            .unwrap_or_else(|| panic!("{line} is no line attaching {name}"));
        let (sha, ext) = file.split_once('.').unwrap();
        assert!(
            ext == "png" && sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
            "{file}"
        );
        let saved = std::fs::read(checkout.join("evidence").join(name)).unwrap();
        assert_eq!(
            std::fs::read(checkout.join("artifacts/uploads").join(file)).unwrap(),
            saved,
            "{name} is stored as it was saved"
        );
        stored.push(file.to_string());
    }
    assert_ne!(stored[0], stored[1], "the page changed between the two");
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
/// a plain span, and one under a closed shadow root of decoration only on a plain span. Then, each
/// hidden in its label: one under a span slotted into a button in a closed shadow root on a plain
/// span, and one under a span slotted into a button in a component's closed shadow root; one under
/// a custom element never defined, and one under a defined custom element with no shadow root.
/// Then one hidden in its label under a track twenty plain spans deep, no shadow root anywhere.
fn labels() -> Vec<u8> {
    let deep = format!(
        r#"{}<span id="deep-track" class="track"></span>{}"#,
        "<span>".repeat(20),
        "</span>".repeat(20)
    );
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
<p><label class="toggle"><input type="checkbox" id="slot-spanned" class="hidden"><span id="slot-span-host"><span id="slot-span-track" class="track"></span></span> a span slotted into a closed root's button on a span, over it</label></p>
<p><label class="toggle"><input type="checkbox" id="slot-closed" class="hidden"><x-closed-wrap id="closed-wrap"><span id="closed-slotted" class="track"></span></x-closed-wrap> a span slotted into a component's closed button, over it</label></p>
<p><label class="toggle"><input type="checkbox" id="undefined" class="hidden"><x-undefined id="undefined-host" class="track"></x-undefined> a custom element never defined, over it</label></p>
<p><label class="toggle"><input type="checkbox" id="light" class="hidden"><x-light id="light-host" class="track"></x-light> a custom element in the light, over it</label></p>
<p><label class="toggle"><input type="checkbox" id="deep" class="hidden">{deep} a track deep in plain spans, over it</label></p>
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
  const slotted = '<button type="button" style="padding: 0; border: 0"><slot></slot></button>';
  document.getElementById('slot-span-host').attachShadow({{mode: 'closed'}}).innerHTML = slotted;
  component('x-closed-wrap', 'closed', slotted);
  customElements.define('x-light', class extends HTMLElement {{}});
</script>
{POINTED}"##
    )
    .into_bytes()
}

/// The checkboxes of [`labels`] a click reaches through its label, in the order [`label_clicks`]
/// clicks them: three its own label covers and one under a closed root's decoration on a span;
/// three under a custom element no control takes; one under a track deep in plain spans.
const CLICKED: [&str; 8] = [
    "hidden",
    "under",
    "named",
    "plain",
    "undefined",
    "light",
    "adorned",
    "deep",
];

/// The checkboxes of [`labels`] no click of [`label_clicks`] is on, each read after them all.
const LEFT: [&str; 4] = ["unnamed", "linked", "closed", "slot-closed"];

/// What one run of `symphony shot` on [`labels`] did, clicking each of [`CLICKED`] in turn: what
/// it said, the presses and clicks the page took in order (`landed`, three a checkbox: the press
/// and the click where the pointer was, and the click the label gave its control), whether each
/// checkbox of [`CLICKED`] and [`LEFT`] is checked after them all, by its id, and its picture's
/// width; of a run that failed, what it said alone.
struct LabelClicks {
    said: String,
    succeeded: bool,
    evaluations: usize,
    landed: Vec<Value>,
    checked: serde_json::Map<String, Value>,
    width: Option<usize>,
}

impl LabelClicks {
    /// The run succeeded and printed its one evaluation.
    #[track_caller]
    fn ran(&self) {
        assert!(self.succeeded, "{}", self.said);
        assert_eq!(self.evaluations, 1, "one evaluation: {}", self.said);
    }

    /// What the page took for the click on `id`: its three of `landed`.
    #[track_caller]
    fn of(&self, id: &str) -> Value {
        self.ran();
        assert_eq!(
            self.landed.len(),
            3 * CLICKED.len(),
            "a press, a click and the control's click for each of {CLICKED:?} and nothing more: {:?}",
            self.landed
        );
        let at = CLICKED
            .iter()
            .position(|clicked| *clicked == id)
            .expect("a checkbox the run clicks");
        json!(self.landed[3 * at..3 * at + 3])
    }

    /// Whether each of `ids` is checked once every click of the run is done.
    #[track_caller]
    fn checked(&self, ids: &[&str]) -> Vec<Value> {
        self.ran();
        ids.iter().map(|id| self.checked[*id].clone()).collect()
    }
}

/// The one run of `symphony shot` the label tests read ([`LabelClicks`]), made by the first of
/// them to ask: each click is on a checkbox of its own row of [`labels`], which no other click
/// reaches, so the tests' clicks meet on one page as they did on a page each. `None` when this
/// machine has no Chrome.
fn label_clicks() -> Option<&'static LabelClicks> {
    static RUN: std::sync::OnceLock<Option<LabelClicks>> = std::sync::OnceLock::new();
    RUN.get_or_init(|| {
        let selectors: Vec<String> = CLICKED.iter().map(|id| format!("#{id}")).collect();
        let mut steps: Vec<&str> = selectors
            .iter()
            .flat_map(|selector| ["--click", selector.as_str()])
            .collect();
        let read = format!(
            "[landed, Object.fromEntries({}.map(id => [id, document.getElementById(id).checked]))]",
            json!([&CLICKED[..], &LEFT[..]].concat())
        );
        steps.extend(["--eval", read.as_str()]);
        let (output, out, _requests, _scratch) = on_page("labels", &labels(), &steps)?;
        let succeeded = output.status.success();
        let lines = if succeeded {
            printed(&output)
        } else {
            Vec::new()
        };
        let after = lines.first().cloned().unwrap_or_default();
        Some(LabelClicks {
            said: said(&output),
            succeeded,
            evaluations: lines.len(),
            landed: after[0].as_array().cloned().unwrap_or_default(),
            checked: after[1].as_object().cloned().unwrap_or_default(),
            width: succeeded.then(|| Picture::read(&out).width),
        })
    })
    .as_ref()
}

#[test]
fn a_click_on_a_checkbox_its_own_label_covers_lands_on_the_label_and_checks_it() {
    let Some(clicks) = label_clicks() else { return };
    // Each press landed on the label, or on its track, and the label clicked its control. The
    // last, on a closed root's decoration, the browser's own hit confirmed: no control there.
    assert_eq!(
        json!([
            clicks.of("hidden"),
            clicks.of("under"),
            clicks.of("named"),
            clicks.of("plain")
        ]),
        json!([
            [
                ["pointerdown", "hidden-track"],
                ["click", "hidden-track"],
                ["click", "hidden"]
            ],
            [
                ["pointerdown", "under-track"],
                ["click", "under-track"],
                ["click", "under"]
            ],
            [
                ["pointerdown", "named-label"],
                ["click", "named-label"],
                ["click", "named"]
            ],
            [
                ["pointerdown", "plain-host"],
                ["click", "plain-host"],
                ["click", "plain"]
            ]
        ]),
        "{}",
        clicks.said
    );
    assert_eq!(
        clicks.checked(&["hidden", "under", "named", "unnamed", "linked", "plain"]),
        [true, true, true, false, false, true],
        "{}",
        clicks.said
    );
    assert_eq!(clicks.width, Some(1440));
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
    // the flat tree, so the label never activates. The browser's own hit finds a button in a closed
    // shadow root, a component's or a plain span's, named in its host's.
    for (name, css, cover) in [
        ("label-slotted", "#slotted", "span#slotted-span.track"),
        (
            "label-closed",
            "#closed",
            "button#closed-button in x-closed#closed-host's shadow root",
        ),
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
fn a_click_on_a_checkbox_under_a_span_slotted_into_a_closed_roots_button_stays_covered() {
    // The page cannot see the slot a closed shadow root assigns the span to, so it takes the press
    // as the label's; the browser names the slot, and the walk up from it finds the button the
    // press reaches, which never activates the label. On a plain span, or on a component.
    for (name, css, cover) in [
        (
            "label-slot-span",
            "#slot-spanned",
            "span#slot-span-track.track",
        ),
        (
            "label-slot-closed",
            "#slot-closed",
            "span#closed-slotted.track",
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
fn a_click_on_a_checkbox_under_a_custom_element_no_control_takes_lands_on_the_label_and_checks_it()
{
    // A custom element never defined, one with no shadow root, and one whose closed root holds only
    // decoration: the page cannot see inside the last, the browser's own hit confirms nothing there
    // takes the press, and the label clicks its control.
    let Some(clicks) = label_clicks() else { return };
    assert_eq!(
        json!([
            clicks.of("undefined"),
            clicks.of("light"),
            clicks.of("adorned")
        ]),
        json!([
            [
                ["pointerdown", "undefined-host"],
                ["click", "undefined-host"],
                ["click", "undefined"]
            ],
            [
                ["pointerdown", "light-host"],
                ["click", "light-host"],
                ["click", "light"]
            ],
            [
                ["pointerdown", "deco-host"],
                ["click", "deco-host"],
                ["click", "adorned"]
            ]
        ]),
        "{}",
        clicks.said
    );
    assert_eq!(
        clicks.checked(&["undefined", "light", "adorned", "closed", "slot-closed"]),
        [true, true, true, false, false],
        "{}",
        clicks.said
    );
}

#[test]
fn a_click_on_a_checkbox_under_a_track_deep_in_plain_spans_lands_on_the_label_and_checks_it() {
    // Each span may hold a closed shadow root the page cannot see, so the browser is asked about
    // every one on the press's way, once, however many: none holds one, and the label clicks its
    // control.
    let Some(clicks) = label_clicks() else { return };
    assert_eq!(
        clicks.of("deep"),
        json!([
            ["pointerdown", "deep-track"],
            ["click", "deep-track"],
            ["click", "deep"]
        ]),
        "{}",
        clicks.said
    );
    assert_eq!(clicks.checked(&["deep"]), [true], "{}", clicks.said);
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

/// All in a root lifted onto a layer of its own (`translateZ(0)`): a button its box's `clip-path`
/// cuts away at its middle, that box in place, moved across the page by a `transform`, and by the
/// `translate` property. Then, in a square box moved across the page, one turned a quarter, one
/// mirrored by `scaleX(-1)` and one turned by `rotate(180deg)`, a box with `pointer-events: none`
/// whose inset keeps its right half, holding a checkbox its own `clip` hides in that kept half, at
/// its bottom right, which each move but the first carries out of the half the box's place keeps.
fn moved() -> Vec<u8> {
    let held = |id: &str, transform: &str| {
        format!(
            r#"<section><div id="{id}" class="square" style="transform: {transform}"><div class="kept"><input type="checkbox" id="{id}-check" class="own-clip"></div></div></section>"#
        )
    };
    format!(
        r##"<!doctype html>
<meta charset="utf-8">
<style>
  body {{ margin: 0; padding: 20px; font: 16px/1.4 sans-serif; }}
  section {{ margin: 0 0 40px; }}
  .cut {{ clip-path: inset(0 0 60% 0); height: 80px; background: #eef; }}
  .cut > button {{ margin-top: 50px; }}
  .square {{ width: 100px; height: 100px; background: #fee; }}
  .kept {{ pointer-events: none; clip-path: inset(0 0 0 50%); width: 100px; height: 100px; position: relative; }}
  .own-clip {{ pointer-events: auto; position: absolute; left: 75px; top: 75px; width: 1px; height: 1px; margin: 0; clip: rect(0, 0, 0, 0); }}
</style>
<main id="app" style="transform: translateZ(0)">
<section><div id="lifted" class="cut"><button id="lifted-button">its box's clip-path cuts it away</button></div></section>
<section><div id="slid" class="cut" style="transform: translate(30px, 10px)"><button id="slid-button">its box, moved, cuts it away</button></div></section>
<section><div id="nudged" class="cut" style="translate: 30px 10px"><button id="nudged-button">its box, moved, cuts it away</button></div></section>
{shifted}
{quartered}
{mirrored}
{turned}
</main>
{POINTED}"##,
        shifted = held("shifted", "translate(40px, 0)"),
        quartered = held("quartered", "rotate(90deg)"),
        mirrored = held("mirrored", "scaleX(-1)"),
        turned = held("turned", "rotate(180deg)"),
    )
    .into_bytes()
}

#[test]
fn a_clip_path_on_a_box_moved_only_across_the_page_is_read_and_one_turned_or_mirrored_is_not() {
    // A box lifted onto a layer, or moved across the page, by a transform or the translate
    // property, still cuts the press away where its clip-path leaves the middle out.
    for (name, css, by) in [
        ("lifted", "#lifted-button", "div#lifted.cut"),
        ("slid", "#slid-button", "div#slid.cut"),
        ("nudged", "#nudged-button", "div#nudged.cut"),
    ] {
        let message = format!(
            "--click {css} stayed clipped: {by} clips it away at its middle after every scroll \
             within 5s"
        );
        if !refused(&format!("moved-{name}"), &moved(), "--click", css, &message) {
            return;
        }
    }
    // The checkbox's own clip is what hides it: the clip-path around it keeps its middle, moved
    // across the page with its box or turned, mirrored or half-turned with it, where the box's
    // place alone would cut it; never called clipped by that box.
    for id in ["shifted", "quartered", "mirrored", "turned"] {
        let message = format!(
            "--click #{id}-check stayed clipped by its own clip: div#{id}.square, which holds it, \
             was on top at its middle after every scroll within 5s"
        );
        let css = format!("#{id}-check");
        if !refused(&format!("moved-{id}"), &moved(), "--click", &css, &message) {
            return;
        }
    }
}

/// Buttons inside boxes whose `clip-path` leaves their middle out: an inset with a `calc()` side, a
/// circle placed by four values (computed as `calc()`), an inset's rounded corner, and an inset on
/// the box's content box, where its border box would keep the button.
fn shaped() -> Vec<u8> {
    format!(
        r##"<!doctype html>
<meta charset="utf-8">
<style>
  body {{ margin: 0; padding: 20px; font: 16px/1.4 sans-serif; }}
  section {{ margin: 0 0 40px; }}
  section > div {{ position: relative; width: 300px; height: 60px; background: #eef; }}
  section button {{ position: absolute; width: 20px; height: 20px; padding: 0; border: 0; background: #08f; }}
</style>
<section><div id="rounded" style="clip-path: inset(0 round 30px)"><button id="corner-button" style="left: 0; top: 0; width: 12px; height: 12px"></button></div></section>
<section><div id="calced" style="clip-path: inset(0 calc(50% + 10px) 0 0)"><button id="calc-button" style="left: 200px; top: 20px"></button></div></section>
<section><div id="placed" style="clip-path: circle(20px at right 10px top 20px)"><button id="placed-button" style="left: 20px; top: 20px"></button></div></section>
<section><div id="boxed" style="clip-path: inset(0 0 0 50%) content-box; width: 200px; padding-left: 100px"><button id="boxed-button" style="left: 160px; top: 20px"></button></div></section>
{POINTED}"##
    )
    .into_bytes()
}

#[test]
fn a_clip_path_with_a_calc_or_a_four_value_place_is_read_and_a_rounded_corner_or_another_box_is_not()
 {
    // Chrome computes a calc() of a length and a percentage, and a place by four values, to
    // calc(P% + Lpx): read, the box is named.
    for (name, css, by) in [
        ("calced", "#calc-button", "div#calced"),
        ("placed", "#placed-button", "div#placed"),
    ] {
        let message = format!(
            "--click {css} stayed clipped: {by} clips it away at its middle after every scroll \
             within 5s"
        );
        if !refused(
            &format!("shaped-{name}"),
            &shaped(),
            "--click",
            css,
            &message,
        ) {
            return;
        }
    }
    // A rounded corner's arc and a reference box other than the border box are not read: what
    // holds the button is named, no cause made up.
    for (name, css) in [("rounded", "#corner-button"), ("boxed", "#boxed-button")] {
        let message = format!(
            "--click {css} takes no pointer at its middle: body > section, which holds it, was on \
             top there after every scroll within 5s"
        );
        if !refused(
            &format!("shaped-{name}"),
            &shaped(),
            "--click",
            css,
            &message,
        ) {
            return;
        }
    }
}

/// Two labels at whole pixels, each over a checkbox hidden in it whose 1px box starts a fifth of a
/// pixel past a whole one (its middle seven tenths past): in the first, a plain span starting at
/// the next whole pixel holds a button in a closed shadow root, which records its clicks; the
/// second holds nothing else. A hit at the pixel the middle lies in finds the label, one at the
/// middle itself, the button.
fn fractional() -> Vec<u8> {
    format!(
        r##"<!doctype html>
<meta charset="utf-8">
<style>
  body {{ margin: 0; font: 16px/1.4 sans-serif; }}
  .row {{ position: relative; height: 24px; margin: 20px 0 40px 20px; }}
  label {{ position: absolute; left: 0; top: 0; width: 120px; height: 24px; background: #eee; }}
  .sr {{ position: absolute; left: 30.2px; top: 10.2px; width: 1px; height: 1px; margin: 0; padding: 0; border: 0; overflow: hidden; clip: rect(0, 0, 0, 0); }}
  #split-host {{ position: absolute; left: 31px; top: 0; width: 40px; height: 24px; }}
</style>
<div class="row"><label id="split-label"><input type="checkbox" id="split" class="sr"><span id="split-host"></span></label></div>
<div class="row"><label id="whole-label"><input type="checkbox" id="whole" class="sr"></label></div>
{POINTED}
<script>
  const root = document.getElementById('split-host').attachShadow({{mode: 'closed'}});
  root.innerHTML = '<button type="button" style="display: block; width: 40px; height: 24px; padding: 0; border: 0; background: #08f"></button>';
  root.querySelector('button').addEventListener('click', () => landed.push(['click', 'split-button']));
</script>"##
    )
    .into_bytes()
}

#[test]
fn a_click_aims_tests_and_presses_one_whole_pixel_of_the_page() {
    // The box's middle lies past the span's edge, in a pixel before it: the aim tests, and the
    // browser's own hit confirms, at the middle's nearest whole pixel, where the press would land,
    // finds the button in the span's closed root, and presses nothing.
    let message = "--click #split stayed covered: button in span#split-host's shadow root was on \
                   top at its middle after every scroll within 5s";
    if !refused(
        "fractional-split",
        &fractional(),
        "--click",
        "#split",
        message,
    ) {
        return;
    }
    // With nothing past that edge, the press at the whole pixel lands on the label, which checks
    // its control.
    let Some((output, _out, _requests, _scratch)) = on_page(
        "fractional-whole",
        &fractional(),
        &[
            "--click",
            "#whole",
            "--eval",
            "[landed, document.getElementById('whole').checked]",
        ],
    ) else {
        return;
    };
    assert!(output.status.success(), "{}", said(&output));
    assert_eq!(
        printed(&output),
        [json!([
            [
                ["pointerdown", "whole-label"],
                ["click", "whole-label"],
                ["click", "whole"]
            ],
            true
        ])]
    );
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

/// A page 3200 px wide and tall, its meta viewport `meta` (none when empty): a button far right, one
/// far down, a checkbox hidden in its label far right and down (pressed on its styled track), a
/// button in the middle, and one far down to hover, which records the pointer coming over it. On
/// a phone at its own scale (`initial-scale=1`) the page widens the layout viewport to 1560x3376, a
/// quarter's zoom, and the screen shows 390x844 of it, scrolled within it apart from the page; at
/// `initial-scale=2` it shows 195x422. With no meta viewport, or one without a scale, the phone
/// shows the whole layout viewport, zoomed out to a quarter.
fn wide(meta: &str) -> Vec<u8> {
    let meta = if meta.is_empty() {
        String::new()
    } else {
        format!(r#"<meta name="viewport" content="{meta}">"#)
    };
    format!(
        r##"<!doctype html>
<meta charset="utf-8">
{meta}
<style>
  body {{ margin: 0; font: 16px/1.4 sans-serif; width: 3200px; height: 3200px; position: relative; }}
  .spot {{ position: absolute; }}
  button {{ width: 80px; height: 30px; }}
  .toggle {{ position: relative; display: inline-flex; gap: 8px; align-items: center; }}
  .hidden {{ position: absolute; left: 12px; top: 8px; width: 1px; height: 1px; margin: 0; padding: 0; border: 0; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; }}
  .track {{ display: inline-block; width: 40px; height: 24px; background: #ccc; }}
</style>
<div class="spot" style="left: 1200px; top: 300px"><button id="right" type="button">far right</button></div>
<div class="spot" style="left: 20px; top: 2600px"><button id="down" type="button">far down</button></div>
<div class="spot" style="left: 2600px; top: 1800px"><label class="toggle"><input type="checkbox" id="toggle" class="hidden"><span class="track" id="track"></span> far right and down</label></div>
<div class="spot" style="left: 300px; top: 600px"><button id="mid" type="button">mid</button></div>
<div class="spot" style="left: 900px; top: 2900px"><button id="hovered" type="button">hover me</button></div>
<script>
  window.came = [];
  document.getElementById('hovered').addEventListener('pointerover', event => came.push(event.target.id));
  window.view = () => {{ const v = visualViewport; return {{inner: [innerWidth, innerHeight], screen: [v.offsetLeft, v.offsetTop, v.width, v.height], scale: v.scale}}; }};
</script>
{POINTED}"##
    )
    .into_bytes()
}

/// `symphony shot` on [`wide`] with `meta` and `steps`, then its one evaluation: the presses and
/// clicks the page recorded, whether the checkbox is checked, what the hover recorded and the
/// view as the last step left it. `None` without a Chrome.
fn on_wide(name: &str, meta: &str, steps: &[&str]) -> Option<Value> {
    let eval = "[landed, document.getElementById('toggle').checked, came, view()]";
    let (output, out, _requests, _scratch) =
        on_page(name, &wide(meta), &[steps, &["--eval", eval][..]].concat())?;
    assert!(output.status.success(), "{}", said(&output));
    let lines = printed(&output);
    let [after] = lines.as_slice() else {
        panic!("one evaluation: {}", said(&output));
    };
    assert!(Picture::read(&out).width > 0);
    Some(after.clone())
}

/// Each press and click [`wide`] records for its far right button, its far down button and its
/// checkbox's label, clicked in that order.
fn far_presses() -> Value {
    json!([
        ["pointerdown", "right"],
        ["click", "right"],
        ["pointerdown", "down"],
        ["click", "down"],
        ["pointerdown", "track"],
        ["click", "track"],
        ["click", "toggle"]
    ])
}

#[test]
fn a_phone_click_on_a_page_wider_than_its_screen_lands_where_the_screen_shows_its_target() {
    let Some(after) = on_wide(
        "wide-phone",
        "width=device-width, initial-scale=1",
        &[
            "--phone", "--click", "#right", "--click", "#down", "--click", "#toggle", "--hover",
            "#hovered",
        ],
    ) else {
        return;
    };
    // The case at all: the screen shows a part of a larger layout viewport (1560x3376 on this
    // Chrome), scrolled within it.
    assert_eq!(after[3]["scale"], json!(1), "{after}");
    let screen = rect(&after[3]["screen"]);
    assert_eq!(screen[2..], [390.0, 844.0], "{after}");
    let inner = &after[3]["inner"];
    assert!(
        inner[0].as_f64().unwrap() > 390.0 && inner[1].as_f64().unwrap() > 844.0,
        "{after}"
    );
    assert!(
        screen[1] > 0.0,
        "the screen moved down within the layout viewport: {after}"
    );
    // Each press landed on its target, far right, far down and on the label's track, which checked
    // the box; the hover came over its button. Main pressed each on the page's root, exit 0.
    assert_eq!(after[0], far_presses(), "{after}");
    assert_eq!(after[1], json!(true), "{after}");
    assert_eq!(after[2], json!(["hovered"]), "{after}");
}

#[test]
fn a_phone_click_on_a_page_zoomed_in_lands_on_its_target_on_the_screen_moved_within_the_page() {
    let Some(after) = on_wide(
        "wide-zoomed",
        "width=device-width, initial-scale=2",
        &["--phone", "--click", "#mid", "--click", "#down"],
    ) else {
        return;
    };
    // Zoomed in twice: the screen shows 195x422 of the page, scrolled away from its corner.
    assert_eq!(after[3]["scale"], json!(2), "{after}");
    let screen = rect(&after[3]["screen"]);
    assert_eq!(screen[2..], [195.0, 422.0], "{after}");
    assert!(screen[1] > 0.0, "{after}");
    assert_eq!(
        after[0],
        json!([
            ["pointerdown", "mid"],
            ["click", "mid"],
            ["pointerdown", "down"],
            ["click", "down"]
        ]),
        "{after}"
    );
}

#[test]
fn a_phone_with_no_meta_viewport_and_a_desktop_still_click_where_they_did() {
    for (name, view) in [
        ("wide-bare-phone", &["--phone"][..]),
        ("wide-desktop", &["--size", "800x600"][..]),
    ] {
        let steps = [
            "--click", "#right", "--click", "#down", "--click", "#toggle", "--hover", "#hovered",
        ];
        let Some(after) = on_wide(name, "", &[view, &steps[..]].concat()) else {
            return;
        };
        // No part of the layout viewport off the screen: the phone zoomed out to show it whole, the
        // desktop's window being it.
        let screen = rect(&after[3]["screen"]);
        assert_eq!(screen[..2], [0.0, 0.0], "{name}: {after}");
        assert_eq!(after[0], far_presses(), "{name}: {after}");
        assert_eq!(after[1], json!(true), "{name}: {after}");
        assert_eq!(after[2], json!(["hovered"]), "{name}: {after}");
    }
}

/// Labels whose control comes after them in the document, which Chrome hovers with its hovered
/// label: a label naming a text field by `for`, a span inside a label around its checkbox, a link
/// inside a label naming a checkbox by `for`, and far right, past a phone's screen on a page as wide
/// as [`wide`], a label naming a checkbox by `for`. With `meta` as the page's meta viewport.
fn followed(meta: &str) -> Vec<u8> {
    let meta = if meta.is_empty() {
        String::new()
    } else {
        format!(r#"<meta name="viewport" content="{meta}">"#)
    };
    format!(
        r##"<!doctype html>
<meta charset="utf-8">
{meta}
<style>
  body {{ margin: 0; font: 16px/1.4 sans-serif; width: 3200px; height: 3200px; position: relative; }}
  .spot {{ position: absolute; }}
</style>
<div class="spot" style="left: 20px; top: 40px"><label for="email" id="lfor">Email</label> <input id="email"></div>
<div class="spot" style="left: 20px; top: 120px"><label><span id="lspan">Accept the terms</span> <input type="checkbox" id="accept"></label></div>
<div class="spot" style="left: 20px; top: 200px"><label for="agree">I accept the <a id="terms" href="#terms">terms</a></label> <input type="checkbox" id="agree"></div>
<div class="spot" style="left: 1200px; top: 300px"><label for="far" id="lfar">far right</label> <input type="checkbox" id="far"></div>
{POINTED}"##
    )
    .into_bytes()
}

#[test]
fn a_click_or_a_hover_on_a_label_whose_control_follows_it_lands_on_the_label_on_a_desktop_and_a_phone()
 {
    // Each label's target hovered, then clicked: hovered once the hover is done, and the click
    // landing on the label, which clicks its control, or on the link, which a label's activation
    // leaves alone. A pointer read as the last element the page's hover matches was on the control
    // each label hovers, and each step was refused.
    let hovered = |id: &str| format!("document.getElementById('{id}').matches(':hover')");
    let ids = ["lfor", "lspan", "terms", "lfar"];
    let checks = ids.map(hovered);
    let mut steps = Vec::new();
    for (id, check) in ids.iter().zip(&checks) {
        let css = format!("#{id}");
        steps.extend(
            ["--hover", &css, "--eval", check, "--click", &css].map(|step| step.to_string()),
        );
    }
    let after = "[landed, ['accept', 'agree', 'far'].map(id => document.getElementById(id).checked), \
                 location.hash, visualViewport.offsetLeft]";
    for (name, meta, view) in [
        ("followed-desktop", "", &[][..]),
        (
            "followed-phone",
            "width=device-width, initial-scale=1",
            &["--phone"][..],
        ),
    ] {
        let steps: Vec<&str> = view
            .iter()
            .copied()
            .chain(steps.iter().map(String::as_str))
            .chain(["--eval", after])
            .collect();
        let Some((output, out, _requests, _scratch)) = on_page(name, &followed(meta), &steps)
        else {
            return;
        };
        assert!(output.status.success(), "{name}: {}", said(&output));
        let lines = printed(&output);
        let [lfor, lspan, terms, lfar, after] = lines.as_slice() else {
            panic!("{name}: five evaluations: {}", said(&output));
        };
        assert_eq!(
            [lfor, lspan, terms, lfar],
            [&json!(true); 4],
            "{name}: {}",
            said(&output)
        );
        assert_eq!(
            after[0],
            json!([
                ["pointerdown", "lfor"],
                ["click", "lfor"],
                ["click", "email"],
                ["pointerdown", "lspan"],
                ["click", "lspan"],
                ["click", "accept"],
                ["pointerdown", "terms"],
                ["click", "terms"],
                ["pointerdown", "lfar"],
                ["click", "lfar"],
                ["click", "far"]
            ]),
            "{name}: {after}"
        );
        // The checkbox inside the label and the one far right checked, the one whose label holds
        // the link left alone, the link followed.
        assert_eq!(after[1], json!([true, false, true]), "{name}: {after}");
        assert_eq!(after[2], json!("#terms"), "{name}: {after}");
        assert!(Picture::read(&out).width > 0);
        if name == "followed-phone" {
            // The far label was pressed with the screen moved right within the page, to show it.
            assert!(after[3].as_f64().unwrap() > 0.0, "{name}: {after}");
        }
    }
}

/// Three frames, each a button filling it that tells the page of the pointer coming over it and
/// of its click (`messages`, which `heard` waits on): one of the page's own, which shares its
/// process; one sandboxed without the page's origin, as the client draws its document mockups,
/// which Chrome runs in a process of its own; and the same in a component's closed shadow root,
/// which the page's hit test sees only as its host. Each frame's document takes the move over it,
/// so the page hears none; the sandboxed ones leave their element and its host unhovered in the
/// page too. With `meta` as the page's meta viewport.
fn framed(meta: &str) -> Vec<u8> {
    let meta = if meta.is_empty() {
        String::new()
    } else {
        format!(r#"<meta name="viewport" content="{meta}">"#)
    };
    let button = |which: &str| {
        format!(
            "<style>body {{ margin: 0 }} button {{ width: 100%; height: 100vh }}</style><button \
             type='button' onpointerover='parent.postMessage(&quot;over {which}&quot;, &quot;*&quot;)' \
             onclick='parent.postMessage(&quot;pressed {which}&quot;, &quot;*&quot;)'>{which}</button>"
        )
    };
    format!(
        r##"<!doctype html>
<meta charset="utf-8">
{meta}
<style>
  body {{ margin: 0; padding: 20px; font: 16px/1.4 sans-serif; }}
  iframe, x-closed {{ display: block; border: 0; margin: 0 0 20px; width: 220px; }}
</style>
<iframe id="own" width="220" height="50" srcdoc="{own}"></iframe>
<iframe id="sandboxed" sandbox="allow-scripts" width="220" height="50" srcdoc="{sandboxed}"></iframe>
<x-closed id="closed" data-doc="{closed}"></x-closed>
<script>
  window.messages = [];
  addEventListener('message', event => messages.push(event.data));
  // Once every one of `notes` has come, the messages so far, sorted: frames in processes of their
  // own post them in no order the page can rely on.
  window.heard = (...notes) => new Promise(resolve => {{
    const check = () => notes.every(note => messages.includes(note)) ? resolve(messages.slice().sort()) : setTimeout(check, 10);
    check();
  }});
  customElements.define('x-closed', class extends HTMLElement {{
    constructor() {{
      super();
      const frame = document.createElement('iframe');
      frame.sandbox = 'allow-scripts';
      frame.width = 220;
      frame.height = 50;
      frame.style.cssText = 'display: block; border: 0';
      frame.srcdoc = this.dataset.doc;
      this.attachShadow({{mode: 'closed'}}).append(frame);
    }}
  }});
</script>
{POINTED}"##,
        own = button("own"),
        sandboxed = button("sandboxed"),
        closed = button("closed"),
    )
    .into_bytes()
}

#[test]
fn a_click_or_a_hover_into_a_frame_sandboxed_in_a_closed_root_or_the_pages_own_reaches_its_button()
{
    // Each frame hovered, then clicked: the pointer came over its button, and the click pressed
    // it, on a desktop and a phone. The page hears no move over any, and the sandboxed frames are
    // not hovered in it: the pointer's place inside a frame goes unconfirmed, the one in a closed
    // root found by the browser's own hit, and the step goes on as aimed. Each read waits for the
    // notes it expects, the frames posting them from processes of their own.
    let steps = [
        "--hover",
        "#own",
        "--eval",
        "heard('over own')",
        "--click",
        "#own",
        "--hover",
        "#sandboxed",
        "--eval",
        "heard('pressed own', 'over sandboxed')",
        "--click",
        "#sandboxed",
        "--hover",
        "#closed",
        "--eval",
        "heard('pressed sandboxed', 'over closed')",
        "--click",
        "#closed",
        "--eval",
        "heard('pressed closed').then(notes => [notes, landed])",
    ];
    for (name, meta, view) in [
        ("framed-desktop", "", &[][..]),
        (
            "framed-phone",
            "width=device-width, initial-scale=1",
            &["--phone"][..],
        ),
    ] {
        let Some((output, out, _requests, _scratch)) =
            on_page(name, &framed(meta), &[view, &steps[..]].concat())
        else {
            return;
        };
        assert!(output.status.success(), "{name}: {}", said(&output));
        assert_eq!(
            printed(&output),
            [
                json!(["over own"]),
                json!(["over own", "over sandboxed", "pressed own"]),
                json!([
                    "over closed",
                    "over own",
                    "over sandboxed",
                    "pressed own",
                    "pressed sandboxed"
                ]),
                // The page itself saw no press: each went into its frame.
                json!([
                    [
                        "over closed",
                        "over own",
                        "over sandboxed",
                        "pressed closed",
                        "pressed own",
                        "pressed sandboxed"
                    ],
                    []
                ]),
            ],
            "{name}: {}",
            said(&output)
        );
        assert!(Picture::read(&out).width > 0);
    }
}

/// The colour a framed document paints itself, by a stylesheet of its own.
const FRAMED_GREEN: [u8; 4] = [0, 160, 0, 255];

/// Where [`holding_a_frame`] draws its frame, `[x, y, width, height]` in CSS pixels.
const FRAME_BOX: [f64; 4] = [200.0, 0.0, 300.0, 200.0];

/// A page beside whose button, `#outside` (it counts its clicks in `window.clicks`), is an iframe
/// at [`FRAME_BOX`] with `attributes` (its `src` among them).
fn holding_a_frame(attributes: &str) -> Vec<u8> {
    let [x, y, width, height] = FRAME_BOX;
    format!(
        r#"<!doctype html>
<meta charset="utf-8">
<style>
  body {{ margin: 0; }}
  #outside {{ position: absolute; left: 0; top: 0; width: 120px; height: 40px; }}
  iframe {{ position: absolute; left: {x}px; top: {y}px; width: {width}px; height: {height}px; border: 0; }}
</style>
<button id="outside" type="button" onclick="window.clicks = (window.clicks || 0) + 1">outside</button>
<iframe {attributes}></iframe>"#
    )
    .into_bytes()
}

/// Serve a framed document at `path` on `site`, linking the stylesheet at `style`, which, unless
/// `site` holds it, paints the document [`FRAMED_GREEN`].
fn serve_framed(site: &site::Site, path: &str, style: &str) {
    site.set_file(
        path,
        format!(r#"<!doctype html><link rel="stylesheet" href="{style}"><p>framed</p>"#)
            .into_bytes(),
        "text/html",
    );
    site.set_file(
        style,
        b"html { background: rgb(0, 160, 0); }".to_vec(),
        "text/css",
    );
}

#[test]
fn a_page_with_a_frame_chrome_runs_in_a_process_of_its_own_is_shot_drawn_and_clicked_outside_it() {
    // A frame sandboxed without the page's origin, as the client draws a document's mockup, and a
    // frame of another site (the other stand-in, as `localhost`): Chrome runs each in a process of
    // its own, which takes the answer to its document. The shot is taken once each frame's
    // document and its stylesheet have loaded, the frame green in the picture, and a click on a
    // control outside the frame lands on it.
    let other = site();
    serve_framed(&other, "/framed.html", "/framed.css");
    let cross = format!("http://localhost:{}/framed.html", other.addr.port());
    for (name, attributes, framing) in [
        (
            "sandboxed-src",
            r#"sandbox="allow-scripts" src="/framed.html""#.to_string(),
            None,
        ),
        ("cross-site", format!(r#"src="{cross}""#), Some(&other)),
    ] {
        for steps in [
            &[][..],
            &["--click", "#outside", "--eval", "window.clicks"][..],
        ] {
            let Some(chrome) = chrome::find() else {
                eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
                return;
            };
            let site = site();
            serve_framed(&site, "/framed.html", "/framed.css");
            site.set_file("/page.html", holding_a_frame(&attributes), "text/html");
            let scratch = Scratch::new(name);
            let out = scratch.join("framed.png");
            let output = shot(
                &chrome,
                &[
                    &[
                        site.url("/page.html").as_str(),
                        "--out",
                        out.to_str().unwrap(),
                    ][..],
                    steps,
                ]
                .concat(),
            )
            .output()
            .unwrap();
            assert!(
                output.status.success(),
                "{name} {steps:?}: {}",
                said(&output)
            );
            let clicked = if steps.is_empty() {
                vec![]
            } else {
                vec![json!(1)]
            };
            assert_eq!(printed(&output), clicked, "{name} {steps:?}");
            let served = framing.unwrap_or(&site).requests();
            for path in ["/framed.html", "/framed.css"] {
                assert!(
                    served.iter().any(|served| served == path),
                    "{name}: {path} not asked for: {served:?}"
                );
            }
            let picture = Picture::read(&out);
            let [x, y, width, height] = FRAME_BOX;
            let middle = ((y + height / 2.0) as usize) * picture.width + (x + width / 2.0) as usize;
            assert_eq!(
                picture.rgba[middle], FRAMED_GREEN,
                "{name} {steps:?}: the frame is not drawn in the picture"
            );
        }
    }
}

#[test]
fn a_board_whose_next_card_shows_a_mockup_is_shot_with_the_mockup_drawn() {
    // The client draws a document's mockup as a frame sandboxed without the page's origin, which
    // Chrome runs in a process of its own: here the board's next card shows one, in view.
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    let site = site();
    let mut board: Value = serde_json::from_str(&super::common::fixture("board.json")).unwrap();
    board["next"][0]["pictures"] = json!([{"src": "/mockup.html", "caption": "Mockup"}]);
    site.set_board(&board.to_string());
    serve_framed(&site, "/mockup.html", "/mockup.css");
    let scratch = Scratch::new("mockup");
    let out = scratch.join("mockup.png");
    let output = shot(
        &chrome,
        &[
            &site.url("/"),
            "--out",
            out.to_str().unwrap(),
            "--wait-for",
            ".hero .mockup iframe",
            "--eval",
            "(() => { const b = document.querySelector('.hero .mockup iframe').getBoundingClientRect(); \
             return [b.x, b.y, b.width, b.height]; })()",
        ],
    )
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", said(&output));
    let [frame] = printed(&output).try_into().unwrap();
    let [x, y, width, height] = rect(&frame);
    assert!(width > 0.0 && height > 0.0, "{frame}");
    for path in ["/mockup.html", "/mockup.css"] {
        assert!(
            site.requests().iter().any(|served| served == path),
            "{path} not asked for: {:?}",
            site.requests()
        );
    }
    let picture = Picture::read(&out);
    let middle = ((y + height / 2.0) as usize) * picture.width + (x + width / 2.0) as usize;
    assert_eq!(
        picture.rgba[middle], FRAMED_GREEN,
        "the mockup at {frame} is not drawn in the picture"
    );
}

/// Resolves once the shell has begun warming the persistent modules (shell/shell.ts, `warm`): it
/// does so once the page has drawn its data and gone quiet, as it starts reading ahead the links
/// on the screen (`ahead`), so the shot's settle after it spans the page's reads ahead. Neither
/// warming nor this wait reads this device's store.
const WARMING: &str = "new Promise(resolve => { const look = () => \
     document.querySelector('link[rel=prefetch]') ? resolve(true) : setTimeout(look, 100); look(); })";

/// The home's links to the project's pages, by their words.
const HOME_LINKS: &str =
    "[...document.querySelectorAll('.dash-links a')].map(link => link.textContent.trim())";

/// A new project's home, the real server's, as `symphony init` leaves the project: no
/// docs/DECISIONS.md yet. The shot waits until the page has gone quiet and starts its reads ahead
/// (of its next card's document, the links before it first), then is drawn and saved: the home
/// links no register, so nothing reads one ahead. Before, the home linked docs/DECISIONS.md and
/// read it ahead, the server answered 404, and that answer, its body never read, stayed in flight
/// in Chrome's eyes until the shot's timeout. With the file there, the home links it and is shot
/// as well.
#[test]
fn a_new_projects_home_without_a_register_links_none_and_is_shot_drawn() {
    const LATER: &str = "# Later\n\n- [ ] **alpha** — First question.\n";
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    for (documents, register) in [
        (&[("docs/LATER.md", LATER)][..], false),
        (
            &[
                ("docs/LATER.md", LATER),
                ("docs/DECISIONS.md", "# Decision register\n"),
            ][..],
            true,
        ),
    ] {
        let site = super::restored::Native::start_committed(None, "main", documents, None);
        let scratch = Scratch::new(if register {
            "registered"
        } else {
            "unregistered"
        });
        let out = scratch.join("home.png");
        let output = shot(
            &chrome,
            &[
                &site.url("/"),
                "--out",
                out.to_str().unwrap(),
                "--wait-for",
                ".hero .docchip",
                "--eval",
                WARMING,
                "--eval",
                HOME_LINKS,
                "--report",
            ],
        )
        .output()
        .unwrap();
        assert!(
            output.status.success(),
            "register {register}: {}",
            said(&output)
        );
        let [warming, links, report] = printed(&output).try_into().unwrap();
        assert_eq!(warming, json!(true));
        let links: Vec<&str> = links
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(links.contains(&"Full queue"), "{links:?}");
        assert_eq!(
            links.contains(&"Decision register"),
            register,
            "the register linked only once its file is there: {links:?}"
        );
        assert!(
            !report.to_string().contains("DECISIONS.md"),
            "no read of a register failed: {report}"
        );
        let picture = Picture::read(&out);
        assert!(
            picture.colours([0.0, 0.0, 1440.0, 900.0], 1.0) > 16,
            "register {register}: the home is not drawn in the picture"
        );
    }
}

/// A page's icon, given in the page, so that Chrome asks the stand-in for no `/favicon.ico` once
/// the page has loaded: its 404 would be a failed request of the page's own.
const NO_ICON: &str = r#"<link rel="icon" href="data:,">"#;

/// A document whose first half, all the stand-in sends of it while it stalls its body, posts
/// `name` to its parent: the script comes first, then text gzip cannot shrink much, so the half
/// sent holds the script whole.
fn telling_its_parent(name: &str) -> Vec<u8> {
    let mut state = 0x2545_f491_u32;
    let noise: String = (0..32 * 1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            char::from(b'a' + (state % 26) as u8)
        })
        .collect();
    format!(
        r#"<!doctype html><meta charset="utf-8"><script>parent.postMessage('{name}', '*')</script><p>{noise}</p>"#
    )
    .into_bytes()
}

/// A dedicated worker's script that fetches `path` and posts its parent the answer's text, having
/// posted `started` once the fetch is asked for.
fn fetching(path: &str) -> Vec<u8> {
    format!(
        "const asked = fetch('{path}'); postMessage('started'); \
         asked.then(answer => answer.text()).then(text => postMessage(text));"
    )
    .into_bytes()
}

/// A shared worker's script that, for each page that connects, fetches `path` and posts the answer's
/// text on the page's port, having posted `started` once the fetch is asked for.
fn shared_fetching(path: &str) -> Vec<u8> {
    format!(
        "onconnect = event => {{ const port = event.ports[0]; const asked = fetch('{path}'); \
         port.postMessage('started'); \
         asked.then(answer => answer.text()).then(text => port.postMessage(text)); }};"
    )
    .into_bytes()
}

#[test]
fn a_frame_removed_while_its_document_still_arrives_is_not_waited_for() {
    // A frame of another site whose document the stand-in stalls after its first half, which tells
    // the page; the page then removes the frame and marks its own part drawn. Nothing the page
    // holds still loads, so the shot is taken.
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    let other = site();
    let site = site();
    other.set_file("/going.html", telling_its_parent("going"), "text/html");
    other.stall_body("/going.html", true);
    site.set_file(
        "/removing.html",
        format!(
            r#"<!doctype html>
<meta charset="utf-8">
{NO_ICON}
<p id="loading" data-ready="loading">a frame on its way out</p>
<iframe id="going" src="http://localhost:{}/going.html"></iframe>
<script>
  addEventListener('message', () => {{
    document.getElementById('going').remove();
    document.getElementById('loading').remove();
  }});
</script>"#,
            other.addr.port()
        )
        .into_bytes(),
        "text/html",
    );
    let scratch = Scratch::new("removing-frame");
    let out = scratch.join("removing.png");
    let output = shot(
        &chrome,
        &[&site.url("/removing.html"), "--out", out.to_str().unwrap()],
    )
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", said(&output));
    assert!(
        other.requests().iter().any(|path| path == "/going.html"),
        "the frame's document was not asked for: {:?}",
        other.requests()
    );
    assert!(Picture::read(&out).width > 0);
}

/// The colour at the middle of the rectangle `[x, y, width, height]` (CSS pixels) of a picture
/// taken at one device pixel to the CSS pixel.
fn middle_of(picture: &Picture, [x, y, width, height]: [f64; 4]) -> [u8; 4] {
    picture.rgba[((y + height / 2.0) as usize) * picture.width + (x + width / 2.0) as usize]
}

#[test]
fn a_frame_whose_parent_goes_back_into_the_pages_process_mid_load_is_not_waited_for() {
    // A frame of another site (the other stand-in, as `localhost`) holding a frame of a third
    // (`c.localhost`, the same stand-in), each of which Chrome runs in a process of its own. The
    // stand-in stalls the inner frame's document after its first half, which tells its parent, and
    // the parent then goes on to a document of the page's own site, which Chrome loads in the
    // page's process, taking the inner frame away with it. Nothing the page holds still loads, so
    // the shot is taken, the frame drawn with the document it went on to.
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    let other = site();
    let site = site();
    serve_framed(&site, "/arrived.html", "/arrived.css");
    let arrived = site.url("/arrived.html");
    let port = other.addr.port();
    other.set_file("/held.html", telling_its_parent("held"), "text/html");
    other.stall_body("/held.html", true);
    other.set_file(
        "/holding.html",
        format!(
            r#"<!doctype html><meta charset="utf-8"><script>addEventListener('message', () => {{ location = '{arrived}'; }});</script>
<p>holding</p><iframe src="http://c.localhost:{port}/held.html"></iframe>"#
        )
        .into_bytes(),
        "text/html",
    );
    site.set_file(
        "/nesting.html",
        holding_a_frame(&format!(r#"src="http://localhost:{port}/holding.html""#)),
        "text/html",
    );
    let scratch = Scratch::new("nesting-frames");
    let out = scratch.join("nesting.png");
    let output = shot(
        &chrome,
        &[
            &site.url("/nesting.html"),
            "--out",
            out.to_str().unwrap(),
            "--eval",
            "document.querySelector('iframe').contentDocument.URL",
        ],
    )
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", said(&output));
    assert_eq!(printed(&output), vec![json!(arrived)]);
    assert!(
        other.requests().iter().any(|path| path == "/held.html"),
        "the inner frame's document was not asked for: {:?}",
        other.requests()
    );
    assert_eq!(
        middle_of(&Picture::read(&out), FRAME_BOX),
        FRAMED_GREEN,
        "the frame is not drawn with the document it went on to"
    );
}

#[test]
fn a_frame_sent_on_while_its_document_still_arrives_waits_for_the_next_document_alone() {
    // Four frames the page sends on while the stand-in stalls each one's document after its first
    // half, which tells the page: one of the page's own site to another of its own; one of another
    // site (as `localhost`), which Chrome runs in a process of its own, to another of that site;
    // one of another site to one of the page's own, which Chrome loads in the page's process; and
    // one of the page's own to one of another site. No frame holds its first document any more,
    // never finished, so the shot is taken once each next document has loaded, each frame drawn
    // with it.
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    let other = site();
    let site = site();
    let away = format!("http://localhost:{}", other.addr.port());
    let frames = [
        ("near", &site, &site, [0.0, 0.0, 200.0, 150.0]),
        ("far", &other, &other, [220.0, 0.0, 200.0, 150.0]),
        ("back", &other, &site, [0.0, 170.0, 200.0, 150.0]),
        ("out", &site, &other, [220.0, 170.0, 200.0, 150.0]),
    ];
    let address = |on: &site::Site, path: &str| {
        if std::ptr::eq(on, &other) {
            format!("{away}{path}")
        } else {
            site.url(path)
        }
    };
    let mut next = serde_json::Map::new();
    let mut held = String::new();
    for (name, first, then, [x, y, ..]) in frames {
        let stalled = format!("/{name}.html");
        first.set_file(&stalled, telling_its_parent(name), "text/html");
        first.stall_body(&stalled, true);
        serve_framed(then, &format!("/{name}-next.html"), "/next.css");
        next.insert(
            name.into(),
            json!(address(then, &format!("/{name}-next.html"))),
        );
        held += &format!(
            r#"<iframe id="{name}" src="{}" style="left: {x}px; top: {y}px"></iframe>"#,
            address(first, &stalled)
        );
    }
    site.set_file(
        "/sending.html",
        format!(
            r#"<!doctype html>
<meta charset="utf-8">
{NO_ICON}
<style>
  body {{ margin: 0; }}
  iframe {{ position: absolute; width: 200px; height: 150px; border: 0; }}
</style>
<script>
  const next = {};
  addEventListener('message', event => {{ document.getElementById(event.data).src = next[event.data]; }});
</script>
{held}"#,
            Value::Object(next)
        )
        .into_bytes(),
        "text/html",
    );
    let scratch = Scratch::new("sending-frames");
    let out = scratch.join("sending.png");
    let output = shot(
        &chrome,
        &[&site.url("/sending.html"), "--out", out.to_str().unwrap()],
    )
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", said(&output));
    let picture = Picture::read(&out);
    for (name, first, then, frame) in frames {
        for (on, path) in [
            (first, format!("/{name}.html")),
            (then, format!("/{name}-next.html")),
        ] {
            assert!(
                on.requests().contains(&path),
                "{name}: {path} not asked for: {:?}",
                on.requests()
            );
        }
        assert_eq!(
            middle_of(&picture, frame),
            FRAMED_GREEN,
            "{name}: the frame is not drawn with the document it was sent on to"
        );
    }
}

#[test]
fn a_dialog_the_page_or_a_frame_opens_is_dismissed_and_the_report_names_each() {
    // The page opens an alert as it loads; a frame of its own site asks to confirm, and a frame of
    // another site, which Chrome runs in a process of its own, prompts; each frame posts the page
    // its answer, and the page marks its own part drawn once both have come. Each dialog is
    // dismissed as it opens, the confirm and the prompt answered as by their Cancel, so the page
    // goes on and is shot; a step's own confirm is answered the same way, and the report names all
    // four, each by its kind, its text and the document that opened it.
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    let other = site();
    let site = site();
    let away = format!("http://localhost:{}", other.addr.port());
    site.set_file(
        "/confirming.html",
        b"<!doctype html><script>parent.postMessage(['confirm', confirm('in the frame?')], '*')</script>"
            .to_vec(),
        "text/html",
    );
    other.set_file(
        "/prompting.html",
        b"<!doctype html><script>parent.postMessage(['prompt', prompt('Name?', 'nobody')], '*')</script>"
            .to_vec(),
        "text/html",
    );
    site.set_file(
        "/asking.html",
        format!(
            r#"<!doctype html>
<meta charset="utf-8">
{NO_ICON}
<p id="loading" data-ready="loading">asking</p>
<script>
  window.answers = {{}};
  addEventListener('message', event => {{
    const [kind, answer] = event.data;
    window.answers[kind] = answer;
    if ('confirm' in window.answers && 'prompt' in window.answers) document.getElementById('loading').remove();
  }});
  alert('on the page');
</script>
<iframe src="/confirming.html"></iframe>
<iframe src="{away}/prompting.html"></iframe>"#
        )
        .into_bytes(),
        "text/html",
    );
    let scratch = Scratch::new("asking");
    let out = scratch.join("asking.png");
    let url = site.url("/asking.html");
    let output = shot(
        &chrome,
        &[
            &url,
            "--out",
            out.to_str().unwrap(),
            "--eval",
            "window.answers",
            "--eval",
            "confirm('asked by a step')",
            "--report",
        ],
    )
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", said(&output));
    let [answers, asked, report] = printed(&output).try_into().unwrap();
    assert_eq!(answers, json!({"confirm": false, "prompt": null}));
    assert_eq!(asked, json!(false));
    let dialogs: BTreeSet<String> = report["dialogs"]
        .as_array()
        .unwrap_or_else(|| panic!("no dialogs: {report}"))
        .iter()
        .map(Value::to_string)
        .collect();
    let expected: BTreeSet<String> = [
        json!({"type": "alert", "message": "on the page", "url": url}),
        json!({"type": "confirm", "message": "in the frame?", "url": site.url("/confirming.html")}),
        json!({"type": "prompt", "message": "Name?", "url": format!("{away}/prompting.html")}),
        json!({"type": "confirm", "message": "asked by a step", "url": url}),
    ]
    .iter()
    .map(Value::to_string)
    .collect();
    assert_eq!(dialogs, expected, "{report}");
    assert!(Picture::read(&out).width > 0);
}

#[test]
fn a_page_with_workers_is_shot_once_they_have_answered_never_waiting_on_a_shared_workers_loads() {
    // A dedicated worker of the page and one of a frame of another site, which Chrome runs in a
    // process of its own, each fetching what it posts back, a worker whose script is missing, and
    // a shared worker whose fetch the stand-in never answers: the page marks its own part drawn
    // once the dedicated workers have answered, the missing one has failed and the shared one has
    // started. The shot is taken with each answer, waiting on no worker's script as on a frame's
    // document, nor on the shared worker's fetch: Chrome runs a shared worker in the browser,
    // where the shot does not follow it.
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    let other = site();
    let site = site();
    let away = format!("http://localhost:{}", other.addr.port());
    site.set_file("/worker.js", fetching("/worker-data"), "text/javascript");
    site.set_file("/worker-data", b"from the worker".to_vec(), "text/plain");
    site.set_file(
        "/shared.js",
        shared_fetching("/shared-data"),
        "text/javascript",
    );
    site.hold_route("/shared-data", true);
    other.set_file(
        "/frame-worker.js",
        fetching("/frame-data"),
        "text/javascript",
    );
    other.set_file(
        "/frame-data",
        b"from the frame's worker".to_vec(),
        "text/plain",
    );
    other.set_file(
        "/working.html",
        b"<!doctype html><script>new Worker('/frame-worker.js').onmessage = event => { \
          if (event.data !== 'started') parent.postMessage(event.data, '*'); };</script>"
            .to_vec(),
        "text/html",
    );
    site.set_file(
        "/workers.html",
        format!(
            r#"<!doctype html>
<meta charset="utf-8">
{NO_ICON}
<p id="loading" data-ready="loading">workers at work</p>
<script>
  window.answers = {{}};
  const answered = (who, what) => {{
    window.answers[who] = what;
    if (Object.keys(window.answers).length === 4) document.getElementById('loading').remove();
  }};
  new Worker('/worker.js').onmessage = event => {{
    if (event.data !== 'started') answered('dedicated', event.data);
  }};
  new SharedWorker('/shared.js').port.onmessage = event => {{
    if (!('shared' in window.answers)) answered('shared', event.data);
  }};
  new Worker('/no-such-worker.js').onerror = () => answered('missing', 'failed');
  addEventListener('message', event => answered('framed', event.data));
</script>
<iframe src="{away}/working.html"></iframe>"#
        )
        .into_bytes(),
        "text/html",
    );
    let scratch = Scratch::new("workers");
    let out = scratch.join("workers.png");
    let output = shot(
        &chrome,
        &[
            &site.url("/workers.html"),
            "--out",
            out.to_str().unwrap(),
            "--eval",
            "window.answers",
        ],
    )
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", said(&output));
    assert_eq!(
        printed(&output),
        vec![json!({
            "dedicated": "from the worker",
            "shared": "started",
            "framed": "from the frame's worker",
            "missing": "failed",
        })]
    );
    assert!(Picture::read(&out).width > 0);
}

#[test]
fn a_dedicated_workers_script_answered_late_is_waited_for_and_what_its_worker_says_is_drawn() {
    // A dedicated worker whose script the stand-in holds, its answer not begun, for a second and
    // more after it is asked for: Chrome attaches the worker only once that answer starts, so until
    // then its script is a request of the page's in flight, and the shot waits for it. Answered,
    // the worker fetches what it posts back and the page paints a box green with it, as the
    // picture shows.
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    let site = site();
    site.set_file("/late-worker.js", fetching("/late-data"), "text/javascript");
    site.hold_route("/late-worker.js", true);
    site.set_file("/late-data", b"from the late worker".to_vec(), "text/plain");
    site.set_file(
        "/late.html",
        format!(
            r#"<!doctype html>
<meta charset="utf-8">
{NO_ICON}
<style>body {{ margin: 0; }} #answer {{ width: 200px; height: 200px; }}</style>
<div id="answer"></div>
<script>
  new Worker('/late-worker.js').onmessage = event => {{
    if (event.data === 'started') return;
    const answer = document.getElementById('answer');
    answer.dataset.said = event.data;
    answer.style.background = 'rgb(0, 160, 0)';
  }};
</script>"#
        )
        .into_bytes(),
        "text/html",
    );
    let scratch = Scratch::new("late-worker");
    let out = scratch.join("late.png");
    let mut running = shot(
        &chrome,
        &[
            &site.url("/late.html"),
            "--out",
            out.to_str().unwrap(),
            "--eval",
            "document.getElementById('answer').dataset.said ?? null",
        ],
    )
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
    let ended = |running: std::process::Child, when: &str| -> ! {
        let output = running.wait_with_output().unwrap();
        panic!("the shot ended {when}: {}", said(&output));
    };
    // Asked for, or the shot ended first.
    let asked = test_time::until(
        "the shot asks for its worker's script",
        test_time::FLOOR,
        || {
            if site.requests().iter().any(|path| path == "/late-worker.js") {
                return Some(true);
            }
            running.try_wait().unwrap().is_some().then_some(false)
        },
    )
    .unwrap_or_else(|stall| panic!("{stall}"));
    if !asked {
        ended(running, "before its worker's script was asked for");
    }
    // A second on, the shot still waits: a loaded machine only slows it, and it ends only once the
    // script is answered.
    #[expect(
        clippy::disallowed_methods,
        reason = "no-timed-waits baseline: a test holds for a length of time to show the shot still waits"
    )]
    std::thread::sleep(Duration::from_secs(1));
    if running.try_wait().unwrap().is_some() {
        ended(running, "with its worker's script still unanswered");
    }
    site.hold_route("/late-worker.js", false);
    let output = running.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", said(&output));
    assert_eq!(printed(&output), vec![json!("from the late worker")]);
    // Painted the green a framed document paints itself.
    assert_eq!(
        middle_of(&Picture::read(&out), [0.0, 0.0, 200.0, 200.0]),
        FRAMED_GREEN,
        "the worker's answer is not drawn"
    );
}

#[test]
fn a_click_that_opens_a_page_with_its_opener_returns_and_the_page_is_shot() {
    // A button that opens a page of the page's own site with `window.open`, keeping its opener; one
    // that opens a page of another site (as `localhost`); two that open a page of the page's own
    // site, which Chrome runs in the opener's process, that alerts as it loads or once it has
    // loaded, then marks the opener (`#after`); and one that opens a blank page, which takes the
    // page's own site and process, and writes it a script that confirms, then alerts, then marks the
    // opener, in its first turn after the click, which comes before the shot can ask the page
    // anything: the opened page shows its dialog while it is in front. A dialog shown in front holds
    // the process the two pages share, and every answer of the page's, until another tab is in
    // front: the alerting page's own load left that order to a loaded machine's chance, and the
    // page never answered. One more opens a blank page and writes it a script that alerts at once,
    // then marks the opener, within the click, so the click itself waits on the process the alert
    // holds. Each opens a tab. The shot leaves a page it did not open as Chrome leaves it: let
    // start, and behind the shot's page, brought back in front as soon as Chrome tells of the new
    // one, where Chrome lets its dialogs go (an alert as by its OK, a confirm as by its Cancel)
    // and its script goes on. Each click returns, the page goes on with the window it opened and
    // is shot, and the report has no dialog of the opened page's. (A page opened later, from a
    // timer, needs the click's user activation, which lapses after five seconds, so no form of it
    // here is free of the machine's load: the driver's own tests hold that case.)
    let other = site();
    let site = site();
    let away = format!("http://localhost:{}", other.addr.port());
    for on in [&site, &other] {
        on.set_file(
            "/opened.html",
            b"<!doctype html><p>opened</p>".to_vec(),
            "text/html",
        );
    }
    let marking = "opener.document.body.append(Object.assign(opener.document.createElement('p'), \
                   {id: 'after'}))";
    site.set_file(
        "/alerting.html",
        format!("<!doctype html><script>alert('as it loads'); {marking};</script>").into_bytes(),
        "text/html",
    );
    site.set_file(
        "/later.html",
        format!(
            "<!doctype html><script>onload = () => setTimeout(() => {{ alert('once loaded'); \
             {marking}; }}, 100);</script>"
        )
        .into_bytes(),
        "text/html",
    );
    site.set_file(
        "/opening.html",
        format!(
            r#"<!doctype html>
<meta charset="utf-8">
{NO_ICON}
<script>
  const openPage = address => {{ window.opened = window.open(address); }};
  const openAsking = () => {{
    openPage('');
    window.opened.document.write("<script>setTimeout(() => {{ confirm('as it opens'); alert('as it opens'); {marking}; }});<\/script>");
    window.opened.document.close();
  }};
  const openAlerting = () => {{
    openPage('');
    window.opened.document.write("<script>alert('within the click'); {marking};<\/script>");
    window.opened.document.close();
  }};
</script>
<button id="same" type="button" onclick="openPage('/opened.html')">same site</button>
<button id="cross" type="button" onclick="openPage('{away}/opened.html')">another site</button>
<button id="alerting" type="button" onclick="openPage('/alerting.html')">alerting</button>
<button id="later" type="button" onclick="openPage('/later.html')">alerting later</button>
<button id="asking" type="button" onclick="openAsking()">asking as it opens</button>
<button id="written" type="button" onclick="openAlerting()">alerting within the click</button>"#
        )
        .into_bytes(),
        "text/html",
    );
    let marked = ["--wait-for", "#after"];
    for (button, waits) in [
        ("#same", &[][..]),
        ("#cross", &[][..]),
        ("#alerting", &marked[..]),
        ("#later", &marked[..]),
        ("#asking", &marked[..]),
        ("#written", &marked[..]),
    ] {
        let Some(chrome) = chrome::find() else {
            eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
            return;
        };
        let scratch = Scratch::new("opening");
        let out = scratch.join("opening.png");
        let url = site.url("/opening.html");
        let output = shot(
            &chrome,
            &[
                &[
                    url.as_str(),
                    "--out",
                    out.to_str().unwrap(),
                    "--click",
                    button,
                ][..],
                waits,
                &[
                    "--eval",
                    "[!!window.opened, window.opened.closed]",
                    "--report",
                ][..],
            ]
            .concat(),
        )
        .output()
        .unwrap();
        assert!(output.status.success(), "{button}: {}", said(&output));
        let [opened, report] = printed(&output).try_into().unwrap();
        assert_eq!(opened, json!([true, false]), "{button}");
        assert_eq!(report["dialogs"], json!([]), "{button}: {report}");
        assert!(Picture::read(&out).width > 0);
    }
}

#[test]
fn the_report_names_the_frame_each_console_error_and_failed_request_came_from() {
    // A frame of another site, which Chrome runs in a process of its own, and a frame of the page's
    // own site, each missing a picture and logging an error, on a page that logs one too: the
    // report has each frame's missing picture, its console error and the network's error for the
    // picture, the last two naming the frame by its address, and the page's error naming none.
    let Some(chrome) = chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): shot not run");
        return;
    };
    let other = site();
    let site = site();
    let erring = |said: &str| {
        format!(
            r#"<!doctype html><img src="/missing.png"><script>console.error('{said}')</script>"#
        )
        .into_bytes()
    };
    other.set_file("/erring.html", erring("in the frame"), "text/html");
    site.set_file(
        "/erring.html",
        erring("in a frame of its own site"),
        "text/html",
    );
    let away = format!("http://localhost:{}", other.addr.port());
    site.set_file(
        "/reporting.html",
        format!(
            r#"<!doctype html>{NO_ICON}<iframe src="{away}/erring.html"></iframe><iframe src="/erring.html"></iframe><script>console.error('on the page')</script>"#
        )
        .into_bytes(),
        "text/html",
    );
    let scratch = Scratch::new("reporting-frame");
    let out = scratch.join("reporting.png");
    let output = shot(
        &chrome,
        &[
            &site.url("/reporting.html"),
            "--out",
            out.to_str().unwrap(),
            "--report",
        ],
    )
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", said(&output));
    let [report] = printed(&output).try_into().unwrap();
    let as_set = |list: &Value| -> BTreeSet<String> {
        list.as_array()
            .unwrap_or_else(|| panic!("no list: {report}"))
            .iter()
            .map(Value::to_string)
            .collect()
    };
    // The network's own words for a missing picture are Chrome's: the report is held to its 404.
    let errors: Vec<Value> = report["console_errors"]
        .as_array()
        .unwrap_or_else(|| panic!("no console errors: {report}"))
        .iter()
        .map(|error| {
            let mut error = error.clone();
            if error["source"] == "network" {
                let text = error["text"].as_str().unwrap_or("");
                assert!(text.contains("404"), "{error}");
                error["text"] = json!("404");
            }
            error
        })
        .collect();
    let (here, there) = (site.url("/erring.html"), format!("{away}/erring.html"));
    assert_eq!(
        as_set(&Value::Array(errors)),
        as_set(&json!([
            {"source": "console", "text": "on the page"},
            {"source": "console", "text": "in the frame", "frame": there},
            {"source": "network", "text": "404", "url": format!("{away}/missing.png"), "frame": there},
            {"source": "console", "text": "in a frame of its own site", "frame": here},
            {"source": "network", "text": "404", "url": site.url("/missing.png"), "frame": here},
        ])),
        "{report}"
    );
    assert_eq!(
        as_set(&report["failed_requests"]),
        as_set(&json!([
            {"url": format!("{away}/missing.png"), "status": 404},
            {"url": site.url("/missing.png"), "status": 404},
        ])),
        "{report}"
    );
}

/// A page that replaces the window's `addEventListener` with one that throws, after its own
/// listeners are set, and, with `prototype`, the one every element and the window inherit too.
fn refusing(prototype: bool) -> Vec<u8> {
    let target = if prototype {
        "window.addEventListener = EventTarget.prototype.addEventListener = refuse;"
    } else {
        "window.addEventListener = refuse;"
    };
    format!(
        r#"<!doctype html>
<meta charset="utf-8">
<button id="plain" type="button">plain</button>
{POINTED}
<script>
  const refuse = function () {{ throw new Error('no listeners here'); }};
  {target}
</script>"#
    )
    .into_bytes()
}

#[test]
fn a_page_whose_listeners_throw_is_still_clicked_and_keeps_nothing_of_the_watch() {
    // The watch listens through EventTarget's own methods, so a window whose own throws still
    // reports the move; one whose inherited methods throw gives no report, and the button found
    // there, hovered, takes the click. Either way the page keeps no trace of the watch.
    for (name, prototype) in [("refusing-window", false), ("refusing-prototype", true)] {
        let Some((output, _out, _requests, _scratch)) = on_page(
            name,
            &refusing(prototype),
            &[
                "--hover",
                "#plain",
                "--click",
                "#plain",
                "--eval",
                "[landed, 'symphony-shot-pointer' in window, Object.keys(window).filter(key => key.startsWith('symphony'))]",
            ],
        ) else {
            return;
        };
        assert!(output.status.success(), "{name}: {}", said(&output));
        assert_eq!(
            printed(&output),
            [json!([
                [["pointerdown", "plain"], ["click", "plain"]],
                false,
                []
            ])],
            "{name}: {}",
            said(&output)
        );
    }
}

/// A button whose hover opens a menu over the whole page, a record of the pointer moving (`/moved`,
/// which a settle waits on) and pressing (`/pressed`), and each press's target (`pressed`).
const OPENS_ON_HOVER: &[u8] = br#"<!doctype html>
<meta charset="utf-8">
<style>
  body { margin: 0; font: 16px/1.4 sans-serif; }
  #menu { display: none; position: fixed; inset: 0; background: rgba(0, 0, 0, 0.3); }
  #menu.open { display: block; }
</style>
<button id="opener" type="button">opens on hover</button>
<div id="menu"></div>
<script>
  window.pressed = [];
  document.getElementById('opener').addEventListener('pointerenter', () => document.getElementById('menu').classList.add('open'));
  addEventListener('pointermove', () => fetch('/moved'), true);
  addEventListener('pointerdown', event => { pressed.push(event.target.id); fetch('/pressed'); }, true);
</script>"#;

#[test]
fn a_click_whose_hover_opens_a_menu_over_its_target_fails_naming_it_and_presses_nothing() {
    // The aim finds the button on top; the pointer reaching it opens the menu over it, so a press
    // there would reach the menu.
    let Some((output, out, requests, _scratch)) =
        on_page("opens-on-hover", OPENS_ON_HOVER, &["--click", "#opener"])
    else {
        return;
    };
    assert_eq!(output.status.code(), Some(1), "{}", said(&output));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim_end(),
        "symphony shot: --click #opener: the press would have landed on div#menu.open, on top at \
         its middle once the pointer reached it; nothing was pressed (for a menu its hover opens, \
         --hover it, then --click the item meant)",
        "{}",
        said(&output)
    );
    assert!(
        !requests.iter().any(|path| path == "/pressed"),
        "pressed: {requests:?}"
    );
    assert!(!out.exists(), "a failed shot left a picture");
    // A hover there has done what it does: the pointer reached the button and opened the menu,
    // whose item then takes the click, as the failure says.
    let Some((output, out, _requests, _scratch)) = on_page(
        "opens-on-hover-hovered",
        OPENS_ON_HOVER,
        &[
            "--hover",
            "#opener",
            "--eval",
            "document.getElementById('menu').className",
            "--click",
            "#menu",
            "--eval",
            "pressed",
        ],
    ) else {
        return;
    };
    assert!(output.status.success(), "{}", said(&output));
    assert_eq!(printed(&output), [json!("open"), json!(["menu"])]);
    assert_eq!(Picture::read(&out).width, 1440);
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
    // When the site read the page's request, before it answered and so before the shot's waits
    // for the page and then the selector began; or none, as the shot ended first.
    let asked = test_time::until("the shot asks for its page", test_time::FLOOR, || {
        if let Some(asked) = site.first_asked("/still.html") {
            return Some(Some(asked));
        }
        running.try_wait().unwrap().is_some().then_some(None)
    })
    .unwrap_or_else(|stall| panic!("{stall}"));
    let Some(asked) = asked else {
        let output = running.wait_with_output().unwrap();
        panic!(
            "the shot ended before it asked for its page: {}",
            said(&output)
        );
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
    // Each waits on its page's read of the board's columns, held until both browsers are seen:
    // a shot waits for every request of its page but the feed's, so neither ends before then.
    site.hold_route("/api/task-board", true);
    let mut runs: Vec<Running> = outs
        .iter()
        .map(|out| {
            Running(
                shot_with(
                    &wrapper,
                    &records,
                    &chrome,
                    &[&url, "--out", out.to_str().unwrap()],
                )
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
            )
        })
        .collect();
    let ports = test_time::until(
        "two browsers started, each with its DevTools port",
        test_time::backstop(Duration::from_secs(60)),
        || {
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
            if launched.len() != 2 || ports.iter().any(Option::is_none) {
                return None;
            }
            // SAFETY: signal zero only asks whether each recorded browser is still running.
            let both = launched.iter().all(
                |(pid, _)| unsafe { libc::kill(libc::pid_t::try_from(*pid).unwrap(), 0) } == 0,
            );
            assert!(
                both,
                "the two browsers never ran at the same time: {launched:?}"
            );
            Some(ports.into_iter().map(Option::unwrap).collect::<Vec<_>>())
        },
    )
    .unwrap_or_else(|stall| panic!("{stall}: {:?}", launches(&records)));
    let launched = launches(&records);
    assert_ne!(
        launched[0].1, launched[1].1,
        "the two shots shared a profile"
    );
    assert_ne!(ports[0], ports[1], "the two shots shared a DevTools port");
    site.hold_route("/api/task-board", false);
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
