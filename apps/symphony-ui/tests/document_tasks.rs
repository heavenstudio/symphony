use dioxus::prelude::*;
use symphony_api::DocumentTask;
use symphony_ui::doc_components::{DocumentWithTasks, Related};

fn task(slug: &str, title: &str, status: &str) -> DocumentTask {
    DocumentTask {
        slug: slug.into(),
        title: title.into(),
        status: status.into(),
    }
}

fn slugs(tasks: &[DocumentTask]) -> Vec<&str> {
    tasks.iter().map(|task| task.slug.as_str()).collect()
}

fn panel(tasks: Vec<DocumentTask>) -> String {
    dioxus_ssr::render_element(rsx! {
        DocumentWithTasks { tasks, article { "Document prose" } }
    })
}

#[test]
fn related_work_is_accessible_and_untrusted_titles_stay_text() {
    let html = panel(vec![
        task("ui/layout", "Layout <review> & spacing", "in_progress"),
        task("ui/navigation", "Navigation", "ready"),
    ]);
    assert!(html.contains("<article>Document prose</article>"), "{html}");
    assert!(html.contains("aria-label=\"Related tasks\""), "{html}");
    assert!(html.contains("href=\"/task/ui/layout\""), "{html}");
    assert!(
        html.contains("Layout &#60;review&#62; &#38; spacing"),
        "{html}"
    );
    assert!(
        html.contains("<span class=\"doc-task-status\">in progress</span>"),
        "{html}"
    );
    assert!(
        !html.contains("in_progress"),
        "a status is its column's name, never the raw value: {html}"
    );
    assert!(html.contains("href=\"/task/ui/navigation\""), "{html}");
    assert!(
        html.contains("<span class=\"doc-task-status\">ready</span>"),
        "{html}"
    );
    assert_eq!(html.matches("<aside").count(), 1, "{html}");
}

#[test]
fn a_document_without_related_work_has_no_empty_panel() {
    let html = panel(vec![]);
    assert!(html.contains("<article>Document prose</article>"), "{html}");
    assert!(!html.contains("<aside"), "{html}");
    assert!(!html.contains("Related tasks"), "{html}");
}

#[test]
fn open_work_comes_first_and_finished_work_follows_each_in_the_boards_order() {
    let related = Related::of(vec![
        task("perf/done", "Done", "done"),
        task("perf/review", "Review", "review"),
        task("perf/cancelled", "Cancelled", "cancelled"),
        task("perf/ready", "Ready", "ready"),
        task("perf/blocked", "Blocked", "blocked"),
        task("perf/later", "Later", "done"),
    ]);
    assert_eq!(
        slugs(&related.open),
        ["perf/review", "perf/ready", "perf/blocked"]
    );
    assert_eq!(
        slugs(&related.finished),
        ["perf/done", "perf/cancelled", "perf/later"],
        "done and cancelled are both finished"
    );
}

#[test]
fn the_tasks_answering_the_owners_verdicts_are_not_related_work() {
    let related = Related::of(vec![
        task(
            "verdicts/2026-11-11-1244",
            "Answer the owner on lamp-a-pic",
            "done",
        ),
        task(
            "perf/client-cache",
            "Answer the owner on lamp-a-pic",
            "ready",
        ),
        task(
            "verdicts/2026-11-11-1742",
            "Answer the owner on lamp-a-pic",
            "ready",
        ),
        task("verdicts/2026-11-11-1742-2", "The cache", "in_progress"),
    ]);
    assert_eq!(
        slugs(&related.open),
        ["perf/client-cache"],
        "the slug's area, never the title, tells a verdict task"
    );
    assert!(related.finished.is_empty());
    assert_eq!(
        Related::of(vec![
            task("verdict/x", "", "ready"),
            task("app/verdicts/x", "", "ready")
        ])
        .open
        .len(),
        2,
        "only the verdicts/ area answers verdicts"
    );
}

#[test]
fn a_document_linked_only_by_verdict_tasks_has_no_panel() {
    let html = panel(vec![
        task(
            "verdicts/2026-11-11-1244",
            "Answer the owner on lamp-a-pic",
            "done",
        ),
        task(
            "verdicts/2026-11-11-1742",
            "Answer the owner on lamp-a-pic",
            "ready",
        ),
    ]);
    assert!(!html.contains("<aside"), "{html}");
}

#[test]
fn each_row_names_its_slug_beside_its_title_then_its_status() {
    let html = panel(vec![
        task("perf/client-cache", "Measure return visits", "ready"),
        task(
            "perf/client-cache/design",
            "Measure return visits",
            "in_progress",
        ),
    ]);
    for (slug, status) in [
        ("perf/client-cache", "ready"),
        ("perf/client-cache/design", "in progress"),
    ] {
        assert!(
            html.contains(&format!(
                "<a href=\"/task/{slug}\">Measure return visits</a><span class=\"doc-task-meta\"><code class=\"doc-task-slug\">{slug}</code> · <span class=\"doc-task-status\">{status}</span></span>"
            )),
            "{slug}: {html}"
        );
    }
}

#[test]
fn finished_work_folds_under_its_count_below_the_open_work() {
    let html = panel(vec![
        task("perf/one", "One", "done"),
        task("perf/open", "Open", "ready"),
        task("perf/two", "Two", "cancelled"),
        task(
            "verdicts/2026-11-11-1244",
            "Answer the owner on lamp-a-pic",
            "done",
        ),
    ]);
    let open = html.find("class=\"doc-tasks-open\"").expect(&html);
    let folded = html
        .find("<details class=\"doc-tasks-finished\"><summary>2 finished</summary>")
        .expect(&html);
    assert!(open < folded, "{html}");
    assert!(
        !html.contains("<details class=\"doc-tasks-finished\" open"),
        "{html}"
    );
    for slug in ["perf/one", "perf/two"] {
        assert!(
            html[folded..].contains(&format!("/task/{slug}")),
            "{slug}: {html}"
        );
    }
    assert!(!html[folded..].contains("/task/perf/open"), "{html}");
    assert!(!html.contains("verdicts/"), "{html}");
}

#[test]
fn a_panel_of_finished_work_alone_is_only_its_count() {
    let html = panel(vec![task("perf/one", "One", "done")]);
    assert!(!html.contains("doc-tasks-open"), "{html}");
    assert!(html.contains("<summary>1 finished</summary>"), "{html}");
}

#[test]
fn comparisons_label_each_kind_and_link_the_task() {
    use symphony_api::{Difference, DocumentComparison, SectionComparison};
    use symphony_ui::doc_components::{ComparisonNote, SectionDifference};
    let comparison = DocumentComparison {
        task: "design/policy".into(),
    };
    let html = dioxus_ssr::render_element(rsx! { ComparisonNote { comparison } });
    assert!(html.contains("href=\"/task/design/policy\""));
    for (kind, label) in [
        (Difference::Added, "Added"),
        (Difference::Changed, "Changed"),
        (Difference::Removed, "Removed"),
    ] {
        let comparison = Some(SectionComparison {
            kind,
            before: Vec::new(),
        });
        let html = dioxus_ssr::render_element(
            rsx! { SectionDifference { comparison, p { "Current prose" } } },
        );
        assert!(html.contains(&format!(">{label}</p>")));
        assert!(html.contains("Current prose"));
    }
    let html = dioxus_ssr::render_element(
        rsx! { SectionDifference { comparison: None, p { "Unchanged prose" } } },
    );
    assert!(html.contains("Unchanged prose"));
    assert!(!html.contains("data-difference"));
}
