//! Generated-publication recovery and concurrent saves on private repositories.
include!("common/commit_fixture.rs");

#[test]
fn a_slow_publish_does_not_hold_back_the_next_verdict() {
    isolated("a_slow_publish_does_not_hold_back_the_next_verdict", |f| {
        let root = f.repo();
        let base = git(&root, &["rev-parse", "main"]);
        f.generate();
        let gate = Gate::new();
        let notice = Gate::new();
        f.check_gate(&gate);
        f.main_notice(&notice);
        let committer = ClosingCommitter::start(&root);
        let verdict = DOC.replace("[ ]", "[x]");
        committer
            .0
            .submit(f.edit("docs/a.md", DOC, &verdict))
            .unwrap();
        let (release, prepared) = gate.entered();
        assert_ne!(Path::new(&prepared), root);
        assert_eq!(f.main("docs/a.md"), DOC);
        assert!(!root.join("content/one").exists());
        let second = TWO.replace("second proposal.", "second proposal, noted.");
        committer
            .0
            .submit(f.edit("docs/b.md", TWO, &second))
            .unwrap();
        // This hook can only fire after the second verdict reaches main, while the first
        // publication is still blocked. A serialized worker cannot satisfy this handshake.
        let (_, observed) = notice.entered();
        assert_eq!(observed, second.trim_end().replace('\n', "|"));
        assert_eq!(f.main("docs/a.md"), DOC);
        drop(release);
        // The rebuilt output publication also notifies main's ref hook.
        let (_, observed) = notice.entered();
        assert_eq!(observed, second.trim_end().replace('\n', "|"));
        committer.0.close();
        assert_eq!(f.main("docs/a.md"), verdict);
        assert_eq!(f.main("docs/b.md"), second);
        assert_eq!(f.main("content/one"), "approved\n");
        assert_eq!(f.main("view/hull_atlas"), "approved\n");
        assert_linear_main(&root, &base, 2);
        assert!(git(&root, &["status", "--porcelain"]).is_empty());
        assert!(
            !Path::new(&prepared).exists(),
            "private output checkout removed"
        );
        f.clean_workers();
    });
}

#[test]
fn a_slow_land_candidate_does_not_hold_back_an_unrelated_plain_save() {
    isolated(
        "a_slow_land_candidate_does_not_hold_back_an_unrelated_plain_save",
        |f| {
            let root = f.repo();
            f.generate();
            let publish = std::fs::read_to_string(f.home.join("publish.sh")).unwrap();
            let gate = Gate::new();
            let notice = Gate::new();
            f.main_notice(&notice);
            script(
                &f.home.join("publish.sh"),
                &format!(
                    "SF_AFTER_COMMIT_GATE={} {} --exact publication_gate --nocapture\n{publish}",
                    gate.address(),
                    quote(&std::env::current_exe().unwrap())
                ),
            );
            let committer = ClosingCommitter::start(&root);
            let approved = DOC.replace("[ ]", "[x]");
            committer
                .0
                .submit(f.edit("docs/a.md", DOC, &approved))
                .unwrap();
            let (release, _) = gate.entered();
            let later = TWO.replace(
                "second proposal.",
                "second proposal, during land candidate verification.",
            );
            committer
                .0
                .submit(f.edit("docs/b.md", TWO, &later))
                .unwrap();
            let (_, observed) = notice.entered();
            assert_eq!(observed, later.trim_end().replace('\n', "|"));
            assert_eq!(f.main("docs/a.md"), DOC);
            // The notice fires inside the save's main update, before it syncs main's checkout
            // under the write lock; land's dirty-main check must not see that halfway state.
            let writes = symphony::commit::write_lock(&root).unwrap();
            assert!(
                git(&root, &["status", "--porcelain", "--untracked-files=no"]).is_empty(),
                "the plain save finished its checkout update while land was held"
            );
            drop(writes);
            drop(release);
            let (_, _) = notice.entered();
            committer.0.close();
            assert_eq!(f.main("docs/a.md"), approved);
            assert_eq!(f.main("docs/b.md"), later);
            assert_eq!(f.main("content/one"), "approved\n");
            f.clean_workers();
        },
    );
}

