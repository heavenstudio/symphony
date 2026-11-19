//! A head a pull request squashed into main, as the sweep proves it: the second proof of a landing,
//! beside ancestry. The host's squash leaves the task's head in no history of main
//! ([`crate::land::delivery`]), so the receipt the land kept once it proved the host's commit is
//! what says the head landed, and only while git and the kept files bear all of it out now:
//!
//! * the head still kept under [`RETAINED`]`SLUG`, so a branch deleted loses no commit;
//! * that task's [`Receipt`], kept under its own name, whose `head` is exactly the commit asked
//!   about: a branch that moved on, or an ancestor of a delivered head, is of another head;
//! * its `squash`, `parent` and `tree` each a whole object id, never a name, a short id or an
//!   option git would read as one;
//! * its `squash` a commit of that one parent and that tree, the tree git makes of merging the head
//!   into that parent, as the land proved it;
//! * that squash on the remote default branch as last fetched (`refs/remotes/`[`REMOTE`]`/main`)
//!   and in main here: a delivery the main checkout has not followed yet is not landed here.
//!
//! Anything else is not delivered, and the worktree and branch stay as any unmerged one does: no
//! receipt, one that does not read or is of another task, another head, a retained head gone or
//! moved, a commit that is not the receipt's, a remote that never held it. An import's commit has
//! two parents and its head is in main's history, which ancestry proves. This module reads: it
//! writes no receipt, moves no reference, fetches nothing and asks no host anything. Tests:
//! `crates/tasks/tests/sweep/delivered.rs`.

use super::{git, trigger};
use crate::land::delivery::{Journal, RETAINED, Receipt, Stage};
use crate::land::{FOLDER, REMOTE};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The deliveries of one repository, read only when a head is not in main, once per commit.
pub struct Delivered {
    root: PathBuf,
    /// The lands' folder; none where `root` is in no repository.
    lands: Option<PathBuf>,
    proved: BTreeMap<String, Option<String>>,
}

impl Delivered {
    pub fn of(root: &Path) -> Delivered {
        let lands = crate::git::common_dir(root)
            .ok()
            .map(|common| trigger::folder(&common).join(FOLDER));
        Delivered {
            root: root.to_path_buf(),
            lands,
            proved: BTreeMap::new(),
        }
    }

    /// How `head` was delivered into `main`, a commit of the default branch here, when a receipt
    /// git bears out says it was: `squashed into main as S by pull request #N` ([`said`]).
    pub fn proof(&mut self, head: &str, main: &str) -> Option<String> {
        if !object(head) {
            return None;
        }
        if let Some(known) = self.proved.get(head) {
            return known.clone();
        }
        let proof = {
            let lands = self.lands.as_deref()?;
            let at = format!("--points-at={head}");
            let under = RETAINED.trim_end_matches('/');
            let retained = ["for-each-ref", &at, "--format=%(refname)", under];
            let listed = git::stdout(&self.root, &retained).unwrap_or_default();
            (listed.lines())
                .filter_map(|name| Receipt::read(lands, name.strip_prefix(RETAINED)?))
                .find(|receipt| receipt.head == head && borne_out(&self.root, receipt, main))
                .map(|receipt| said(&receipt.squash, receipt.pull))
        };
        self.proved.insert(head.to_string(), proof.clone());
        proof
    }

    /// Why the branch `name` stays while a delivery of its task is under way and not yet asked to
    /// merge, if one is: a land run again then reads the branch's tip against the pull request
    /// it is bound to. From [`Stage::MergeRequested`] on it goes on from its journal alone and
    /// never reads the branch, so nothing keeps it: a branch kept once its worktree went would be
    /// kept for good, as only the pass that removes its worktree deletes it.
    pub fn under_way(&self, name: &str) -> Option<String> {
        let journal = Journal::read(self.lands.as_deref()?, name)
            .filter(|journal| journal.stage < Stage::MergeRequested)?;
        Some(format!(
            "a delivery of it by pull request is not yet asked to merge, and `just land {}` run \
             again reads its tip",
            journal.slug
        ))
    }
}

/// How a line says a squash: `squashed into main as S by pull request #N`.
fn said(squash: &str, pull: i64) -> String {
    let short = squash.get(..12).unwrap_or(squash);
    format!("squashed into main as {short} by pull request #{pull}")
}

/// Whether `id` is a whole object id, SHA-1's or SHA-256's.
fn object(id: &str) -> bool {
    matches!(id.len(), 40 | 64) && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether git, in `root`, bears out all `receipt` says of its head and `main` holds its squash.
fn borne_out(root: &Path, receipt: &Receipt, main: &str) -> bool {
    let (head, squash, parent, tree) = (
        &receipt.head,
        &receipt.squash,
        &receipt.parent,
        &receipt.tree,
    );
    if ![squash, parent, tree].into_iter().all(|id| object(id)) {
        return false;
    }
    let recorded = format!("{parent}\t{tree}");
    let commit = git::stdout(root, &["log", "-1", "--format=%P%x09%T", squash]);
    if commit.as_deref().map(str::trim) != Some(recorded.as_str()) {
        return false;
    }
    let merge = git::stdout(root, &["merge-tree", "--write-tree", parent, head]);
    merge.as_deref().and_then(|out| out.lines().next()) == Some(tree.as_str())
        && git::ancestor(root, squash, &format!("refs/remotes/{REMOTE}/main"))
        && git::ancestor(root, squash, main)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_head_and_a_folder_in_no_repository_prove_nothing() {
        let nowhere =
            std::env::temp_dir().join(format!("symphony-delivered-{}", std::process::id()));
        std::fs::create_dir_all(&nowhere).unwrap();
        let mut delivered = Delivered::of(&nowhere);
        assert_eq!(delivered.proof("", "main"), None);
        assert_eq!(delivered.proof("main", "main"), None, "a name, not an id");
        assert_eq!(delivered.proof(&"a".repeat(40), "main"), None);
        assert_eq!(delivered.under_way("tools/any"), None);
        std::fs::remove_dir(&nowhere).unwrap();
    }

    #[test]
    fn an_object_id_is_whole_and_hexadecimal() {
        assert!(object(&"0123456789abcdef".repeat(4)[..40]));
        assert!(object(&"f".repeat(64)));
        for not in [
            "",
            "main",
            "abc1234",
            &"a".repeat(39),
            &"a".repeat(41),
            &"g".repeat(40),
            &format!("--output={}", "a".repeat(31)),
        ] {
            assert!(!object(not), "{not}");
        }
    }

    #[test]
    fn a_squash_is_said_short_with_its_pull_request() {
        assert_eq!(
            said(&"b".repeat(40), 9),
            "squashed into main as bbbbbbbbbbbb by pull request #9"
        );
        assert_eq!(
            said("abc", 9),
            "squashed into main as abc by pull request #9"
        );
    }
}
