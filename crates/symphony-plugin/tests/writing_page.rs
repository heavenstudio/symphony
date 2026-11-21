//! The page that walks through a first plugin (docs/PLUGIN_WRITING.md) shows the kit's example
//! (`examples/hello.rs`) whole, so the code a reader copies is code this workspace compiled and
//! tested.

use std::path::Path;

#[test]
fn the_writing_page_shows_the_example_as_it_is_built() {
    let kit = Path::new(env!("CARGO_MANIFEST_DIR"));
    let example = std::fs::read_to_string(kit.join("examples/hello.rs")).unwrap();
    let page = std::fs::read_to_string(kit.join("../../docs/PLUGIN_WRITING.md")).unwrap();
    assert!(
        page.contains(&format!("```rust\n{example}```\n")),
        "docs/PLUGIN_WRITING.md's code is not examples/hello.rs: copy the file into its rust block"
    );
}