#[test]
fn concurrent_generators_keep_private_checkouts_and_drain_same_page_edits() {
    isolated(
        "concurrent_generators_keep_private_checkouts_and_drain_same_page_edits",
        |f| {
            let root = f.repo();
            let gate = Gate::new();
            script(
                &f.home.join("generate.sh"),
                &format!(
                    "if mkdir {} 2>/dev/null; then\n  SF_AFTER_COMMIT_GATE={} {} --exact publication_gate --nocapture\nfi\nmkdir -p content\nif grep -qF -- '- [x] **one**' docs/a.md; then printf 'approved\\n' > content/one; fi",
                    quote(&f.home.join("generation-once")),
                    gate.address(),
                    quote(&std::env::current_exe().unwrap())
                ),
            );
            let first = ClosingCommitter::start(&root);
            let second = ClosingCommitter::start(&root);
            let approved = DOC.replace("[ ]", "[x]");
            first.0.submit(f.edit("docs/a.md", DOC, &approved)).unwrap();
            let (release, checking) = gate.entered();
            // A second worker can generate and commit while the first generator is held.
            let notice = Gate::new();
            f.main_notice(&notice);
            let noted = TWO.replace("second proposal.", "second proposal, concurrent.");
            second.0.submit(f.edit("docs/b.md", TWO, &noted)).unwrap();
            let (_, observed) = notice.entered();
            assert_eq!(observed, noted.trim_end().replace('\n', "|"));
            let later = approved.replace("last paragraph", "last paragraph, queued");
            first
                .0
                .submit(f.edit("docs/a.md", &approved, &later))
                .unwrap();
            drop(release);
            let (_, _) = notice.entered();
            // The same-page follow-up is a separate immutable save, drained before close returns.
            let (_, _) = notice.entered();
            first.0.close();
            second.0.close();
            assert_eq!(f.main("docs/a.md"), later);
            assert_eq!(f.main("docs/b.md"), noted);
            assert_eq!(f.main("content/one"), "approved\n");
            assert!(git(&root, &["status", "--porcelain"]).is_empty());
            assert!(!Path::new(&checking).exists());
            f.clean_workers();
        },
    );
}

#[test]
fn a_failed_publish_restores_main_and_index_and_the_next_verdict_lands() {
    isolated(
        "a_failed_publish_restores_main_and_index_and_the_next_verdict_lands",
        |f| {
            let root = f.repo();
            f.generate();
            // The real publisher boundary refuses after release() has temporarily put the main
            // checkout's document back at HEAD. This exercises restore(false), not a check failure.
            write(&root.join("tools/test_budget.json"), "{}");
            git(&root, &["commit", "-qam", "fixture publisher"]);
            let gate = Gate::new();
            script(
                &f.home.join("publish.sh"),
                &format!(
                    "SF_AFTER_COMMIT_GATE={} {} --exact publication_gate --nocapture\necho 'fixture publisher refuses' >&2\nexit 23",
                    gate.address(),
                    quote(&std::env::current_exe().unwrap())
                ),
            );
            write(&root.join("other"), "unrelated staged work\n");
            git(&root, &["add", "other"]);
            let main = git(&root, &["rev-parse", "main"]);
            let index = git(&root, &["ls-files", "-s"]);
            let committer = ClosingCommitter::start(&root);
            let approved = DOC.replace("[ ]", "[x]");
            committer
                .0
                .submit(f.edit("docs/a.md", DOC, &approved))
                .unwrap();
            let (release, publishing) = gate.entered();
            assert_ne!(Path::new(&publishing), root);
            assert_eq!(
                std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
                DOC,
                "the publication must have released the saved document before refusing"
            );
            let finished = f.pending_job();
            drop(release);
            finished.lock().unwrap();
            assert_eq!(git(&root, &["rev-parse", "main"]), main);
            assert_eq!(git(&root, &["ls-files", "-s"]), index);
            assert_eq!(
                std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
                approved
            );
            assert!(!root.join("content/one").exists());
            let reports = f.store().tasks(None).unwrap();
            assert_eq!(reports.len(), 1, "one visible publication failure");
            assert!(
                reports[0].body.contains("fixture publisher refuses"),
                "{}",
                reports[0].body
            );
            let branches = git(
                &root,
                &[
                    "for-each-ref",
                    "--format=%(refname:short)",
                    "refs/heads/docs/verdict-*",
                ],
            );
            assert_eq!(branches.lines().count(), 1);
            assert_eq!(
                git(&root, &["show", &format!("{branches}:docs/a.md")]) + "\n",
                approved
            );
            assert_eq!(
                git(&root, &["show", &format!("{branches}:content/one")]),
                "approved"
            );
            let next = TWO.replace(
                "second proposal.",
                "second proposal, after refused publication.",
            );
            committer.0.submit(f.edit("docs/b.md", TWO, &next)).unwrap();
            committer.0.close();
            assert_eq!(f.main("docs/b.md"), next);
            assert_eq!(f.main("docs/a.md"), DOC);
            assert_eq!(f.main("other"), "original\n");
            assert_eq!(
                std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
                approved
            );
            assert_eq!(
                git(&root, &["ls-files", "-s", "other"]),
                index.lines().find(|l| l.ends_with("\tother")).unwrap()
            );
            assert_linear_main(&root, &main, 1);
            assert_eq!(
                f.store().tasks(None).unwrap().len(),
                1,
                "the next verdict succeeds without another error"
            );
            f.clean_publication_state();
        },
    );
}

