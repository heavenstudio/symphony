//! The app on macOS: one window per server, its web view on that server's own pages, kept alive
//! while hidden; a reader per server, which keeps following with every window closed; the dock
//! badge and the menu-bar item with what waits on the owner across every server, the item's menu
//! listing each server and project. Every decision is the library's: this only draws them.

use std::io::{self, Write};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSString, NSURL};
use symphony_desktop::address::Server;
use symphony_desktop::follow::{Follow, Line, Notice};
use symphony_desktop::navigation::{self, Go};
use symphony_desktop::servers::{self, List, Places};
use symphony_desktop::{NOTIFICATION_STUB, read};
use tao::dpi::LogicalSize;
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy};
use tao::platform::macos::EventLoopWindowTargetExtMacOS;
use tao::window::{Window, WindowBuilder, WindowId};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};
use wry::{NewWindowResponse, WebView, WebViewBuilder};

/// How long a read of a server's list waits at each step.
const READ_WAIT: Duration = Duration::from_secs(5);
/// The menu item that quits the app.
const QUIT: &str = "quit";

/// What reaches the event loop from elsewhere.
enum Message {
    /// A read changed what the follower holds, with the notices it gave.
    Heard(Vec<Notice>),
    /// A menu item was chosen.
    Chosen(MenuId),
}

/// A server's window and the web view in it.
struct Shown {
    window: Window,
    view: WebView,
}

/// The app, on the servers `places` holds, until it quits.
pub(crate) fn run(places: &Places) -> ! {
    let servers = followed(places);
    let event_loop = EventLoopBuilder::<Message>::with_user_event().build();
    // By the server's place in the list, as the follower and the menu name them.
    let shown: Vec<Option<Shown>> = servers
        .iter()
        .map(|(server, name)| {
            show(&event_loop, server, name)
                .map_err(|why| say(&format!("{name} ({server}) cannot be shown: {why}")))
                .ok()
        })
        .collect();
    let follow = Arc::new(Mutex::new(Follow::new(servers.clone())));
    for (at, (server, _)) in servers.iter().enumerate() {
        follow_server(at, server.clone(), &follow, event_loop.create_proxy());
    }
    let chosen = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = chosen.send_event(Message::Chosen(event.id));
    }));
    let mut tray: Option<TrayIcon> = None;
    let mut lines: Vec<Line> = Vec::new();
    event_loop.run(move |event, target, flow| {
        *flow = ControlFlow::Wait;
        match event {
            // The menu-bar item is made once the application runs, as AppKit asks.
            Event::NewEvents(StartCause::Init) => {
                tray = TrayIconBuilder::new()
                    .with_title("0")
                    .with_tooltip("Symphony: what waits on you")
                    .with_menu(Box::new(menu(&[])))
                    .with_menu_on_left_click(true)
                    .build()
                    .map_err(|why| say(&format!("no menu-bar item: {why}")))
                    .ok();
            }
            Event::UserEvent(Message::Heard(notices)) => {
                let held = follow.lock().unwrap_or_else(PoisonError::into_inner);
                let total = held.total();
                lines = held.lines(now());
                drop(held);
                target.set_badge_label((total > 0).then(|| total.to_string()));
                if let Some(tray) = &tray {
                    tray.set_title(Some(total.to_string()));
                    tray.set_menu(Some(Box::new(menu(&lines))));
                }
                for notice in &notices {
                    told(notice);
                }
            }
            Event::UserEvent(Message::Chosen(id)) if id.as_ref() == QUIT => {
                *flow = ControlFlow::Exit;
            }
            Event::UserEvent(Message::Chosen(id)) => {
                let line = id
                    .as_ref()
                    .parse::<usize>()
                    .ok()
                    .and_then(|at| lines.get(at));
                if let Some(line) = line
                    && let Some(Some(shown)) = shown.get(line.server)
                {
                    bring(shown, line.page.as_deref());
                }
            }
            Event::WindowEvent {
                window_id,
                event: WindowEvent::CloseRequested,
                ..
            } => {
                // Hidden, not closed: its page, drafts and scroll are there when it comes back.
                if let Some(shown) = find(&shown, window_id) {
                    shown.window.set_visible(false);
                }
            }
            Event::Reopen {
                has_visible_windows: false,
                ..
            } => {
                if let Some(first) = shown.iter().flatten().next() {
                    bring(first, None);
                }
            }
            _ => {}
        }
    })
}

