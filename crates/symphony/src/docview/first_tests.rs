//! What a page reads ahead of a document (`/api/document?view=first`, [`first`]): its leading
//! sections whole within the cap, a small document all of it.

use super::{document, first, outline};
use crate::render::Render;
use crate::test_tree::Tree;
use symphony_api::AHEAD_BYTES;

fn keys(sections: &[symphony_api::Section]) -> Vec<&str> {
    sections
        .iter()
        .map(|section| section.key.as_str())
        .collect()
}

#[test]
fn a_large_documents_first_part_is_its_leading_sections_within_the_cap_and_a_small_one_is_whole() {
    let tree = Tree::new();
    tree.write(
        "docs/SMALL.md",
        "# Small\n\nA topic.\n\n## More\n\nAnd more.\n",
    );
    let paragraph = "A long paragraph of the register, as an agent writes one. ".repeat(12);
    let large: String = (1..=60)
        .map(|n| format!("## Part {n}\n\n{paragraph}\n\n"))
        .collect();
    tree.write("docs/LARGE.md", &format!("# Large\n\n{large}"));
    let render = Render::new(&tree.root);

    let small = document(&render, "docs/SMALL.md", "", &[]).unwrap();
    let whole = first(small.clone()).unwrap();
    assert_eq!(keys(&whole.document.sections), keys(&small.sections));
    assert_eq!(whole.document.sections, small.sections, "each body with it");
    assert_eq!(whole.anchors, outline(small).anchors);

    let large = document(&render, "docs/LARGE.md", "", &[]).unwrap();
    assert!(
        serde_json::to_vec(&large).unwrap().len() > 2 * AHEAD_BYTES,
        "the fixture is large"
    );
    let part = first(large.clone()).unwrap();
    let bytes = serde_json::to_vec(&part).unwrap().len();
    assert!(bytes <= AHEAD_BYTES, "{bytes} bytes over the cap");
    let taken = part.document.sections.len();
    assert!(
        taken > 1 && taken < large.sections.len(),
        "{taken} of {}",
        large.sections.len()
    );
    assert_eq!(
        part.document.sections,
        large.sections[..taken],
        "the leading sections, whole"
    );
    let mut more = part.clone();
    more.document.sections.push(large.sections[taken].clone());
    assert!(
        serde_json::to_vec(&more).unwrap().len() > AHEAD_BYTES,
        "the next section would not fit"
    );
}