#[test]
fn a_land_retry_is_picked_up_by_the_next_save_without_reverification() {
    isolated(
        "a_land_retry_is_picked_up_by_the_next_save_without_reverification",
        |f| {
            let root = f.repo();
            f.generate();
            let base = git(&root, &["rev-parse", "main"]);
            let publish = std::fs::read_to_string(f.home.join("publish.sh")).unwrap();
            let gate = Gate::new();
            script(
                &f.home.join("publish.sh"),
                &format!(
                    "if mkdir {} 2>/dev/null; then\n SF_AFTER_COMMIT_GATE={} {} --exact publication_gate --nocapture\n echo 'main moved again; retry just land with reusable candidate records retained'\n exit 1\nfi\n{publish}",
                    quote(&f.home.join("first-land")),
                    gate.address(),
                    quote(&std::env::current_exe().unwrap())
                ),
            );
            let committer = ClosingCommitter::start(&root);
            let approved = DOC.replace("[ ]", "[x]");
            committer
                .0
                .submit(f.edit("docs/a.md", DOC, &approved))
                .unwrap();
            let (release, checkout) = gate.entered();
            let commands = f.commands();
            assert_eq!(
                commands
                    .iter()
                    .map(|row| row[0].as_str())
                    .collect::<Vec<_>>(),
                ["verify", "land"]
            );
            assert_eq!(git(&root, &["rev-parse", "main"]), base);
            // Another serving generation must not adopt a publication whose lease is held.
            ClosingCommitter::start(&root).0.close();
            assert_eq!(f.commands(), commands);
            drop(release);
            // This second owner save supplies publication pass 2. It calls land on the retained
            // first branch, not verify or the generators, then drains the later ordinary edit.
            let second = TWO.replace(
                "second proposal.",
                "second proposal, next publication pass.",
            );
            committer
                .0
                .submit(f.edit("docs/b.md", TWO, &second))
                .unwrap();
            committer.0.close();
            let commands = f.commands();
            assert_eq!(
                commands
                    .iter()
                    .map(|row| row[0].as_str())
                    .collect::<Vec<_>>(),
                ["verify", "land", "land"]
            );
            assert_eq!(
                commands[1], commands[2],
                "same branch, actor, checkout and verified tip"
            );
            assert_eq!(f.main("docs/a.md"), approved);
            assert_eq!(f.main("docs/b.md"), second);
            assert_eq!(f.main("content/one"), "approved\n");
            assert_eq!(f.main("view/hull_atlas"), "approved\n");
            assert_linear_main(&root, &base, 2);
            assert!(!Path::new(&checkout).exists());
            f.clean_publication_state();
            assert_eq!(
                std::fs::read_dir(root.join(".git/sf-symphony/verdict-publications"))
                    .unwrap()
                    .count(),
                0
            );
            assert!(
                f.store().tasks(None).unwrap().is_empty(),
                "a bounded retry is queued, not filed as a lost verdict"
            );
        },
    );
}

#[test]
fn a_queued_land_survives_committer_restart_with_its_verified_checkout() {
    isolated(
        "a_queued_land_survives_committer_restart_with_its_verified_checkout",
        |f| {
            let root = f.repo();
            f.generate();
            let base = git(&root, &["rev-parse", "main"]);
            let publish = std::fs::read_to_string(f.home.join("publish.sh")).unwrap();
            script(
                &f.home.join("publish.sh"),
                "echo 'retry just land: main moved again'; exit 1",
            );
            let approved = DOC.replace("[ ]", "[x]");
            let first = ClosingCommitter::start(&root);
            first.0.submit(f.edit("docs/a.md", DOC, &approved)).unwrap();
            first.0.close();
            assert_eq!(git(&root, &["rev-parse", "main"]), base);
            let records: Vec<_> =
                std::fs::read_dir(root.join(".git/sf-symphony/verdict-publications"))
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .collect();
            assert_eq!(records.len(), 1);
            let record: Value =
                serde_json::from_slice(&std::fs::read(&records[0]).unwrap()).unwrap();
            let checkout = Path::new(record["publication"]["checkout"].as_str().unwrap());
            assert!(
                checkout.exists(),
                "the verified worktree and its metadata survive shutdown"
            );
            assert_eq!(record["edit"]["new"], approved);
            assert_eq!(
                f.commands()
                    .iter()
                    .map(|row| row[0].as_str())
                    .collect::<Vec<_>>(),
                ["verify", "land"]
            );
            script(&f.home.join("publish.sh"), &publish);
            let alias = f.home.join("unexpected-checkout-alias");
            std::os::unix::fs::symlink(checkout, &alias).unwrap();
            let mut invalid = record.clone();
            invalid["publication"]["checkout"] = json!(alias);
            write(&records[0], &invalid.to_string());
            ClosingCommitter::start(&root).0.close();
            assert!(
                checkout.exists() && alias.exists(),
                "invalid recovery never removes the checkout"
            );
            assert_eq!(
                f.commands().len(),
                2,
                "invalid recovery must not run a command"
            );
            let reports = f.store().tasks(None).unwrap();
            assert_eq!(reports.len(), 1);
            assert!(
                reports[0]
                    .body
                    .contains("invalid retained publication checkout")
            );
            write(&records[0], &record.to_string());
            // The restarted worker's first publication pass resumes the journal entry without
            // preparing another branch or issuing another verify.
            ClosingCommitter::start(&root).0.close();
            let commands = f.commands();
            assert_eq!(
                commands
                    .iter()
                    .map(|row| row[0].as_str())
                    .collect::<Vec<_>>(),
                ["verify", "land", "land"]
            );
            assert_eq!(commands[1], commands[2]);
            assert_eq!(f.main("docs/a.md"), approved);
            assert_eq!(f.main("content/one"), "approved\n");
            assert!(!checkout.exists());
            assert!(!records[0].exists());
            f.clean_publication_state();
            assert_eq!(
                f.store().tasks(None).unwrap().len(),
                1,
                "successful recovery adds no failure report"
            );
        },
    );
}

