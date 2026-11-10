//! Instruction launches retain the parent feature tip at every nesting depth.

use crate::{World, git};
use std::path::Path;

#[test]
fn an_instruction_launches_on_the_parent_feature_tip_at_any_depth() {
    for slug in ["sim/jump/r2", "sim/jump/r2/deep"] {
        let w = World::new();
        const NOTE: &str = "parent feature slice";
        let root = w.launch("sim/jump").row;
        git(
            Path::new(&root.worktree),
            &["commit", "--allow-empty", "-m", NOTE],
        );
        let feature_tip = git(Path::new(&root.worktree), &["rev-parse", "HEAD"]);
        w.task("sim/jump/r2", Some("sim/jump"));
        w.task("sim/jump/r2/deep", Some("sim/jump/r2"));
        let s = w.launch(slug).row;
        assert_eq!(
            git(Path::new(&s.worktree), &["rev-parse", "HEAD"]),
            feature_tip
        );
        assert_ne!(feature_tip, git(&w.repo, &["rev-parse", "main"]));
        assert_eq!((s.slug.as_str(), s.branch.as_str()), (slug, ""));
        assert_eq!(
            Path::new(&s.worktree)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
            slug.replace('/', "-")
        );
        assert_eq!(
            git(
                Path::new(&s.worktree),
                &["rev-parse", "--abbrev-ref", "HEAD"]
            ),
            "HEAD"
        );
    }
}
