//! The header: one name for each of the Symphony's pages, the shown page lit and never named again
//! by a crumb, Appearance opened from it rather than from the chat sheet, and on a phone two rows
//! that keep the Symphony's seven pages in view at 390 wide and scroll a project's more across.
use super::{SHOWN, chrome, chrome::Chrome, look_parity, site, site::Site, texts};
use serde_json::json;

/// The Symphony's seven pages, then the stand-in project's own page under its plugin's label:
/// eight, one more than a phone 390 wide holds in view.
const PAGES: [&str; 8] = [
    "Home",
    "Queue",
    "Docs",
    "Board",
    "Reports",
    "Runs",
    "Agents",
    "⬡ lamps",
];

/// The stand-in server, answering a document, a task and their threads as well as the board.
fn content_site() -> Site {
    let site = site();
    let content = look_parity::responses();
    for path in [
        "/api/document",
        "/api/section",
        "/api/task-page",
        "/api/task-history",
        "/api/asks",
    ] {
        site.set_json(path, &content[path]);
    }
    site
}

fn open(page: &mut Chrome, site: &Site, path: &str, ready: &str) {
    page.open(&site.url(path));
    assert!(
        page.wait_for(
            &format!(
                "document.documentElement.dataset.navigation === 'ready' && document.querySelector({}) !== null && document.querySelector('#appearance') !== null",
                json!(ready)
            ),
            SHOWN
        ),
        "{path}: {}",
        page.eval("document.body ? document.body.innerText.slice(0, 1000) : '(no body)'")
    );
}

#[test]
fn every_page_lights_its_one_name_and_no_crumb_repeats_it() {
    let site = content_site();
    let Some(mut page) = chrome() else { return };
    page.viewport(1440, 900, false);
    for (path, ready, lit, crumbs) in [
        ("/", ".hero-title", "Home", &[][..]),
        ("/queue", "[data-ready=queue]", "Queue", &[]),
        ("/docs/", "[data-ready=folder]", "Docs", &[]),
        ("/tasks", ".waiting-board:not(.waiting-ghost)", "Board", &[]),
        ("/reports", "[data-ready=reports]", "Reports", &[]),
        ("/runs", "[data-ready=runs]", "Runs", &[]),
        ("/agents", ".agents", "Agents", &[]),
        (
            "/docs/look-parity.md",
            ".doc-card",
            "Docs",
            &["docs", "look-parity.md"],
        ),
    ] {
        open(&mut page, &site, path, ready);
        assert_eq!(texts(&mut page, ".app-pages a"), PAGES, "{path}");
        assert_eq!(
            texts(&mut page, ".app-pages a.active[aria-current=page]"),
            [lit],
            "{path}"
        );
        assert_eq!(
            texts(&mut page, "header .crumbs a, header .crumbs span"),
            crumbs,
            "{path}"
        );
        // A wide screen keeps the whole header on one line.
        assert_eq!(
            page.eval(
                "(() => { const mid = e => { const b = e.getBoundingClientRect(); return Math.round((b.top + b.bottom) / 2); }; return new Set([...document.querySelectorAll('.app-pages a, #appearance, #bell')].map(mid)).size; })()"
            ),
            json!(1),
            "{path}"
        );
    }
    open(&mut page, &site, "/task/visual/in-progress", "article.task");
    assert_eq!(texts(&mut page, ".app-pages a.active"), ["Board"]);
    open(
        &mut page,
        &site,
        "/tasks",
        ".waiting-board:not(.waiting-ghost)",
    );
    assert_eq!(texts(&mut page, ".page-head h1"), ["Board"]);
}