#[test]
fn a_later_generation_retries_a_publication_released_after_its_startup() {
    isolated(
        "a_later_generation_retries_a_publication_released_after_its_startup",
        |f| {
            let root = f.repo();
            f.generate();
            let publish = std::fs::read_to_string(f.home.join("publish.sh")).unwrap();
            let gate = Gate::new();
            script(
                &f.home.join("publish.sh"),
                &format!(
                    "SF_AFTER_COMMIT_GATE={} {} --exact publication_gate --nocapture\necho 'retry just land: main moved again'; exit 1",
                    gate.address(),
                    quote(&std::env::current_exe().unwrap())
                ),
            );
            let first = ClosingCommitter::start(&root);
            let approved = DOC.replace("[ ]", "[x]");
            first.0.submit(f.edit("docs/a.md", DOC, &approved)).unwrap();
            let (release, _) = gate.entered();
            let second = ClosingCommitter::start(&root);
            assert_eq!(
                f.commands().len(),
                2,
                "the live owner's lease prevents adoption"
            );
            drop(release);
            first.0.close();
            script(&f.home.join("publish.sh"), &publish);
            let later = TWO.replace(
                "second proposal.",
                "second proposal, on the new generation.",
            );
            // This generation was already alive when the old worker released its journal.
            // Its next save must rescan, adopt and land that verified branch.
            second.0.submit(f.edit("docs/b.md", TWO, &later)).unwrap();
            second.0.close();
            let commands = f.commands();
            assert_eq!(
                commands
                    .iter()
                    .map(|row| row[0].as_str())
                    .collect::<Vec<_>>(),
                ["verify", "land", "land"]
            );
            assert_eq!(commands[1], commands[2]);
            assert_eq!(f.main("docs/a.md"), approved);
            assert_eq!(f.main("docs/b.md"), later);
            assert_eq!(f.main("content/one"), "approved\n");
            f.clean_publication_state();
            assert!(f.store().tasks(None).unwrap().is_empty());
        },
    );
}

#[test]
fn a_completed_publication_is_not_repeated_after_restart() {
    isolated(
        "a_completed_publication_is_not_repeated_after_restart",
        |f| {
            let root = f.repo();
            f.generate();
            let publish = std::fs::read_to_string(f.home.join("publish.sh")).unwrap();
            script(&f.home.join("publish.sh"), "echo 'retry just land'; exit 1");
            let first = ClosingCommitter::start(&root);
            let approved = DOC.replace("[ ]", "[x]");
            first.0.submit(f.edit("docs/a.md", DOC, &approved)).unwrap();
            first.0.close();
            let records: Vec<_> =
                std::fs::read_dir(root.join(".git/sf-symphony/verdict-publications"))
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .collect();
            assert_eq!(records.len(), 1);
            let record: Value =
                serde_json::from_slice(&std::fs::read(&records[0]).unwrap()).unwrap();
            let checkout = Path::new(record["publication"]["checkout"].as_str().unwrap());
            // Model a process ending after land advanced main but before journal removal.
            // Use the fixture publisher, not a certificate or a fake result record.
            git(&root, &["checkout", "-q", "HEAD", "--", "docs/a.md"]);
            script(&f.home.join("publish.sh"), &publish);
            let done = tasks::sys::output(
                tasks::sys::Command::new("sh")
                    .arg(f.home.join("publish.sh"))
                    .arg(record["publication"]["branch"].as_str().unwrap())
                    .current_dir(checkout),
            )
            .unwrap();
            assert!(done.status.success(), "{done:?}");
            assert_eq!(f.main("docs/a.md"), approved);
            assert!(checkout.exists() && records[0].exists());
            let commands = f.commands();
            ClosingCommitter::start(&root).0.close();
            assert_eq!(
                f.commands(),
                commands,
                "an already landed publication is not run again"
            );
            assert!(!records[0].exists() && !checkout.exists());
            f.clean_workers();
        },
    );
}

