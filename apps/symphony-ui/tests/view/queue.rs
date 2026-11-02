use dioxus::prelude::*;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use symphony_ui::{platform::Platform, queue::WaitingQueue, queue_filter};

fn cards() -> Vec<symphony_api::Card> {
    [("art", 30), ("engineering", 3)]
        .into_iter()
        .flat_map(|(folder, count)| {
            (0..count).map(move |n| {
                serde_json::from_value(serde_json::json!({
            "doc":format!("docs/{folder}/CARDS.md"), "slug":format!("{folder}-{n}"),
            "doc_title":folder, "title":format!("Decision {folder}-{n}"), "why":"design call"
        })).unwrap()
            })
        })
        .collect()
}

fn render(selected: &str) -> String {
    fn view(selected: String) -> Element {
        rsx! { WaitingQueue { items: cards(), selected, onselect: |_| {} } }
    }
    let mut dom = VirtualDom::new_with_props(view, selected.to_string());
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

#[test]
fn category_counts_sum_to_the_section_and_filtering_keeps_only_its_rows() {
    let cards = cards();
    let counts = queue_filter::counts(cards.iter().map(|c| c.doc.as_str()));
    assert_eq!(
        counts,
        BTreeMap::from([("art".into(), 30), ("engineering".into(), 3)])
    );
    assert_eq!(counts.values().sum::<usize>(), cards.len());
    let all = render("");
    assert_eq!(all.matches("data-card=").count(), 33);
    assert!(
        all.contains("All (33)") && all.contains("art (30)") && all.contains("engineering (3)")
    );
    let art = render("art");
    assert_eq!(art.matches("data-card=").count(), 30);
    assert!(!art.contains("data-card=\"docs/engineering/"));
    assert!(art.contains("Showing 30 of 33"));
    let engineering = render("engineering");
    assert_eq!(engineering.matches("data-card=").count(), 3);
    assert!(!engineering.contains("data-card=\"docs/art/"));
    assert_eq!(
        render("removed-folder"),
        all,
        "a vanished saved category falls back to All"
    );
    assert_eq!(
        symphony_api::queue_category("@proposal~topic/docs/art/CARDS.md"),
        "art"
    );
    assert_eq!(symphony_api::queue_category("docs/ROOT.md"), "docs");
    let mut empty = VirtualDom::new(
        || rsx! { WaitingQueue { items: vec![], selected: "art", onselect: |_| {} } },
    );
    empty.rebuild_in_place();
    let empty = dioxus_ssr::render(&empty);
    assert!(empty.contains("All (0)") && empty.contains("nothing to decide"));
    assert_eq!(empty.matches("data-card=").count(), 0);
}

#[derive(Clone, Default)]
struct Device(Rc<RefCell<BTreeMap<String, String>>>);
impl Platform for Device {
    fn notify(&self, _: &str, _: &str) {}
    fn open_external(&self, _: &str) {}
    fn load(&self, key: &str) -> Option<String> {
        self.0.borrow().get(key).cloned()
    }
    fn save(&self, key: &str, value: &str) {
        self.0.borrow_mut().insert(key.into(), value.into());
    }
}

fn chips(device: Device) -> Element {
    rsx! { queue_filter::CategoryChips {
        counts: BTreeMap::from([("art".into(), 30), ("engineering".into(), 3)]),
        selected: queue_filter::load(&device),
        onselect: move |category: String| queue_filter::remember(&device, &category)
    } }
}

#[test]
fn tapping_a_chip_remembers_this_device_and_all_clears_the_filter() {
    use dioxus::core::{AttributeValue, Mutation, Mutations};
    use std::any::Any;
    let device = Device::default();
    let other = Device::default();
    queue_filter::remember(&other, "engineering");
    assert_eq!(queue_filter::load(&device), "");
    let mut dom = VirtualDom::new_with_props(chips, device.clone());
    let mut mutations = Mutations::default();
    dom.rebuild(&mut mutations);
    let buttons: BTreeMap<_, _> = mutations
        .edits
        .iter()
        .filter_map(|mutation| match mutation {
            Mutation::SetAttribute {
                name: "data-category",
                value: AttributeValue::Text(category),
                id,
                ..
            } => Some((category.clone(), *id)),
            _ => None,
        })
        .collect();
    assert_eq!(buttons.len(), 3);
    let click = |id| {
        let data = Rc::new(dioxus::html::PlatformEventData::new(Box::new(()))) as Rc<dyn Any>;
        dom.runtime()
            .handle_event("click", Event::new(data, true), id);
    };
    click(buttons["art"]);
    assert_eq!(queue_filter::load(&device), "art");
    let mut other_dom = VirtualDom::new_with_props(chips, other.clone());
    other_dom.rebuild_in_place();
    let other_html = dioxus_ssr::render(&other_dom);
    let selected_chip = other_html
        .split("<button")
        .filter_map(|part| part.split_once('>').map(|(attributes, _)| attributes))
        .find(|attributes| attributes.contains("data-category=\"engineering\""))
        .expect("the other device renders its saved category");
    assert!(selected_chip.contains("aria-pressed=true"), "{other_html}");
    click(buttons[""]);
    assert_eq!(queue_filter::load(&device), "");
}