/// On a phone 390 wide the header is two rows, the buttons and then the pages on one row. The
/// Symphony's seven pages fit it; a project's own page beyond them makes the row scroll across,
/// shaded at the edge it continues past, as the owner's header-on-a-phone card says.
#[test]
fn on_a_phone_the_header_is_two_rows_and_its_pages_row_scrolls_a_project_s_more_shaded() {
    let site = content_site();
    let Some(mut page) = chrome() else { return };
    page.viewport_scaled(390, 844, 3, true);
    for (path, ready) in [
        ("/", ".hero-title"),
        ("/tasks", ".waiting-board:not(.waiting-ghost)"),
        ("/reports", "[data-ready=reports]"),
        ("/runs", "[data-ready=runs]"),
        ("/docs/look-parity.md", ".doc-card"),
    ] {
        open(&mut page, &site, path, ready);
        assert_eq!(
            page.eval(
                r#"(() => {
                    const box = e => e.getBoundingClientRect();
                    const mid = e => (box(e).top + box(e).bottom) / 2;
                    const bell = document.getElementById('bell'), look = document.getElementById('appearance');
                    const pages = document.querySelector('.app-pages');
                    const links = [...pages.querySelectorAll('a')];
                    const inView = l => box(l).width > 0 && box(l).left >= box(pages).left - 0.5 && box(l).right <= box(pages).right + 0.5;
                    const style = getComputedStyle(pages);
                    // The row fades, itself, at the one edge it continues past (phone_labels.rs).
                    const shaded = pages.dataset.overflow === 'end' && style.maskImage !== 'none' && style.backgroundImage === 'none';
                    pages.scrollLeft = 0;
                    const firstAtTheStart = inView(links[0]);
                    pages.scrollLeft = pages.scrollWidth;
                    const lastAtTheEnd = inView(links.at(-1));
                    pages.scrollLeft = 0;
                    const scrolls = style.overflowX === 'auto' && pages.scrollWidth > pages.clientWidth;
                    // The project's own page set aside: the Symphony's seven.
                    links.at(-1).style.display = 'none';
                    const symphonysFit = pages.scrollWidth <= pages.clientWidth && links.slice(0, -1).every(inView);
                    links.at(-1).style.display = '';
                    return {
                        buttonsShareARow: Math.abs(mid(bell) - mid(look)) < 2,
                        pagesBelowThem: box(pages).top >= box(bell).bottom,
                        pagesOnOneRow: new Set(links.map(l => Math.round(box(l).top))).size === 1,
                        scrolls,
                        shaded,
                        firstAtTheStart,
                        lastAtTheEnd,
                        symphonysFit,
                        crumbsShown: [...document.querySelectorAll('.app-header .crumbs')].some(c => c.getClientRects().length > 0),
                        headerUnder100: box(document.querySelector('.app-header')).height < 100,
                    };
                })()"#
            ),
            json!({"buttonsShareARow":true,"pagesBelowThem":true,"pagesOnOneRow":true,"scrolls":true,"shaded":true,"firstAtTheStart":true,"lastAtTheEnd":true,"symphonysFit":true,"crumbsShown":false,"headerUnder100":true}),
            "{path}"
        );
    }
}

/// A theme that picks the compact header keeps one row at every width: on a phone too narrow for
/// it the row scrolls across, and no page link is ever drawn over the Appearance or Activity
/// button. The server marks the page as `shell.rs` does (`data-layout-header`); the stand-in
/// server serves the plain page, so the test sets the same attribute.
#[test]
fn a_compact_header_on_a_phone_scrolls_one_row_and_never_draws_a_page_over_a_button() {
    let site = content_site();
    let Some(mut page) = chrome() else { return };
    for width in [390, 320] {
        page.viewport_scaled(width, 700, 3, true);
        for (path, ready) in [
            ("/", ".hero-title"),
            ("/tasks", ".waiting-board:not(.waiting-ghost)"),
        ] {
            open(&mut page, &site, path, ready);
            page.eval("document.documentElement.dataset.layoutHeader = 'compact'");
            let shown = page.eval(
                r#"(() => {
                    const box = e => e.getBoundingClientRect();
                    const header = document.querySelector('.app-header');
                    header.scrollLeft = 0;
                    const buttons = [...document.querySelectorAll('#appearance, #bell, .project-switch')].map(box);
                    const links = [...document.querySelectorAll('.app-pages a')].map(box);
                    const meet = (a, b) => a.left < b.right - 0.5 && b.left < a.right - 0.5 && a.top < b.bottom - 0.5 && b.top < a.bottom - 0.5;
                    const mids = [...links, ...buttons].map(b => (b.top + b.bottom) / 2);
                    const start = {
                        oneRow: Math.max(...mids) - Math.min(...mids) < 2,
                        pageOverAButton: links.some(l => buttons.some(b => meet(l, b))),
                        scrolls: getComputedStyle(header).overflowX === 'auto' && header.scrollWidth > header.clientWidth,
                    };
                    header.scrollLeft = header.scrollWidth;
                    const bell = box(document.getElementById('bell'));
                    return {...start, bellInViewAtTheEnd: bell.left >= 0 && bell.right <= innerWidth + 0.5};
                })()"#,
            );
            assert_eq!(
                shown,
                json!({"oneRow":true,"pageOverAButton":false,"scrolls":true,"bellInViewAtTheEnd":true}),
                "{width} {path}"
            );
        }
    }
}