#[test]
fn a_failed_output_check_keeps_the_verdict_branch_and_reports_the_failure() {
    isolated(
        "a_failed_output_check_keeps_the_verdict_branch_and_reports_the_failure",
        |f| {
            let root = f.repo();
            f.generate();
            script(
                &f.home.join("check.sh"),
                "echo 'fixture content check refused' >&2\nexit 23",
            );
            let before = git(&root, &["rev-parse", "main"]);
            let committer = ClosingCommitter::start(&root);
            let verdict = DOC.replace("[ ]", "[x]");
            committer
                .0
                .submit(f.edit("docs/a.md", DOC, &verdict))
                .unwrap();
            committer.0.close();
            assert_eq!(git(&root, &["rev-parse", "main"]), before);
            assert_eq!(
                std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
                verdict
            );
            let branches = git(
                &root,
                &[
                    "for-each-ref",
                    "--format=%(refname:short)",
                    "refs/heads/docs/verdict-*",
                ],
            );
            let branches: Vec<_> = branches.lines().collect();
            assert_eq!(
                branches.len(),
                1,
                "the unpublished edit remains recoverable"
            );
            assert_eq!(
                git(&root, &["show", &format!("{}:docs/a.md", branches[0])]) + "\n",
                verdict
            );
            assert_eq!(
                git(&root, &["show", &format!("{}:content/one", branches[0])]),
                "approved"
            );
            let reports = f.store().tasks(None).unwrap();
            assert_eq!(reports.len(), 1, "one visible failure report");
            let commands = f.commands();
            assert_eq!(commands.len(), 1, "a red verify never calls land");
            assert_eq!(commands[0][0], "verify");
            let archives: Vec<_> =
                std::fs::read_dir(root.join(".git/sf-symphony/verdict-publications"))
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .filter(|path| path.extension().is_some_and(|ext| ext == "failed"))
                    .collect();
            assert_eq!(archives.len(), 1, "retain the immutable failed draft");
            let saved: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&archives[0]).unwrap()).unwrap();
            assert_eq!(saved["edit"]["new"], verdict);
            assert!(reports[0].body.contains(archives[0].to_str().unwrap()));
            assert!(
                reports[0].body.contains(branches[0])
                    && reports[0].body.contains("fixture content check refused"),
                "{}",
                reports[0].body
            );
            let listing = git(&root, &["worktree", "list", "--porcelain"]);
            assert_eq!(
                listing
                    .lines()
                    .filter(|l| l.starts_with("worktree "))
                    .count(),
                1,
                "{listing}"
            );
            assert_eq!(
                std::fs::read_dir(root.join(".git/sf-symphony/verdict-backlog"))
                    .unwrap()
                    .count(),
                1
            );
        },
    );
}

#[test]
fn a_plain_save_preparing_across_land_keeps_its_page_and_a_clean_checkout() {
    isolated(
        "a_plain_save_preparing_across_land_keeps_its_page_and_a_clean_checkout",
        |f| {
            let root = f.repo();
            f.generate();
            let preparing = Gate::new();
            let generator = std::fs::read_to_string(f.home.join("generate.sh")).unwrap();
            script(
                &f.home.join("generate.sh"),
                &format!(
                    "if grep -qF 'plain save in flight' docs/b.md && mkdir {} 2>/dev/null; then SF_AFTER_COMMIT_GATE={} {} --exact publication_gate --nocapture; fi\n{generator}",
                    quote(&f.home.join("plain-prepared-once")),
                    preparing.address(),
                    quote(&std::env::current_exe().unwrap())
                ),
            );
            let verifying = Gate::new();
            f.check_gate(&verifying);
            let landing = Gate::new();
            let publisher = std::fs::read_to_string(f.home.join("publish.sh")).unwrap();
            script(
                &f.home.join("publish.sh"),
                &format!(
                    "SF_AFTER_COMMIT_GATE={} {} --exact publication_gate --nocapture\n{publisher}",
                    landing.address(),
                    quote(&std::env::current_exe().unwrap())
                ),
            );
            let committer = ClosingCommitter::start(&root);
            let approved = DOC.replace("[ ]", "[x]");
            committer
                .0
                .submit(f.edit("docs/a.md", DOC, &approved))
                .unwrap();
            let (verify, _) = verifying.entered();
            let note = TWO.replace("second proposal.", "second proposal, plain save in flight.");
            committer.0.submit(f.edit("docs/b.md", TWO, &note)).unwrap();
            let (prepare, _) = preparing.entered();
            let done = f.pending_job_for("docs/b.md");
            drop(verify);
            let (land, _) = landing.entered();
            assert_eq!(
                std::fs::read_to_string(root.join("docs/b.md")).unwrap(),
                note,
                "the fast worker's page stays in place across release"
            );
            drop(prepare);
            done.lock().unwrap();
            done.unlock().unwrap();
            assert_eq!(
                f.main("docs/b.md"),
                note,
                "the plain save committed while land is held"
            );
            assert_eq!(
                std::fs::read_to_string(root.join("docs/b.md")).unwrap(),
                note,
                "synchronization must not mistake a released page for a later owner edit"
            );
            assert!(git(&root, &["status", "--porcelain"]).is_empty());
            drop(land);
            committer.0.close();
            assert_eq!(f.main("docs/a.md"), approved);
            assert_eq!(f.main("docs/b.md"), note);
            assert_eq!(
                std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
                approved
            );
            assert_eq!(
                std::fs::read_to_string(root.join("docs/b.md")).unwrap(),
                note
            );
            assert!(git(&root, &["status", "--porcelain"]).is_empty());
            assert_eq!(
                f.commands()
                    .iter()
                    .map(|row| row[0].as_str())
                    .collect::<Vec<_>>(),
                ["verify", "land"]
            );
            f.clean_workers();
        },
    );
}

