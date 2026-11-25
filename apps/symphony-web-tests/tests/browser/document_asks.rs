//! A document's questions in the release client (apps/symphony-web/src/asks.tsx), against the
//! stand-in server, as its asks read says whether the one reading may ask (`may_ask`): one who may
//! is offered Use selected passage, a question and each thread's follow-up, under "Ask about this
//! document"; one who may not, as a person who may speak and may not direct agents, reads the
//! threads alone under "Questions about this document", with no control, and is drawn nothing,
//! not even the section's room, where there are none. On a desk and a phone, each within the
//! screen's width. The document is the card probe's.
//!
//! With `SYMPHONY_ASKS_SHOTS` naming a folder, the section is saved there at each width and in each
//! scheme, for one who may ask and one who may not.
use super::{SHOWN, chrome, chrome::Chrome, document_cards, site::Site};
use serde_json::{Value, json};

const PAGE: &str = "/docs/PROBE.md";

fn paragraph(words: &str) -> Value {
    json!({"type": "paragraph", "spans": [{"type": "text", "text": words}]})
}

/// stefano's question about the lamps, answered.
fn thread() -> Value {
    json!({"id": 7, "path": "docs/PROBE.md", "anchor": "lamps", "quote": "Keep all 141 lamps",
    "question": "Why ninety?", "asked": 1.0, "updated": 2.0, "answers": [], "status": "answered",
    "messages": [
        {"seq": 1, "kind": "question", "by": "stefano", "text": "", "at": 1.0,
         "blocks": [paragraph("Why ninety and not a hundred?")]},
        {"seq": 2, "kind": "answer", "by": "teal-lynx", "text": "", "at": 2.0,
         "blocks": [paragraph("Ninety covers every finish the catalogue still sells.")]},
    ]})
}

/// The probe's document, its asks read `asks`, the reader asking where `may_ask`.
fn served(asks: &[Value], may_ask: bool) -> Site {
    let site = document_cards::probe_site();
    site.set_json("/api/postponed", &json!([]));
    site.set_json(
        "/api/asks",
        &json!({"asks": asks, "listening": true, "recipient": "teal-lynx", "may_ask": may_ask}),
    );
    site
}

/// The questions' section as drawn: its label, its threads and the messages of each, its buttons
/// and its fields (a box to type in by its id, an attachment's by its type), whether it says where chat goes, and whether it and the page stay within the
/// screen's width; null with no section, and then the room its shell keeps, in px.
fn drawn(page: &mut Chrome) -> Value {
    page.eval(
        r#"(() => {
            const shell = document.querySelector('.asks-shell');
            const section = document.querySelector('#asks');
            if (!section) return {section: null, room: shell.getBoundingClientRect().height};
            const box = section.getBoundingClientRect();
            return {section: {
                label: section.querySelector('h2').textContent,
                threads: [...section.querySelectorAll('.ask-thread')].map(thread => ({
                    id: thread.id,
                    by: [...thread.querySelectorAll('.ask-message .by')].map(by => by.textContent),
                })),
                buttons: [...section.querySelectorAll('button')].map(b => b.textContent.trim()),
                fields: [...section.querySelectorAll('textarea, input')].map(field => field.id || field.type),
                recipient: [...section.querySelectorAll(':scope > p')].some(p => p.textContent.startsWith('Chat goes to')),
                within: box.right <= innerWidth + 0.5 && document.documentElement.scrollWidth <= innerWidth,
            }};
        })()"#,
    )
}

/// The section once its read answered and each of `threads` showed its messages.
fn read(page: &mut Chrome, threads: &[&str], may_ask: bool) -> Value {
    page.reveal(".asks-shell");
    let answered = match (threads, may_ask) {
        (_, true) => "document.querySelector('#new-ask') !== null".to_string(),
        ([], false) => "document.querySelector('.asks-shell').childElementCount === 0".to_string(),
        ([first, ..], false) => format!("document.querySelector('#{first}') !== null"),
    };
    assert!(
        page.wait_for(&answered, SHOWN),
        "the asks read never answered: {}",
        page.eval("document.querySelector('.asks-shell')?.outerHTML.slice(0, 1500)")
    );
    for id in threads {
        page.reveal(&format!("#{id}"));
        assert!(page.wait_for(
            &format!("document.querySelectorAll('#{id} .ask-message').length === 2"),
            SHOWN
        ));
    }
    page.frames_with_fonts();
    drawn(page)
}

/// The desk's and the phone's screens.
const SCREENS: [(u32, u32, bool); 2] = [(1440, 900, false), (390, 844, true)];

/// The screen with the section's label at its top, saved as `NAME-WIDTH-light.png` and
/// `NAME-WIDTH-dark.png` in the folder `SYMPHONY_ASKS_SHOTS` names; nothing without one. The
/// scheme is put back as it was.
fn shot(page: &mut Chrome, name: &str, width: u32) {
    let Some(folder) = std::env::var_os("SYMPHONY_ASKS_SHOTS") else {
        return;
    };
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    page.eval("document.querySelector('#asks').scrollIntoView({block: 'start'})");
    for scheme in ["light", "dark"] {
        page.scheme(scheme);
        page.frames_with_fonts();
        page.wait_for("document.getAnimations().length === 0", SHOWN);
        let file = folder.join(format!("{name}-{width}-{scheme}.png"));
        std::fs::write(file, page.screenshot()).unwrap();
    }
    page.call("Emulation.setEmulatedMedia", json!({"features": []}));
}

/// A section as [`drawn`] gives it, its thread `ask-7` and its two messages, with `buttons` and
/// `fields`.
fn section(label: &str, buttons: Value, fields: Value, recipient: bool) -> Value {
    json!({"section": {
        "label": label,
        "threads": [{"id": "ask-7", "by": ["stefano", "teal-lynx"]}],
        "buttons": buttons,
        "fields": fields,
        "recipient": recipient,
        "within": true,
    }})
}

#[test]
fn a_document_offers_its_questions_controls_to_one_who_may_ask_and_its_threads_alone_else() {
    for (width, height, mobile) in SCREENS {
        let Some(mut page) = chrome() else { return };
        page.viewport(width, height, mobile);

        let site = served(&[thread()], true);
        page.open(&site.url(PAGE));
        let asks = section(
            "Ask about this document",
            json!(["Use selected passage", "Ask", "Send follow-up"]),
            json!(["new-ask", "file", "reply-7", "file"]),
            true,
        );
        assert_eq!(read(&mut page, &["ask-7"], true), asks, "{width}");
        shot(&mut page, "document-asks", width);

        let site = served(&[thread()], false);
        page.open(&site.url(PAGE));
        let reads = section("Questions about this document", json!([]), json!([]), false);
        assert_eq!(read(&mut page, &["ask-7"], false), reads, "{width}");
        shot(&mut page, "document-asks-read", width);

        // No questions: nothing, and no room kept for them.
        let site = served(&[], false);
        page.open(&site.url(PAGE));
        assert_eq!(
            read(&mut page, &[], false),
            json!({"section": null, "room": 0}),
            "{width}"
        );
        assert_eq!(site.posts(), [], "{width}: nothing sent");
    }
}
