//! Size probe: `/` rendered to HTML on the host from a saved API reply, gzipped: what a
//! server-drawn first frame of the page would send. (`/tasks` is the web client's,
//! apps/symphony-web.)
//!
//!     cargo run -p symphony-ui --example ssr_size -- BOARD.json
//!
//! BOARD is `/api/board`, as probes/live.sh saves it. It prints the raw and gzip bytes of the body,
//! and writes the HTML to stderr.
use std::io::Write;

use dioxus::prelude::*;
use flate2::{Compression, write::GzEncoder};
use symphony_api::Board;
use symphony_ui::{view::BoardView, words::Moment};

fn gzip(bytes: &[u8]) -> usize {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap().len()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [path] = args.as_slice() else {
        eprintln!("usage: ssr_size BOARD.json");
        std::process::exit(2);
    };
    let board: Board = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let now = Moment::parse("2026-10-28 11:00").unwrap();
    let home = dioxus_ssr::render_element(rsx! { BoardView { board, now } });
    println!(
        "/      {:>7} raw {:>6} gzip",
        home.len(),
        gzip(home.as_bytes())
    );
    eprintln!("{home}");
}
