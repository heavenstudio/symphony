//! Size probe: `/` and `/tasks` rendered to HTML on the host from saved API replies, gzipped: what
//! a server-drawn first frame of each page would send.
//!
//!     cargo run -p symphony-ui --example ssr_size -- BOARD.json COUNTS.json COLUMN.json...
//!
//! BOARD is `/api/board`, COUNTS the board's `/api/task-board?counts=1` and each COLUMN the first
//! page of one column, as probes/live.sh saves them. It prints the raw and gzip bytes of each body,
//! and writes the HTML to stderr.
use std::io::Write;

use dioxus::prelude::*;
use flate2::{Compression, write::GzEncoder};
use symphony_api::{Board, Task, TaskListing};
use symphony_ui::{tasks::TaskBoard, view::BoardView, words::Moment};

fn gzip(bytes: &[u8]) -> usize {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap().len()
}

fn read<T: serde::de::DeserializeOwned>(path: &str) -> T {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[derive(Props, Clone, PartialEq)]
struct Probe {
    listing: TaskListing,
    tasks: Vec<Task>,
}

#[allow(non_snake_case)]
fn TasksProbe(probe: Probe) -> Element {
    let refreshed = use_signal(|| 0_u64);
    rsx! {
        TaskBoard {
            tasks: probe.tasks,
            signoff: probe.listing.signoff,
            counts: probe.listing.counts,
            statuses: probe.listing.statuses,
            refreshed,
            older: Vec::new(),
            onfilter: move |_| {},
            onload: move |_| {},
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: ssr_size BOARD.json COUNTS.json COLUMN.json...");
        std::process::exit(2);
    }
    let board: Board = read(&args[0]);
    let now = Moment::parse("2026-10-28 11:00").unwrap();
    let home = dioxus_ssr::render_element(rsx! { BoardView { board, now } });
    println!(
        "/      {:>7} raw {:>6} gzip",
        home.len(),
        gzip(home.as_bytes())
    );
    let listing: TaskListing = read(&args[1]);
    let tasks = args[2..]
        .iter()
        .flat_map(|path| read::<TaskListing>(path).tasks)
        .collect();
    let mut dom = VirtualDom::new_with_props(TasksProbe, Probe { listing, tasks });
    dom.rebuild_in_place();
    let html = dioxus_ssr::render(&dom);
    println!(
        "/tasks {:>7} raw {:>6} gzip",
        html.len(),
        gzip(html.as_bytes())
    );
    eprintln!("{home}\n\n{html}");
}
