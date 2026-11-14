//! Size probe: `/tasks` rendered to HTML on the host from saved API replies, gzipped: what a
//! server-drawn first frame of the page would send. (`/` is the web client's, apps/symphony-web.)
//!
//!     cargo run -p symphony-ui --example ssr_size -- COUNTS.json COLUMN.json...
//!
//! COUNTS is the board's `/api/task-board?counts=1` and each COLUMN the first page of one column,
//! as probes/live.sh saves them. It prints the raw and gzip bytes of the body, and writes the HTML
//! to stderr.
use std::io::Write;

use dioxus::prelude::*;
use flate2::{Compression, write::GzEncoder};
use symphony_api::{Task, TaskListing};
use symphony_ui::tasks::TaskBoard;

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
            onfilter: move |_| {},
            onload: move |_| {},
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: ssr_size COUNTS.json COLUMN.json...");
        std::process::exit(2);
    }
    let listing: TaskListing = read(&args[0]);
    let tasks = args[1..]
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
    eprintln!("{html}");
}
