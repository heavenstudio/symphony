//! The Symphony's desktop app for the Mac, as a library: every decision the app makes, so each is
//! tested on the host and the binary (`src/bin/symphony-desktop.rs`) only draws them in windows
//! (docs/analysis/desktop-client.md).
//!
//! - [`address`]: the servers the app may follow, plain HTTP to this Mac and the tailnet alone;
//! - [`navigation`]: where a server's window may go, and what opens in the system browser;
//! - [`main_menu`]: the menu bar, which gives the pages the keyboard's edit keys and ⌘Q;
//! - [`servers`]: this Mac's server, found by itself, and the list file of the others;
//! - [`read`]: one read of a server's project list;
//! - [`follow`]: the counts across servers, what notifies and what a notice says, the menu's lines
//!   and when each server is read next;
//! - [`cli`]: the command line, which opens the app or adds a server;
//! - [`NOTIFICATION_STUB`]: the script each page runs first, so the page never notifies itself.
//!
//! The app reads no file but its own list and the machine folder's record of this Mac's server,
//! writes none but its list, runs no command and opens no local file or application, whatever a
//! server's page or list says.

#![forbid(unsafe_code)]

pub mod address;
pub mod cli;
pub mod follow;
pub mod main_menu;
pub mod navigation;
pub mod read;
pub mod servers;

pub use address::Server;

#[cfg(test)]
pub mod testing;

/// The script every page the app shows runs before its own: the Notification API, its permission
/// always denied, so a page's own notice never doubles the app's. TypeScript with no syntax of its
/// own, so the page runs it as it is written.
pub const NOTIFICATION_STUB: &str = include_str!("notification.ts");
