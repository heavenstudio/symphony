//! Size probe: the home page's body (`view::BoardView`) and its live feed drawn over web-sys with
//! no framework: the board fetched and read by serde-wasm-bindgen, each delta applied by
//! `state::apply`, the regions redrawn. Not the frame, the media dialog or navigation.
#[cfg(target_arch = "wasm32")]
mod page {
    use std::cell::RefCell;
    use std::rc::Rc;
    use symphony_api::{BOARD_PATH, Board, Card, Delta, FEED_EVENT, FEED_PATH, Picture};
    use symphony_ui::state::apply;
    use symphony_ui::words::{Moment, age, greeting, lead, plural};
    use wasm_bindgen::{JsCast, JsValue, closure::Closure};
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{Document, Element, EventSource, MessageEvent};

    struct Ui {
        document: Document,
        root: Element,
    }

    impl Ui {
        fn el(&self, parent: &Element, tag: &str, class: &str, text: &str) -> Element {
            let element = self.document.create_element(tag).expect("an element");
            if !class.is_empty() {
                element.set_class_name(class);
            }
            if !text.is_empty() {
                element.set_text_content(Some(text));
            }
            let _ = parent.append_child(&element);
            element
        }

        fn link(&self, parent: &Element, class: &str, href: &str, text: &str) -> Element {
            let link = self.el(parent, "a", class, text);
            let _ = link.set_attribute("href", href);
            link
        }

        fn picture(&self, parent: &Element, picture: &Picture) {
            let figure = self.el(
                parent,
                "figure",
                if picture.waiting { "waiting" } else { "" },
                "",
            );
            match &picture.src {
                None => {
                    let _ = figure.set_attribute("class", "none");
                    self.el(&figure, "div", "nopic", "picture coming");
                }
                Some(src) => {
                    let image = self.el(&figure, "img", "", "");
                    let _ = image.set_attribute("src", src);
                    let _ = image.set_attribute("loading", "lazy");
                    let _ = image.set_attribute("alt", &picture.caption);
                }
            }
            self.el(&figure, "figcaption", "", &picture.caption);
        }

        fn hero(&self, parent: &Element, card: Option<&Card>, waiting: u32, now: Moment) {
            let section = self.el(parent, "section", "hero card", "");
            let text = self.el(&section, "div", "hero-text", "");
            let Some(card) = card else {
                self.el(&text, "p", "eyebrow", "Next up");
                self.el(&text, "h2", "hero-title", "Nothing waits on you. ✦");
                return;
            };
            self.el(&text, "p", "eyebrow", &format!("Next up · 1 of {waiting}"));
            self.el(&text, "h2", "hero-title", &card.title);
            if !card.summary.is_empty() {
                self.el(&text, "p", "hero-summary", &card.summary);
            }
            let meta = self.el(&text, "p", "hero-meta", "");
            self.link(&meta, "docchip", &format!("/{}", card.doc), &card.doc_title);
            self.el(&meta, "span", "why", &card.why);
            let waited = age(card.posted.as_deref(), now);
            if !waited.is_empty() {
                self.el(&meta, "span", "muted", &format!("waiting {waited}"));
            }
            self.link(&text, "go", &card.href(), "Open this card →");
            if !card.pictures.is_empty() {
                let pictures = self.el(&section, "div", "pics", "");
                for picture in card.pictures.iter().take(6) {
                    self.picture(&pictures, picture);
                }
            }
        }

