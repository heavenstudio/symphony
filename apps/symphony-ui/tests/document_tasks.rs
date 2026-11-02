use dioxus::prelude::*;
use symphony_api::DocumentTask;
use symphony_ui::doc_components::DocumentWithTasks;

#[test]
fn related_work_is_accessible_and_untrusted_titles_stay_text() {
    let tasks = vec![
        DocumentTask {
            slug: "ui/layout".into(),
            title: "Layout <review> & spacing".into(),
            status: "in_progress".into(),
        },
        DocumentTask {
            slug: "ui/navigation".into(),
            title: "Navigation".into(),
            status: "ready".into(),
        },
    ];
    let html = dioxus_ssr::render_element(rsx! {
        DocumentWithTasks { tasks, article { "Document prose" } }
    });
    assert!(html.contains("<article>Document prose</article>"), "{html}");
    assert!(html.contains("aria-label=\"Related tasks\""), "{html}");
    assert!(html.contains("href=\"/task/ui/layout\""), "{html}");
    assert!(
        html.contains("Layout &#60;review&#62; &#38; spacing"),
        "{html}"
    );
    assert!(html.contains("in_progress</span>"), "{html}");
    assert!(html.contains("href=\"/task/ui/navigation\""), "{html}");
    assert!(html.contains("ready</span>"), "{html}");
    assert_eq!(html.matches("<aside").count(), 1, "{html}");
}

#[test]
fn a_document_without_related_work_has_no_empty_panel() {
    let html = dioxus_ssr::render_element(rsx! {
        DocumentWithTasks { tasks: vec![], article { "Document prose" } }
    });
    assert!(html.contains("<article>Document prose</article>"), "{html}");
    assert!(!html.contains("<aside"), "{html}");
    assert!(!html.contains("Related tasks"), "{html}");
}