#[test]
fn repeated_dirty_publications_report_once_across_worker_restarts() {
    isolated(
        "repeated_dirty_publications_report_once_across_worker_restarts",
        |f| {
            let root = f.repo();
            f.generate();
            let publisher = std::fs::read_to_string(f.home.join("publish.sh")).unwrap();
            // Different from the released HEAD: the refusal must really be tracked dirt.
            let dirty = DOC.replace("last paragraph", "last paragraph, lasting dirty edit");
            script(
                &f.home.join("publish.sh"),
                &format!(
                    "printf '%s' {} > {}\n{publisher}",
                    quote(Path::new(&dirty)),
                    quote(&root.join("docs/a.md"))
                ),
            );
            let committer = ClosingCommitter::start(&root);
            let approved = DOC.replace("[ ]", "[x]");
            committer
                .0
                .submit(f.edit("docs/a.md", DOC, &approved))
                .unwrap();
            committer.0.close();
            assert!(
                f.store().tasks(None).unwrap().is_empty(),
                "one transient own-page refusal only queues"
            );
            ClosingCommitter::start(&root).0.close();
            let reports = f.store().tasks(None).unwrap();
            assert_eq!(reports.len(), 1);
            assert!(
                reports[0]
                    .body
                    .contains("remains queued after dirty-main refusal")
            );
            ClosingCommitter::start(&root).0.close();
            assert_eq!(
                f.store().tasks(None).unwrap().len(),
                1,
                "the journal remembers its report across recovery"
            );
            assert_eq!(f.main("docs/a.md"), DOC);
            let commands = f.commands();
            assert_eq!(
                commands
                    .iter()
                    .map(|row| row[0].as_str())
                    .collect::<Vec<_>>(),
                ["verify", "land", "land", "land"]
            );
            assert_eq!(commands[1], commands[2]);
            assert_eq!(commands[2], commands[3]);
            assert!(
                reports[0]
                    .body
                    .contains(commands[1][1].split_whitespace().nth(1).unwrap())
            );
            let journals: Vec<_> =
                std::fs::read_dir(root.join(".git/sf-symphony/verdict-publications"))
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .collect();
            assert_eq!(journals.len(), 1);
            let saved: Value =
                serde_json::from_slice(&std::fs::read(&journals[0]).unwrap()).unwrap();
            assert_eq!(saved["dirty_retry"], true);
            assert_eq!(saved["dirty_reported"], true);
            assert_eq!(saved["edit"]["new"], approved);
        },
    );
}

#[test]
fn a_dirty_path_outside_queued_pages_reports_on_the_first_refusal() {
    isolated(
        "a_dirty_path_outside_queued_pages_reports_on_the_first_refusal",
        |f| {
            let root = f.repo();
            f.generate();
            write(&root.join("other"), "unrelated tracked work\n");
            let committer = ClosingCommitter::start(&root);
            let approved = DOC.replace("[ ]", "[x]");
            committer
                .0
                .submit(f.edit("docs/a.md", DOC, &approved))
                .unwrap();
            committer.0.close();
            let reports = f.store().tasks(None).unwrap();
            assert_eq!(
                reports.len(),
                1,
                "foreign tracked dirt is actionable on its first refusal"
            );
            assert!(reports[0].body.contains("other"));
            ClosingCommitter::start(&root).0.close();
            assert_eq!(f.store().tasks(None).unwrap().len(), 1);
            assert_eq!(
                std::fs::read_to_string(root.join("other")).unwrap(),
                "unrelated tracked work\n"
            );
            assert_eq!(f.main("other"), "original\n");
            assert_eq!(f.main("docs/a.md"), DOC);
            assert_eq!(
                f.commands()
                    .iter()
                    .map(|row| row[0].as_str())
                    .collect::<Vec<_>>(),
                ["verify", "land", "land"]
            );
        },
    );
}

