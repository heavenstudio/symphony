//! Branch document routes preserve their project folder and save destination.

use super::{branch, card, commit, isolated, post, register, repository};
use serde_json::json;
use symphony::{branches, docview, render::Render};

pub(super) fn branch_card_in_project_folder() {
    isolated(
        "a_branch_card_in_a_projects_own_folder_is_linked_read_and_saved_by_its_route",
        |_| {
            let tree = repository();
            let file = "manual/DECISIONS.md";
            // the candidates a card shows are drawn by Star Foundry's picture kind; the folder
            // is spelled as a person may write it
            tree.write(
                "symphony.toml",
                "[[entrypoints]]\nurl = \"/docs/\"\nfolder = \"./manual\"\n[plugins]\ncrates = [\"star-foundry-symphony\"]\n",
            );
            tree.write(file, &register(&card("alpha")));
            commit(&tree.root, "base", 1);
            let wt = branch(&tree, "design/policy", "policy");
            let beta = card("beta").replace("Posted.", "Posted. ![Candidate](proposal.png)");
            std::fs::write(wt.join(file), register(&(card("alpha") + &beta))).unwrap();
            let cards = branches::branch_cards(&tree.root);
            assert_eq!(
                cards
                    .iter()
                    .map(|c| (c.card.slug.as_str(), c.path()))
                    .collect::<Vec<_>>(),
                [("beta", "@design~policy/docs/DECISIONS.md".to_string())]
            );
            let snapshot = symphony::dashboard::snapshot(&tree.root, &cards, "test").unwrap();
            let proposed = snapshot
                .queue
                .waiting
                .iter()
                .find(|c| c.slug == "beta")
                .unwrap();
            assert_eq!(proposed.doc_title, "Decisions");
            assert_eq!(proposed.href(), "/@design~policy/docs/DECISIONS.md#beta");
            assert_eq!(
                proposed.pictures[0].src.as_deref(),
                Some("/@design~policy/docs/proposal.png")
            );
            assert_eq!(
                snapshot
                    .pending
                    .items
                    .iter()
                    .map(|item| item.href.as_str())
                    .collect::<Vec<_>>(),
                [
                    "/docs/DECISIONS.md#alpha",
                    "/@design~policy/docs/DECISIONS.md#beta"
                ]
            );
            let read =
                docview::document(&Render::new(&wt), "docs/DECISIONS.md", "design~policy").unwrap();
            assert_eq!(
                read.cards
                    .iter()
                    .map(|c| c.slug.as_str())
                    .collect::<Vec<_>>(),
                ["alpha", "beta"]
            );
            let saves = symphony::save::Saves::start(&tree.root);
            post(
                &saves,
                "",
                "/api/decision",
                json!({"path":"@design~policy/docs/DECISIONS.md", "slug":"beta", "state":"approved", "note":"Use this policy"}),
            );
            let physical = saves.post(
                "design~policy",
                "/api/decision",
                &json!({"path":file, "slug":"alpha", "state":"approved", "note":""}),
            );
            assert_eq!(
                physical.map_err(|failure| failure.error),
                Err("path must be a markdown file under docs/".to_string())
            );
            saves.close();
            assert!(
                std::fs::read_to_string(wt.join(file))
                    .unwrap()
                    .contains("owner · approved — Use this policy")
            );
            assert!(!tree.read(file).contains("Use this policy"));
        },
    );
}
