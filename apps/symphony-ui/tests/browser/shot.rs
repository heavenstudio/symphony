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
        let path =
            std::env::temp_dir().join(format!("sf-browser-shot-{case}-{}", std::process::id()));
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
        .env("SF_CHROME", chrome);
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
        eprintln!("no Chrome on this machine (SF_CHROME names one): shot not run");
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
        eprintln!("no Chrome on this machine (SF_CHROME names one): shot not run");
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
        eprintln!("no Chrome on this machine (SF_CHROME names one): shot not run");
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
        eprintln!("no Chrome on this machine (SF_CHROME names one): shot not run");
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
        eprintln!("no Chrome on this machine (SF_CHROME names one): shot not run");
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
        eprintln!("no Chrome on this machine (SF_CHROME names one): shot not run");
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
        eprintln!("no Chrome on this machine (SF_CHROME names one): shot not run");
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