#[test]
fn queued_generated_pages_land_with_a_tracked_clean_checkout() {
    isolated(
        "queued_generated_pages_land_with_a_tracked_clean_checkout",
        |f| {
            let root = f.repo();
            let preparing = Gate::new();
            script(
                &f.home.join("generate.sh"),
                &format!(
                    "if grep -qF -- '- [x] **two**' docs/b.md && mkdir {} 2>/dev/null; then SF_AFTER_COMMIT_GATE={} {} --exact publication_gate --nocapture; fi\nmkdir -p content\nif grep -qF -- '- [x] **one**' docs/a.md; then printf 'approved\\n' > content/one; fi\nif grep -qF -- '- [x] **two**' docs/b.md; then printf 'approved\\n' > content/two; fi",
                    quote(&f.home.join("prepared-second-once")),
                    preparing.address(),
                    quote(&std::env::current_exe().unwrap())
                ),
            );
            let landing = Gate::new();
            let publish = std::fs::read_to_string(f.home.join("publish.sh")).unwrap();
            script(
                &f.home.join("publish.sh"),
                &format!(
                    "if mkdir {} 2>/dev/null; then SF_AFTER_COMMIT_GATE={} {} --exact publication_gate --nocapture; fi\n{publish}",
                    quote(&f.home.join("land-held-once")),
                    landing.address(),
                    quote(&std::env::current_exe().unwrap())
                ),
            );
            let gate = Gate::new();
            f.check_gate(&gate);
            let committer = ClosingCommitter::start(&root);
            let first = DOC.replace("[ ]", "[x]");
            let second = TWO.replace("[ ]", "[x]");
            committer
                .0
                .submit(f.edit("docs/a.md", DOC, &first))
                .unwrap();
            let (release, _) = gate.entered();
            committer
                .0
                .submit(f.edit("docs/b.md", TWO, &second))
                .unwrap();
            let (prepared, _) = preparing.entered();
            assert_eq!(f.main("docs/a.md"), DOC);
            assert_eq!(f.main("docs/b.md"), TWO);
            assert_eq!(
                std::fs::read_to_string(root.join("docs/b.md")).unwrap(),
                second,
                "the second generated page is dirty while the first verifies"
            );
            let drafts: Vec<Value> =
                std::fs::read_dir(root.join(".git/sf-symphony/verdict-publications"))
                    .unwrap()
                    .filter_map(|entry| {
                        let path = entry.unwrap().path();
                        (path
                            .extension()
                            .is_some_and(|extension| extension == "json"))
                        .then(|| serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap())
                    })
                    .collect();
            assert!(
                drafts
                    .iter()
                    .any(|draft| draft["edit"]["rel"] == "docs/b.md"
                        && draft["edit"]["base"] == TWO
                        && draft["edit"]["new"] == second
                        && draft["publication"].is_null()),
                "the queued page is durable before release or generation"
            );
            drop(release);
            let (landed, _) = landing.entered();
            assert_eq!(
                std::fs::read_to_string(root.join("docs/b.md")).unwrap(),
                second,
                "a still-preparing page is not released as an output-worker page"
            );
            drop(prepared);
            drop(landed);
            committer.0.close();
            assert_eq!(f.main("docs/a.md"), first);
            assert_eq!(f.main("docs/b.md"), second);
            assert_eq!(f.main("content/one"), "approved\n");
            assert_eq!(f.main("content/two"), "approved\n");
            assert_eq!(
                f.commands()
                    .iter()
                    .map(|row| row[0].as_str())
                    .collect::<Vec<_>>(),
                ["verify", "land", "land", "verify", "land"]
            );
            assert!(git(&root, &["status", "--porcelain"]).is_empty());
            f.clean_publication_state();
            assert!(f.store().tasks(None).unwrap().is_empty());
        },
    );
}

#[test]
fn a_tap_during_land_keeps_the_verified_branch_for_the_next_pass() {
    isolated(
        "a_tap_during_land_keeps_the_verified_branch_for_the_next_pass",
        |f| {
            let root = f.repo();
            f.generate();
            let gate = Gate::new();
            let publish = std::fs::read_to_string(f.home.join("publish.sh")).unwrap();
            script(
                &f.home.join("publish.sh"),
                &format!(
                    "if mkdir {} 2>/dev/null; then\n SF_AFTER_COMMIT_GATE={} {} --exact publication_gate --nocapture\nfi\n{publish}",
                    quote(&f.home.join("land-held-once")),
                    gate.address(),
                    quote(&std::env::current_exe().unwrap())
                ),
            );
            let committer = ClosingCommitter::start(&root);
            let approved = DOC.replace("[ ]", "[x]");
            committer
                .0
                .submit(f.edit("docs/a.md", DOC, &approved))
                .unwrap();
            let (release, _) = gate.entered();
            assert_eq!(
                std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
                DOC,
                "land has released the page before the tap"
            );
            let tapped = DOC.replace("last paragraph", "last paragraph, owner tap during land");
            committer
                .0
                .submit(f.edit("docs/a.md", DOC, &tapped))
                .unwrap();
            drop(release);
            committer.0.close();
            let expected =
                approved.replace("last paragraph", "last paragraph, owner tap during land");
            assert_eq!(f.main("docs/a.md"), expected);
            assert_eq!(
                std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
                expected
            );
            assert_eq!(f.main("content/one"), "approved\n");
            let commands = f.commands();
            assert_eq!(
                commands
                    .iter()
                    .map(|row| row[0].as_str())
                    .collect::<Vec<_>>(),
                ["verify", "land", "land"]
            );
            assert_eq!(
                commands[1], commands[2],
                "dirty-main retry keeps the verified branch and actor"
            );
            f.clean_publication_state();
            assert!(
                f.store().tasks(None).unwrap().is_empty(),
                "a tap must not trigger terminal failure filing"
            );
        },
    );
}

