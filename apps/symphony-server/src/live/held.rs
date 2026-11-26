//! What the hub tells its pages of the comments held on documents, so a document's page reads
//! them again only when one was held or passed on, never on a timer: the `held` feed region, with
//! no data. A hold writes no file for the watcher to tell (`symphony_board::held::OnDocument`).

use super::Hub;
use serde_json::json;
use symphony_board::lock::Recovered as _;

impl Hub {
    /// Every page hears that a comment held on a document moved: held, or passed on.
    pub(crate) fn held_moved(&self) {
        let mut pages = self.pages.write().recover();
        for page in pages.values_mut() {
            self.publish(page, vec![json!({"region": "held", "data": {}})]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Hub;
    use crate::testing::{Tree, repository};
    use serde_json::{Value, json};

    #[test]
    fn a_held_comment_moved_is_one_event_on_every_page() {
        let tree = Tree::new();
        let root = repository(&tree);
        let hub = Hub::new(&root, &root.join("build/client"));
        hub.refresh_page("", &root).unwrap();
        let before = hub.cursor("").unwrap();
        let quiet = hub.pages.read().unwrap()[""].history.len();
        hub.held_moved();
        let pages = hub.pages.read().unwrap();
        let history = &pages[""].history;
        assert_eq!(history.len(), quiet + 1, "one event");
        let event: Value = serde_json::from_str(&history.back().unwrap().json).unwrap();
        assert_eq!(event["version"], before + 1);
        assert_eq!(event["changes"], json!([{"region": "held", "data": {}}]));
    }
}