#[test]
fn appearance_opens_from_the_header_and_the_chat_sheet_no_longer_holds_it() {
    let site = content_site();
    let Some(mut page) = chrome() else { return };
    page.viewport(1440, 900, false);
    open(
        &mut page,
        &site,
        "/tasks",
        ".waiting-board:not(.waiting-ghost)",
    );
    assert_eq!(
        page.eval(
            "({label: document.getElementById('appearance').getAttribute('aria-label'), expanded: document.getElementById('appearance').getAttribute('aria-expanded'), panel: window.reviewPanelReady === true})"
        ),
        json!({"label":"Appearance","expanded":"false","panel":false}),
        "the button is drawn with the page and loads nothing until it is used"
    );
    page.eval("document.getElementById('appearance').click()");
    assert!(page.wait_for(
        "document.querySelector('#appearance-panel[role=dialog] .theme-menu [data-theme-action=import]') !== null && document.getElementById('appearance').getAttribute('aria-expanded') === 'true'",
        SHOWN
    ));
    assert_eq!(
        texts(&mut page, "#appearance-panel [data-theme-action]"),
        ["Import shadcn theme…", "Default theme"]
    );
    // The panel hangs under its button, inside the window.
    let under_its_button = "(() => { const p = document.getElementById('appearance-panel').getBoundingClientRect(), b = document.getElementById('appearance').getBoundingClientRect(); return p.top >= b.bottom && p.top <= b.bottom + 16 && p.right <= innerWidth && p.left >= 0; })()";
    assert_eq!(page.eval(under_its_button), json!(true));
    // A notice above the header pushes the button down, and scrolling moves it: the panel follows.
    page.eval("document.getElementById('appearance').click()");
    assert!(page.wait_for(
        "document.getElementById('appearance-panel') === null",
        SHOWN
    ));
    page.eval(
        "(() => { const notice = document.createElement('div'); notice.style.height = '180px'; document.body.prepend(notice); const tail = document.createElement('div'); tail.style.height = '2000px'; document.body.append(tail); })()",
    );
    page.eval("document.getElementById('appearance').click()");
    assert!(page.wait_for(
        "document.getElementById('appearance-panel') !== null",
        SHOWN
    ));
    assert_eq!(page.eval(under_its_button), json!(true), "under a notice");
    page.eval("scrollTo(0, 100)");
    assert!(
        page.wait_for(&format!("scrollY === 100 && {under_its_button}"), SHOWN),
        "scrolled"
    );
    page.eval("scrollTo(0, 0)");
    // Activity and Appearance share the top right: opening one closes the other.
    page.eval("document.getElementById('bell').click()");
    assert!(page.wait_for(
        "document.getElementById('activity-panel') !== null && document.getElementById('appearance-panel') === null && document.getElementById('appearance').getAttribute('aria-expanded') === 'false'",
        SHOWN
    ));
    page.eval("document.getElementById('appearance').click()");
    assert!(page.wait_for(
        "document.getElementById('appearance-panel') !== null && document.getElementById('activity-panel') === null && document.getElementById('bell').getAttribute('aria-expanded') === 'false'",
        SHOWN
    ));
    page.eval(
        "document.querySelector('#appearance-panel [aria-label=\"Close appearance\"]').click()",
    );
    assert!(page.wait_for(
        "document.getElementById('appearance-panel') === null && document.activeElement === document.getElementById('appearance')",
        SHOWN
    ));
    page.eval("document.getElementById('quick-open').click()");
    assert!(page.wait_for(
        "document.querySelector('#quick-sheet:not([hidden]) [role=tab]') !== null",
        SHOWN
    ));
    assert_eq!(
        page.eval("document.querySelector('#quick-sheet .theme-menu, #quick-sheet [aria-label=Appearance]') === null"),
        json!(true),
        "the chat sheet holds the chat and the new task alone"
    );
    // The open sheet takes the window's right edge and pushes the header aside: the panel still
    // hangs under its button, beside the sheet rather than over it.
    page.eval("document.getElementById('appearance').click()");
    assert!(page.wait_for(
        "document.getElementById('appearance-panel') !== null",
        SHOWN
    ));
    assert_eq!(
        page.eval(
            "(() => { const p = document.getElementById('appearance-panel').getBoundingClientRect(), s = document.getElementById('quick-sheet').getBoundingClientRect(); return p.right <= s.left + 0.5; })()"
        ),
        json!(true),
        "beside the open sheet"
    );
    assert_eq!(
        page.eval(under_its_button),
        json!(true),
        "with the sheet open"
    );
}