#[test]
fn draft_recovery_restores_generated_and_plain_saves_from_a_released_checkout() {
    isolated(
        "draft_recovery_restores_generated_and_plain_saves_from_a_released_checkout",
        |f| {
            let root = f.repo();
            f.generate();
            let draft =
                root.join(".git/sf-symphony/verdict-publications/00000000000000000000.json");
            let approved = DOC.replace("[ ]", "[x]");
            write(
                &draft,
                &json!({"edit":{"rel":"docs/a.md","base":DOC,"new":approved},"publication":null})
                    .to_string(),
            );
            assert_eq!(
                std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
                DOC,
                "the process stopped with the source page temporarily released"
            );
            let viewer = f.home.join("viewer");
            git(
                &root,
                &[
                    "worktree",
                    "add",
                    "-qb",
                    "work/viewer",
                    viewer.to_str().unwrap(),
                ],
            );
            ClosingCommitter::start(&viewer).0.close();
            assert!(draft.exists(), "a branch viewer cannot adopt main's draft");
            assert_eq!(
                std::fs::read_to_string(viewer.join("docs/a.md")).unwrap(),
                DOC
            );
            assert_eq!(
                std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
                DOC
            );
            git(&root, &["worktree", "remove", viewer.to_str().unwrap()]);
            ClosingCommitter::start(&root).0.close();
            assert_eq!(f.main("docs/a.md"), approved);
            assert_eq!(
                std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
                approved
            );
            assert_eq!(
                f.commands()
                    .iter()
                    .map(|row| row[0].as_str())
                    .collect::<Vec<_>>(),
                ["verify", "land"]
            );
            let note = TWO.replace("second proposal.", "second proposal, recovered plain save.");
            write(
                &draft,
                &json!({"edit":{"rel":"docs/b.md","base":TWO,"new":note},"publication":null})
                    .to_string(),
            );
            ClosingCommitter::start(&root).0.close();
            assert_eq!(f.main("docs/b.md"), note);
            assert_eq!(
                std::fs::read_to_string(root.join("docs/b.md")).unwrap(),
                note
            );
            assert_eq!(
                f.commands()
                    .iter()
                    .map(|row| row[0].as_str())
                    .collect::<Vec<_>>(),
                ["verify", "land"],
                "recovered plain saves still use the direct commit path"
            );
            assert!(!draft.exists());
            assert!(git(&root, &["status", "--porcelain"]).is_empty());
            f.clean_workers();
        },
    );
}

#[test]
fn a_bad_journal_does_not_block_good_publications_or_file_repeated_failures() {
    isolated(
        "a_bad_journal_does_not_block_good_publications_or_file_repeated_failures",
        |f| {
            let root = f.repo();
            f.generate();
            let publish = std::fs::read_to_string(f.home.join("publish.sh")).unwrap();
            script(&f.home.join("publish.sh"), "echo 'retry just land'; exit 1");
            let first = ClosingCommitter::start(&root);
            let approved = DOC.replace("[ ]", "[x]");
            first.0.submit(f.edit("docs/a.md", DOC, &approved)).unwrap();
            first.0.close();
            let bad = root.join(".git/sf-symphony/verdict-publications/00000000000000000000.json");
            assert!(!bad.exists());
            write(&bad, "");
            script(&f.home.join("publish.sh"), &publish);
            let resumed = ClosingCommitter::start(&root);
            resumed.0.close();
            assert_eq!(
                f.main("docs/a.md"),
                approved,
                "a later good journal must still land"
            );
            assert_eq!(
                f.commands()
                    .iter()
                    .map(|row| row[0].as_str())
                    .collect::<Vec<_>>(),
                ["verify", "land", "land"]
            );
            let again = ClosingCommitter::start(&root);
            let note = TWO.replace("second proposal.", "second proposal, after a bad journal.");
            again.0.submit(f.edit("docs/b.md", TWO, &note)).unwrap();
            again.0.close();
            assert_eq!(f.main("docs/b.md"), note);
            assert_eq!(
                std::fs::read_to_string(&bad).unwrap(),
                "",
                "bad evidence is preserved"
            );
            assert!(bad.with_extension("reported").exists());
            let reports = f.store().tasks(None).unwrap();
            assert_eq!(
                reports.len(),
                1,
                "one bad journal gets one report across passes and restarts"
            );
            assert!(reports[0].body.contains("00000000000000000000.json"));
            f.clean_publication_state();
        },
    );
}