/// The servers to follow, by address and name: this Mac's first, when the machine folder names a
/// running one, then each added one, in the list's order. A list that does not read is said, and
/// this Mac alone is followed until it reads.
fn followed(places: &Places) -> Vec<(Server, String)> {
    let mut servers = Vec::new();
    if let Some(here) = places.machine.as_deref().and_then(servers::this_mac) {
        let name = here.default_name();
        servers.push((here, name));
    }
    match List::read(&places.settings) {
        Ok(list) => servers.extend(
            list.servers
                .into_iter()
                .map(|saved| (saved.server, saved.name)),
        ),
        Err(why) => say(&why),
    }
    if servers.is_empty() {
        say(
            "no server to follow: this Mac runs none, and none was added (symphony-desktop add URL)",
        );
    }
    servers
}

/// `server`'s window, named `name`, on its project list.
fn show(event_loop: &EventLoop<Message>, server: &Server, name: &str) -> Result<Shown, String> {
    let window = WindowBuilder::new()
        .with_title(format!("Symphony · {name}"))
        .with_inner_size(LogicalSize::new(1280.0, 860.0))
        .build(event_loop)
        .map_err(|why| why.to_string())?;
    let own = server.clone();
    let view = WebViewBuilder::new()
        .with_url(server.page("/"))
        .with_initialization_script(NOTIFICATION_STUB)
        .with_navigation_handler(move |to| go(&navigation::navigation(&own, &to)))
        .with_new_window_req_handler(|to, _| {
            go(&navigation::new_window(&to));
            NewWindowResponse::Deny
        })
        .with_download_started_handler(|from, _| {
            go(&navigation::new_window(&from));
            false
        })
        .build(&window)
        .map_err(|why| why.to_string())?;
    Ok(Shown { window, view })
}

/// A reader of server `at` for as long as the app runs: each answer told to `follow`, which says
/// when to read again, and the event loop told through `proxy`.
fn follow_server(
    at: usize,
    server: Server,
    follow: &Arc<Mutex<Follow>>,
    proxy: EventLoopProxy<Message>,
) {
    let follow = Arc::clone(follow);
    std::thread::spawn(move || {
        loop {
            let answer = read::projects(&server, READ_WAIT);
            let (notices, next) =
                follow
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .heard(at, answer, now());
            if proxy.send_event(Message::Heard(notices)).is_err() {
                return;
            }
            std::thread::sleep(next);
        }
    });
}

/// The menu-bar item's menu: each line, then Quit.
fn menu(lines: &[Line]) -> Menu {
    let menu = Menu::new();
    for (at, line) in lines.iter().enumerate() {
        let _ = menu.append(&MenuItem::with_id(
            at.to_string(),
            &line.label,
            !line.greyed,
            None,
        ));
    }
    if !lines.is_empty() {
        let _ = menu.append(&PredefinedMenuItem::separator());
    }
    let _ = menu.append(&MenuItem::with_id(QUIT, "Quit Symphony", true, None));
    menu
}

/// What the web view does with a navigation the rule answered `go`: whether it loads it.
fn go(go: &Go) -> bool {
    match go {
        Go::Stay => true,
        Go::Browser(url) => {
            if let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) {
                NSWorkspace::sharedWorkspace().openURL(&url);
            }
            false
        }
        Go::Refuse => false,
    }
}

/// `shown`'s window in front, on `page` when one is named.
fn bring(shown: &Shown, page: Option<&str>) {
    if let Some(page) = page {
        let _ = shown.view.load_url(page);
    }
    shown.window.set_visible(true);
    shown.window.set_focus();
}

fn find(shown: &[Option<Shown>], id: WindowId) -> Option<&Shown> {
    shown.iter().flatten().find(|shown| shown.window.id() == id)
}

/// A notice, until the app posts its notices through the notification centre: a line on stderr.
fn told(notice: &Notice) {
    let place = notice
        .place
        .as_deref()
        .map_or_else(String::new, |place| format!("{place}: "));
    say(&format!(
        "notice: {place}{} — {} ({})",
        notice.title, notice.body, notice.page
    ));
}

fn say(line: &str) {
    let _ = writeln!(io::stderr(), "· symphony-desktop: {line}");
}

/// Seconds since the epoch.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}
