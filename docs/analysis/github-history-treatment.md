# Historical personal-path treatment

This is the scratch-only evidence for deciding how to treat the historical path findings in the
[public-readiness audit](public-readiness.md). It grants no publication clearance and changes no
real repository ref, release tag, board, credential, setting or consuming checkout.

The [recorded decision](#github-historical-path-treatment) is **A: retain existing
public history and clean current assumptions/future outputs**. The scratch rewrite below is
comparison evidence, not an operation to apply. Existing release targets, consumer pins and
evidence identities remain intact; the historical paths alone do not require a rewrite to
complete readiness remediation.

The confirmed material is a personal home-directory prefix in twelve historical client WASM
blobs and one retired shipped-skill text blob. The [audit manifest](public-readiness.md#historical-path-manifest)
identifies each without its private value. Classification found 863 build/cache paths and 261
source-checkout paths across those blobs. These are privacy/configuration disclosures, **not
confirmed credentials**. Generic local paths alone do not establish credential exposure. The
initial secret-scan candidates were public WebSocket test samples; no credential revocation is
indicated by that evidence.

## Public refs and the known consumer

The read-only advertised-ref snapshot has thirteen distinct public heads/tags (24 advertised
lines including HEAD and peeled annotated-tag targets). All thirteen identified blobs are
reachable through every one of those histories. No identified blob appears in main's current
tree or in any of the ten release-tag trees. The two older public branches each still have six
affected WASM blobs in their tip tree.

| Public ref | Target commit before scratch cleanup |
| --- | --- |
| `main` | `3b460adb62827f674669e8e99f3090561107264f` |
| `tools/pipeline-review-alarm` | `30436e069f06e4ee5b44a3e657a41e5a78f3dcd8` |
| `tools/pipeline-review-alarm-sdk` | `30436e069f06e4ee5b44a3e657a41e5a78f3dcd8` |
| `release-2026-11-13` | `5f46d97de30c10c4c839171400a10b77d654ec33` |
| `release-2026-11-13.2` | `4056a635f227f2d0a616ebb3de251111fc515a5a` |
| `release-2026-11-14` | `7394675c0cdedabd1704476ad744ff5c329de789` |
| `release-2026-11-14.2` | `0dfa888ad6bc1a2431545de58b8fdfa29261564e` |
| `release-2026-11-14.3` | `fd70821942b4c97d1e17b420217ada28988e27da` |
| `release-2026-11-15` | `c944025a89b5f9e3404f6ade29d11860acba3eb3` |
| `release-2026-11-15.2` | `7a8724f9ecd4a9dd5459767a07d366453872e97a` |
| `release-2026-11-16` | `672d99abdad8de30a894a069dc7738d8894edd87` |
| `release-2026-11-17` | `a34ec21cd48f98eff0670ca870c100da6357e2fc` |
| `release-2026-11-17.2` | `c90a885dc99e02487b0d730c83eadb8dac36ade8` |

The machine registry names Symphony and one consuming project. Read-only inspection of that
consumer's committed main `b3a0ad2c19238b6692850d56cf23f097304d2316` found ten Symphony-family
manifest dependencies at `rev = 46a1d2af60ea320aeb1b6b4423ad67d9fb2246f9`; Cargo.lock resolves
them to the same commit. That pin's history reaches all thirteen blobs, although its current tree
contains none of them. The dependencies are process-cwd, repository-read, symphony, symphony-api,
symphony-args, symphony-board, symphony-docs, symphony-plugin, symphony-schema and symphony-server.
No file, lock or cache in that checkout was changed. Other people's clones, unregistered
consumers and unadvertised PR refs remain unknown.

## Scratch experiment

A bare mirror was cloned from the local repository with `--no-hardlinks` into an owned temporary
directory. Its local-source remote was removed before rewriting, and it had no remote afterward.
It held 376 refs and 3,828 commits. This is a separate evidence snapshot from the initial audit;
it does not extend secret or code-review clearance to later commits.

Upstream [git-filter-repo v2.47.0](https://github.com/newren/git-filter-repo/tree/v2.47.0) was
downloaded as a disposable tool, SHA-256
`67447413e273fc76809289111748870b6f6072f08b17efe94863a92d810b7d94`.
The scratch invocation used `--strip-blobs-with-ids` for the twelve identified WASM blobs and
`--replace-text` for the personal home prefix in non-binary text. It also used
`--preserve-commit-hashes`, `--prune-empty never`, `--prune-degenerate never` and
`--replace-refs delete-no-add`, preserving message text and graph shape rather than rewriting
hash references inside history. `--force` applied only to this owned scratch mirror.

Binary contents were removed by identified blob ID; no byte-string replacement was made inside
WASM, which could corrupt its lengths. The retired text keeps its content with a generic home
prefix. None of the 198 distinct DECISIONS.md history blobs changed or disappeared.

| Measurement/check | Result |
| --- | --- |
| Rewrite exit/status | Exit 0; 20.48 seconds reported by the tool. |
| Commit count | 3,828 before and after; 3,338 commit IDs changed. |
| Identified original blobs | All thirteen absent from reachable rewritten objects. |
| Personal-prefix scan | All 25,635 reachable rewritten blobs scanned; zero retain the original personal home prefix. |
| Repository integrity | `git fsck --full --no-reflogs` exit 0. |
| Owner decision history | All 198 original register-history blobs remain reachable byte-for-byte. |
| Main and ten release trees | Tree IDs unchanged, while target commit IDs changed. |
| Two older public branch trees | Changed: each loses six old compiled client blobs and needs retirement or a rebuilt client before use. |
| Known consumer pin | Maps from `46a1d2af60ea320aeb1b6b4423ad67d9fb2246f9` to `6845a557e8ae167c5cfbc2800862bcd3149294ae`; tree ID unchanged. |
| Scratch remote count | Zero. No push attempted. |

For example, main maps to `a99d6ca5b651b3607cac85fa606956ad214d0550`; the two older public
branches map to `976ad5de47baaf9936604b55639176174205c930`. These are demonstration identities
in the scratch graph, not new publication pins. The proof's commit map is retained as working
evidence; any execution must take a fresh inventory and verify its own mapping at the agreed
frozen tip.

## The decision and compatibility cost

The [history-treatment card](#github-historical-path-treatment) records A,
separately from outbound PR delivery. The comparison retains the unselected option's concrete
cost for reference.

| Choice | Privacy effect | Compatibility effect |
| --- | --- | --- |
| **A. Preserve public history; remove personal paths from future outputs — selected** | Existing historical source/cache paths remain public. Fix current machine assumptions and prevent new personal build-path leakage. A later confirmed secret/private-material finding triggers new triage. | Existing Git pins, tags, task evidence and clones keep their identities. No exceptional force update or coordinated clone migration. |
| B. Coordinate targeted history cleanup — not selected | Removes the identified path-bearing objects from the rewritten reachable graph, retaining register text/history. Does not recall copies, forks or cached views already public. | Main, both older heads, all ten tags and the known consumer pin need migration; old compiled-client versions become incomplete. Existing task stamps, links and runs keyed by old SHAs need a disposition. No live rewrite is authorized by the recorded A decision. |

Under B, identical current source trees do not make old commit IDs valid in a fresh clone of the
rewritten graph. The consumer needs a new manifest revision and lockfile through its own gate;
Symphony's executor does not edit it. Every local worktree/session must be accounted for so a
later merge/push cannot restore old ancestry. Reissue/retire affected release refs and any
signatures explicitly, reconcile old board evidence, and define recovery before changing real
refs. Published crate versions, if any by then, are a separate immutable distribution to inspect.

GitHub documents [changed hashes, lost signatures, clone/PR disruption and retained cached
copies](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/removing-sensitive-data-from-a-repository).
Its support policy does not promise purging ordinary non-sensitive paths. No choice can claim
that all previously public copies have been erased.

Earlier decisions checked: the [accepted client distribution timing](client-off-git.md#client-in-the-crate)
keeps Git distribution available until consumers migrate; [consumer versioning](consumer-contract.md#consumer-contract-versioning)
and [delivery](plugin-delivery.md#release-delivery) retain existing pin contracts; the
[picture-storage split](attached-artifacts.md#pictures-in-every-clone) keeps card pictures and owner
history. They did not themselves settle this personal-path scope; the recorded A decision does.
Optional integration and MIT licensing remain settled and are not asked again.

Current media privacy remediation belongs to [[task:docs/public-media-privacy-review]], including
the four timeline-burst wide images with confirmed machine labels. Clearly labelled redacted
derivatives can repair their current presentation without rewriting public history or presenting
an edited capture as an original. That task owns exact image hashes, regions and replacements;
this historical-path decision does not turn unconfirmed media concerns into credentials or grant
general permission to erase historical evidence.

- [r] **github-historical-path-treatment** — Older published binaries and one retired guide contain personal home/source-folder paths. No credential is confirmed. Should removing those paths also rewrite existing public Git history? A: retain existing history, clean current machine assumptions and prevent paths in future outputs; the old non-credential paths remain public. B: coordinate targeted history cleanup, removing the identified binary versions and redacting the retired text. Recommended: A on the present evidence, because B changes release and consumer identities without recovering copies already public. Approve and say A or B in the note, or comment. [[analysis/github-history-treatment#The decision and compatibility cost]]
  [2026-11-17 18:59] agent — The isolated scratch proof removes all thirteen identified blobs and the personal prefix while retaining all 198 register-history blobs and the current main/ten release trees. It changes 3,338 commit IDs, every advertised release target and the known consumer pin; two older branch trees also lose compiled client assets. B requires coordinated refs, consumer manifest/lock updates through its own gate, worktree recovery and a disposition for evidence linked to old SHAs. No real ref, visibility, credential or setting changed.

    **Earlier verdicts checked.** **client-in-the-crate** (A) retains Git distribution until consumers migrate; **consumer-contract-versioning** (B) and **release-delivery** retain the consumer contract. **pictures-in-every-clone** (F) keeps document/card pictures and their history. None decides whether these personal paths justify changing the repository's existing identities. The task requests a sensitive/local-path audit and removal; this card makes the historical scope and its compatibility cost explicit, without treating generic paths as credentials. Containment coordination stays with the orchestrator; B is not a claim that forks, caches or clones can be recalled.
  [2026-11-17 19:14] owner · approved — A
  [2026-11-17 19:19] agent — A recorded. Existing Git history, release tags and consumer commit identities stay intact; the identified old non-credential paths remain historical. We will clean current machine-specific assumptions and prevent personal paths in future outputs through the staffed portability, media and publication-readiness work. No history rewrite or renewed vote is needed.
  [2026-11-17 19:19] agent · resolved
  [2026-11-17 19:21] agent — Recorded A: retain the existing public history and its release/pin identities, while cleaning current machine assumptions and future outputs. The scratch rewrite remains comparison evidence and will not be applied. Setup portability and ordinary current-media privacy fixes proceed in their separately staffed tasks; retaining history does not waive those fixes or grant publication clearance. The historical-path analysis and readiness disposition will reflect this decision without asking it again.