        fn draw(&self, board: &Board, now: Moment) {
            self.root.set_text_content(None);
            let dash = self.el(&self.root, "div", "dash", "");
            let head = self.el(&dash, "div", "dash-head page-head", "");
            let titles = self.el(&head, "div", "", "");
            self.el(&titles, "h1", "", greeting(now));
            self.el(
                &titles,
                "p",
                "lead",
                &lead(board.counts.waiting, board.counts.docs_waiting),
            );
            let links = self.el(&head, "p", "dash-links", "");
            let project = symphony_ui::project::current();
            let mut pills = vec![
                ("/queue".to_string(), "Full queue".to_string()),
                ("/docs/".into(), "All docs".into()),
            ];
            pills.extend(
                project
                    .nav
                    .iter()
                    .map(|item| (item.href.clone(), item.title.clone())),
            );
            pills.push((format!("/{}", project.register), "Decision register".into()));
            pills.push(("/docs/workflow/REVIEW.md".into(), "How review works".into()));
            for (href, text) in &pills {
                self.link(&links, "pill", href, text);
            }
            let top = self.el(&dash, "div", "dash-top", "");
            self.hero(&top, board.next.first(), board.counts.waiting, now);
            if board.next.len() > 1 {
                let next = self.el(&top, "section", "next card", "");
                self.el(&next, "h2", "eyebrow", "Then");
                let list = self.el(&next, "div", "nextlist", "");
                for card in &board.next[1..] {
                    let row = self.link(&list, "nextcard", &card.href(), "");
                    match card.pictures.iter().find_map(|p| p.src.clone()) {
                        Some(src) => {
                            let image = self.el(&row, "img", "thumb", "");
                            let _ = image.set_attribute("src", &src);
                        }
                        None => {
                            self.el(&row, "span", "thumb glyph", "◆");
                        }
                    }
                    let text = self.el(&row, "span", "nexttext", "");
                    self.el(&text, "strong", "", &card.title);
                    self.el(
                        &text,
                        "small",
                        "",
                        &format!("{} · {}", card.doc_title, card.why),
                    );
                }
                self.link(
                    &next,
                    "more",
                    "/queue",
                    &format!("show all {} →", board.counts.waiting),
                );
            }
            let counts = &board.counts;
            let tiles = self.el(&dash, "div", "tiles", "");
            let (tasks, live) = match &counts.tasks {
                Some(tasks) => (
                    format!("{} · {}", tasks.in_progress, tasks.in_review),
                    tasks.agents_live.to_string(),
                ),
                None => ("–".to_string(), "–".to_string()),
            };
            let hint = if counts.waiting > 0 {
                format!(
                    "across {} {}",
                    counts.docs_waiting,
                    plural(counts.docs_waiting, "document")
                )
            } else {
                "all clear".to_string()
            };
            for (href, value, label, tone, hint) in [
                (
                    "/queue#you",
                    counts.waiting.to_string(),
                    "waiting on you",
                    "you",
                    hint,
                ),
                (
                    "/queue#spot",
                    counts.spot_check.to_string(),
                    "auto-approved to spot-check",
                    "auto",
                    "your verdict overrides".into(),
                ),
                (
                    "#activity",
                    counts.answered_today.to_string(),
                    "answered by agents today",
                    "answered",
                    "replies to you".into(),
                ),
                (
                    "/tasks",
                    tasks,
                    "tasks in progress · in review",
                    "tasks",
                    "on the task board".into(),
                ),
                (
                    "/agents",
                    live,
                    "agents live",
                    "agents",
                    "working now".into(),
                ),
            ] {
                let tile = self.link(&tiles, &format!("tile card tone-{tone}"), href, "");
                self.el(&tile, "b", "", &value);
                self.el(&tile, "span", "", label);
                self.el(&tile, "small", "", &hint);
            }
            let progress = &board.progress;
            let panel = self.el(&dash, "section", "progress panel card", "");
            for region in &progress.regions {
                let done: u32 = region.bars.iter().map(|bar| bar.done).sum();
                let total: u32 = region.bars.iter().map(|bar| bar.total).sum();
                if total == 0 {
                    continue;
                }
                let shown = self.el(&panel, "div", "region", "");
                self.el(&shown, "h2", "", &region.title);
                let big = self.el(
                    &shown,
                    "p",
                    "big",
                    &format!("{:.0}%", 100.0 * f64::from(done) / f64::from(total)),
                );
                self.el(&big, "small", "", &region.note);
                let bars = self.el(&shown, "div", "bars", "");
                for each in &region.bars {
                    let bar = self.link(&bars, "bar", &format!("/{}", each.page), "");
                    self.el(&bar, "span", "bar-name", &each.name);
                    let track = self.el(&bar, "span", "track", "");
                    for (class, part) in [("ok", each.done), ("wait", each.waiting)] {
                        let fill = self.el(&track, "i", class, "");
                        let share = 100.0 * f64::from(part) / f64::from(each.total.max(1));
                        let _ = fill.set_attribute("style", &format!("width: {share:.1}%"));
                    }
                    self.el(
                        &bar,
                        "span",
                        "bar-num",
                        &format!("{}/{}", each.done, each.total),
                    );
                }
            }
            let docs = self.el(&panel, "div", "docsummary", "");
            self.link(
                &docs,
                "subtle",
                "/docs/",
                &format!("browse all {} →", progress.docs),
            );
            let waiting = progress.decisions_waiting;
            self.el(
                &docs,
                "p",
                "",
                &format!("{waiting} {} waiting", plural(waiting, "decision")),
            );
        }
    }

    fn now() -> Moment {
        let date = js_sys::Date::new_0();
        Moment::from_parts(
            i64::from(date.get_full_year()),
            date.get_month() + 1,
            date.get_date(),
            date.get_hours(),
            date.get_minutes(),
        )
        .expect("a moment")
    }

    async fn fetch_board(window: &web_sys::Window) -> Result<Board, JsValue> {
        let response: web_sys::Response = JsFuture::from(window.fetch_with_str(BOARD_PATH))
            .await?
            .dyn_into()?;
        let value = JsFuture::from(response.json()?).await?;
        serde_wasm_bindgen::from_value(value).map_err(JsValue::from)
    }

    pub fn start() {
        wasm_bindgen_futures::spawn_local(async {
            let window = web_sys::window().expect("a window");
            let document = window.document().expect("a document");
            let root = document.get_element_by_id("main").expect("a mount");
            let ui = Rc::new(Ui { document, root });
            let Ok(board) = fetch_board(&window).await else {
                return;
            };
            ui.draw(&board, now());
            let board = Rc::new(RefCell::new(board));
            let url = format!("{FEED_PATH}?since={}", board.borrow().version);
            let Ok(feed) = EventSource::new(&url) else {
                return;
            };
            let listener = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
                let Some(text) = event.data().as_string() else {
                    return;
                };
                let Ok(value) = js_sys::JSON::parse(&text) else {
                    return;
                };
                let Ok(delta) = serde_wasm_bindgen::from_value::<Delta>(value) else {
                    return;
                };
                let moved = apply(&mut board.borrow_mut(), &delta).moved;
                if !moved.is_empty() {
                    ui.draw(&board.borrow(), now());
                }
            });
            let _ = feed
                .add_event_listener_with_callback(FEED_EVENT, listener.as_ref().unchecked_ref());
            listener.forget();
            std::mem::forget(feed);
        });
    }
}

fn main() {
    #[cfg(target_arch = "wasm32")]
    page::start();
}
