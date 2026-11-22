# Public-readiness audit

This initial read-only audit and remediation plan was recorded on 2026-11-17 for the GitHub
integration task. It is not publication clearance. The [integration proposal](github-integration.md)
is separate from repository readiness.

**The repository was already public when inspected.** `gh repo view heavenstudio/symphony`
reported PUBLIC, `isPrivate: false`, main as default branch, MIT recognized, issues enabled and
wiki disabled; the initial finding was recorded at 17:37 on the task thread on 2026-11-17.
A read-only visibility/license recheck at **2026-11-17 17:49:20 UTC** again returned PUBLIC,
`isPrivate: false` and MIT. This is verified exposure context, not a visibility change by this
session. The remaining work is review and remediation of existing exposure.

## Scope and method

Tracked snapshot: `837a0d36c8d8287860b44c7913c35a698779ac80`, 2,024 files, including 47 docs
Markdown files, 70 PNGs, 81 WebPs and 17 SVGs. The separately captured local ref inventory held
378 refs, 3,816 reachable commits, 59,068 objects and 25,612 unique blobs (1,615,349,278
uncompressed blob bytes). The repository was not shallow. `git ls-remote origin` advertised 24
refs; every tip was present locally. These counts describe audit snapshots of a moving repository.
The saved `object-ID ref-name` inventory has SHA-256
`9d498b0db26293b91fd0ccb814e5da2714c09b6c5c9bb9cd83787c9621ad3ba2`.

All scan commands and working files were disposable, outside the repository. No scanner or
runtime change is committed.

| Read | Method and coverage |
| --- | --- |
| Tracked files | Isolated `git archive HEAD` extraction, Gitleaks 8.30.1 `dir`, default rules, full redaction, archive depth 2, no repository ignore file or allow comments: about 29.95 MB. |
| Reachable changes | Gitleaks 8.30.1 `git --log-opts='--all'`, same default-rule configuration/redaction, no repository suppression: 2,098 commits with patches, about 125.65 MB. This differs from the graph count including merges. |
| Commit messages | `git log --all --format='%H%n%B'` into Gitleaks `stdin`: about 1.27 MB, no candidates. |
| All reachable blobs | `git rev-list --objects --all`, `git cat-file --batch-check`, `git cat-file --batch`; throwaway Python checked every blob, including WASM bytes, for home prefixes, macOS temporary prefixes, Homebrew paths, private IPv4 ranges, credential-bearing URLs and private-key markers. Reports kept paths, IDs and counts, never matched values. |
| Dependencies | `tools/cargo.sh metadata --locked --offline --format-version 1`; tracked npm lockfile license/source fields, workspace manifests and copied CSS notices. No advisory database or complete distribution bill of materials. |
| GitHub | Read-only repository, main-branch, Actions-artifact and release APIs. No settings changed. |
| Code/design | Board JSON API and write guards; server peer/Host and native-write origin gates; process-plugin trust; land/release contracts and documentation ownership. A targeted source review, not a security assessment of every module. |
| Contributed package evidence | The publication worker's exact-archive audit and preserved Cargo verification record for `837a0d36c8d8287860b44c7913c35a698779ac80`, summarized below. This session read the redacted report/manifests, not another branch's files, and did not regenerate or independently inspect those archives. |

Gitleaks came from its [official release](https://github.com/gitleaks/gitleaks/releases/tag/v8.30.1).
The Darwin ARM64 archive matched published SHA-256
`b40ab0ae55c505963e365f271a8d3846efbc170aa17f2607f13df610a9aeb6a5`. Configuration extended
default rules without a custom allowlist. The sanitized findings below are the lasting evidence.

Not covered: reflog-only/dangling objects, unadvertised PR refs, deleted remote objects, other
clones, forks, LFS objects, Actions logs/caches, and remote issue/PR bodies. Images were inventoried,
not exhaustively read or OCR-scanned. Encoded/archive content can evade signatures; regexes do
not prove absence of arbitrary credentials or private planning. Scan success cannot substitute
for the requested review.

## Publication evidence by source

**No immutable source is cleared for first publication by this audit.** Evidence belongs to its
named commit and archive hashes; it does not follow a moving branch, a release-candidate name,
or a later document commit.

| Source commit | Evidence and remaining disposition |
| --- | --- |
| `837a0d36c8d8287860b44c7913c35a698779ac80` | Initial tree/metadata audit above. Publication task reports all thirteen packages passing locked dry-run verification, eight package tests, lint and four packaged CLI version smokes. Its later approved run `7ba17f0f9a21e3106a5d5eeaced0a38d` also verifies all thirteen using the normal Cargo home and unchanged pinned toolchain, with verification enabled. Exact thirteen-archive audit covers 838 regular members and identifies the React notice gap and Dioxus notice disposition in L3. Packaging success is not release validation or publication clearance. |
| `facf1a9801f49c75fd68776a76b8bfe68d9be9d2` | Release-candidate test-fix commit identified by coordination. This audit supplies no exact-source package archive or readiness clearance for it. The `837a0d36` source/archives and their verification do not establish this commit's readiness; candidate review and release validation remain separate evidence. |

The corrected exact-archive report/manifests on the publication task have SHA-256
`f0d750da5c8646d02757e27451856c4f277edb61b40f2dfb5ca08c339913d296`; the separate fixture,
verification logs and recorder summaries have SHA-256
`1a5ba276d78dbf2dae493229c1407fee56d3fd476375d153ba893e48f2a8e538`.
These identify the contributed evidence without treating a successful command as an audit.
The normal-home success supersedes the claim that default Cargo state necessarily causes the
earlier no-hash failure: restricted execution versus transient/cache state was not isolated.
No cache corruption or required toolchain/source fix is established.

The corrected catalogue separates 289 external registry versions with license metadata from
eleven packaged Symphony siblings, whose own archive licenses prove MIT. They are not eleven
unknown third-party licenses. None of the five MPL dependencies in the wider-workspace metadata
appears in these exact archived lockfiles. That wider scope is separate from the actual server
notice gap, and external Rust source trees are not bundled in these tarballs.

Any eventual clearance must name the chosen full source SHA, exact rebuilt archive hashes,
disposition of each applicable finding, independent review and required release evidence.
A later source change needs a scoped comparison and validation of the resulting archives,
including notice remediation; it cannot inherit an unrelated commit's scan or dry-run. Historical
path privacy treatment is its own owner decision, not proof of a credential inside a package.
Upload authentication remains unverified, recurring publication remains off, and no credential
read, authentication probe, upload or publication-setting change is part of this document work.

## Findings

| ID | Evidence | Disposition |
| --- | --- | --- |
| S1 | History scan returned three `generic-api-key` candidates: `apps/symphony-server/tests/after_host_origin/after_host_origin.rs:735` in `29c2b1c7beee86e47a0f149258bc6ffc075fe3e0` and `22afbd67bcfbb44f5f044c2ad17403ddfb49396c`, and `apps/symphony-server/src/api/feed_socket.rs:417` in the latter. Current scan found the same two test locations at lines 737 and 424. Source inspection identifies the public RFC 6455 sample nonce used for handshakes. | False positives, not credentials. Both raw scan exits were 1 for candidates; they are not presented as clean scanner runs. No suppression or test changed. |
| S2 | No confirmed credential emerged from those candidates, the commit-message scan or the private-key-marker search. No tracked filename matched the inventory's environment/key-file patterns. | No rotation is indicated by this evidence. Any later confirmed credential is revoked/rotated first; publish only location/type, never its value. |
| P1 | Twelve historical client WASM blobs contain non-fixture home paths, all reachable from main. Representative blob `478142bfdcef88efa12abfee35729ffabd240eb5`, at `apps/symphony-server/client/public/assets/symphony-ui_bg-dxh8833f421cef363a.wasm`. | Path disclosure, not proof of a credential. The [recorded A disposition](github-history-treatment.md#github-historical-path-treatment) retains public history and fixes current/future outputs; no rewrite is required for this finding. [Refs, pins and scratch comparison](github-history-treatment.md) remain evidence. |
| P2 | Historical shipped guidance contains a non-fixture home path: blob `04c10c08a560a348e132768de542ca104cbc52f7`, at `crates/symphony/shipped/skills/apply-review/rewrite-brief.md`. | Covered by the same retain-history A disposition. Preserve the historical blob without copying its personal value into new tasks/guides; it is not a credential finding. |
| P3 | Current home-prefix matches in 14 files, private IPv4 matches in four, macOS temporary-prefix matches in one and credential-shaped URLs in one are synthetic tests. Inspected cases exercise schedule quoting, navigation/traversal refusals and network/address rejection. | Retain meaningful fixtures; an empty grep is not a goal. |
| M1 | `symphony.toml:53` names a Homebrew-specific PostgreSQL executable folder; five other files mention that prefix in tests. | Runtime setting is machine-specific, not a secret. [[task:tools/postgresql-setup-portable]] owns discovery/configuration, strict version/override/error fixtures and setup docs. Original snapshot evidence is not rewritten when that fix lands; record its new source/checks separately. |
| G1 | GitHub reports secret scanning, non-provider patterns, validity checks, push protection and Dependabot security updates disabled. Main reports `protected: false`. | Propose appropriate secret/push protections and advisory/update policy separately. Account for the local land/regenerator contract before protection changes. No setting changed here. |
| G2 | Actions artifact API reports zero artifacts; releases API returns no GitHub releases. Git tags exist and were part of reachable history. | No assets in those responses to inspect; says nothing about logs/caches, deleted assets or forks. |
| L1 | Root LICENSE is MIT with Stefano Benatti's copyright. Workspace metadata says MIT and names `heavenstudio/symphony`; members inherit or declare licensing. Copied Dioxus styles retain upstream revision/provenance and adjacent MIT/Apache licenses. | MIT project intent is recorded. Preserve third-party notices; the root license does not relicense dependencies. |
| L2 | Cargo metadata: 512 packages, 17 workspace and 495 external registry packages, no missing external license field or external Git/path source. Five external crates declare MPL-2.0: cssparser, cssparser-macros, dtoa-short, option-ext, selectors. Other expressions include Apache, Unicode and BSD terms. Main npm lock: 117 non-root entries, all with licenses, all resolved URLs on npm registry. Experiment npm locks also declare mixed licenses. | Make a distribution-specific dependency/notice inventory including native desktop code, copied assets and experiments. Metadata is neither compliance evidence nor a vulnerability scan. Determine obligations of what actually ships; not every dependency is MIT. |
| L3 | Contributed exact-archive audit at `837a0d36c8d8287860b44c7913c35a698779ac80`: `symphony-server-0.1.0.crate`, SHA-256 `38ac2bd484a2439729f3c1c73e0e580c1c579e9febf095869802676505930f0f`, embeds React 19.3.1 in `client/public/assets/shared-c61b2176a5517aea.js` but contains no Meta/Facebook copyright or vendor notice/license file; JS/CSS license comments are absent. Adapted Dioxus controls/focus-trap source provenance and license templates are outside the archive. The worker's bounded private-key/token/path checks found no confirmed credential; path matches were test fixtures. | Concrete distribution-notice remediation at this source, independent of secret-scan results. The orchestrator assigns the client/distribution fix and independent review; the publication task validates the replacement immutable source and exact archives. Select/document the applicable Dioxus license route and retain its notices. Actual-target Rust dependency obligations and the broader audit remain separate; the archive report is not general clearance. |
| D1 | DECISIONS.md: 65 cards, 49 resolved, nine rejected, three postponed, three waiting on an agent, one auto-approved for spot-check; 1,515 lines. Some durable behavior is already in owning docs; register still carries detailed designs and implementation-era context. | Migrate by topic, preserving card anchors, owner verdicts, pending asks and links. See the ownership plan below. |
| D2 | README setup says SurrealDB while root guide also describes PostgreSQL prerequisites. Product index calls its spec form a proposal although the card is resolved. `docs/engineering/analysis/own-names.md` keeps a superseded plan in a reference location. | Reconcile setup with supported behavior, apply accepted decisions into current prose and move superseded evidence to history. |
| D3 | No tracked SECURITY.md, CONTRIBUTING.md or third-party notice inventory found. There are 151 raster pictures. | Add a verified private vulnerability-reporting path and contributor doorway; inspect every published picture for private paths/accounts/task text and use fixtures for public examples. |
| D4 | Contributed current-media review at `3b460adb62827f674669e8e99f3090561107264f` inspected 174 paths, including the six images added after the initial audit. Four confirmed machine-label exposures occur in `docs/design/analysis/timeline-burst/a-wide.webp`, `b-wide.webp`, `c-wide.webp` and `c-wide-dark.webp`; exact hashes and eight original-pixel regions are on the media task. Six neutralized task-derived mockups and three already blurred captures remain distinguished from confirmed readable private material. | [[task:docs/public-media-privacy-review]] owns clearly labelled redacted derivatives, provenance/link checks and final per-file disposition. Ordinary current-media fixes are separately authorized; retaining Git history does not waive them or require another generic rewrite question. This report does no duplicate pixel review or image edit. |

Historical overlapping pattern counts: 72 blobs with home prefixes, 72 with Homebrew prefixes,
35 with private IPv4, three with temporary prefixes and three with credential-shaped URLs.
These are not secret/user counts. Classification of home matches yielded P1/P2; historical
network/URL examples were address/security tests. No matched personal path or secret value is
reproduced here.

### Historical path manifest

Each blob below is reachable from the audited main history. These identify the affected content
without reproducing its personal values; additional copies introduced later need another scan.

| Blob | Historical path |
| --- | --- |
| `478142bfdcef88efa12abfee35729ffabd240eb5` | `apps/symphony-server/client/public/assets/symphony-ui_bg-dxh8833f421cef363a.wasm` |
| `3e970e30ba6a02da0bd749f23224c9f26c184854` | `apps/symphony-server/client/public/assets/symphony-ui_bg-dxhaa2d83d3361a2c1f.wasm` |
| `6f53998373659a5d98f2d451cbf5fa9c5f064092` | `apps/symphony-server/client/public/assets/symphony-ui_bg-dxhb3c19e792b57416d.wasm` |
| `6a407493f980ca70c0dfab3bf485cc137d5fe069` | `apps/symphony-server/client/public/assets/symphony-ui_bg-dxhcb902f3b438d78c.wasm` |
| `2001487efde4675da6d442781afe83a353c134ff` | `apps/symphony-server/client/public/assets/symphony-ui_bg-dxhd6cda453cb4392a.wasm` |
| `831b051fcb698f7e8d4d88329d54392f1dbfd54f` | `apps/symphony-server/client/public/assets/symphony-ui_bg-dxhe64f85d88c4a3044.wasm` |
| `a84ad0c67c176054de4f1a56478c6109bce73e15` | `apps/symphony-server/client/public/assets/symphony-ui_bg-dxh28bc4ea2a0d75c6d.wasm` |
| `d342060320023082a91349ec40f8040e3f02e1d2` | `apps/symphony-server/client/public/assets/symphony-ui_bg-dxh2ccd3bcbc2bbd7ae.wasm` |
| `b4ec4e6f57a471638635d7c544d1d004a9d7a967` | `apps/symphony-server/client/public/assets/symphony-ui_bg-dxh36b81ae3af16b0fb.wasm` |
| `2101a1b3e25d9ac97a14bd309d758abea42f7b7c` | `apps/symphony-server/client/public/assets/symphony-ui_bg-dxh97566c91afdb5b72.wasm` |
| `b97855b6cd0b6b51395fdf785c1b200bdecb3f79` | `apps/symphony-server/client/public/assets/symphony-ui_bg-dxha41dd3a873779a3.wasm` |
| `36501b5d3eeb504309c578cf7a29c239f3a0f1df` | `apps/symphony-server/client/public/assets/symphony-ui_bg-dxhe9a5fe39e4575ab6.wasm` |
| `04c10c08a560a348e132768de542ca104cbc52f7` | `crates/symphony/shipped/skills/apply-review/rewrite-brief.md` |

The server's gate allows local clients and recognized tailnet peers, validates Host, and checks
native-write Origin before reading the body. Process plugins run as trusted local code with
the host's user. Public setup must explain these boundaries; the app is not designed here as a
public authenticated hosting service. A GitHub receiver must not reuse owner-write routes or
remove their gates. No shared service was started or restarted in this audit.

## Documentation ownership plan

Keep README as the install/try/contribute doorway and docs/README.md as an index grouped into
product use, operator setup, development/contracts, and design/history. Package guides own their
mechanics. Public onboarding should not require reading the entire agent operating guide.

| Content | Owning reference | Migration boundary |
| --- | --- | --- |
| Releases, consumer versions, update/schema compatibility, Git pins and built-client delivery | [RELEASES](../../workflow/RELEASES.md), consumer checks in [CHECKS](../../workflow/CHECKS.md) | Current contract once; dated measurements/alternatives in history. Preserve pending publication/distribution followups. |
| Lifecycle/workflows, search, stamps, land, sweep | [TASKS](../../workflow/TASKS.md), knobs in [PROJECT_FILE](../PROJECT_FILE.md) | Separate accepted intent from implemented behavior: workflow approval does not make proposed keys loadable. |
| Header/look, task/agent pages, loading | Topic specs under [product](../../product/README.md) | Complete missing page specs with public fixture pictures; link cards to the owning behavior. |
| Plugin delivery and kit versions | [PLUGIN_PROTOCOL](../../engineering/PLUGIN_PROTOCOL.md) | Current version/trust contracts together; experiments remain bounded analysis/history. |
| Server framework, handover and request boundaries | [Server guide](../../../apps/symphony-server/README.md) | Current setup/prerequisites in reference; engineering rationale links to analysis. |
| Client framework/cache/build/distribution | [Client guide](../../../apps/symphony-web/AGENTS.md) and topic reference | Operating facts in the guide, benchmarks in history, third-party provenance retained. |
| Desktop | [Desktop guide](../../../apps/symphony-desktop/AGENTS.md) | Supported platforms/prerequisites distinguished from local examples. |
| Reports, runs, costs and activity | [MACHINE_WORK](../../workflow/MACHINE_WORK.md), [RUNS](../../workflow/RUNS.md), [AGENT_ACTIVITY](../../workflow/AGENT_ACTIVITY.md) | Each owns its semantics/data; page behavior belongs to its product spec. |
| Owner asks, cards/verdicts and guide content | [REVIEW](../../workflow/REVIEW.md), standing guides/rules | Preserve live queue and exact verdict history; durable design goes to owning references. |
| Artifacts/pictures in clones | Dedicated artifact reference from [attached-artifacts analysis](../../workflow/analysis/attached-artifacts.md) | Accepted storage behavior in reference; verify links and retain rejected alternatives as history. |
| Names and rejected build experiments | Standing guides and history | Superseded naming migration is not current configuration guidance. |

For each resolved card, compare decision, code and owning document. Write missing durable intent
there, file any unimplemented requirement, and replace duplicated design with a concise outcome
and link. Preserve the stable anchor and dated verdict/thread where review can still reach them.
Rejected/postponed cards and live asks do not become accepted reference. The register becomes an
index and owner queue, rather than the sole design home. Moving verdict storage itself would
change the review format and requires separate design, not a blind Markdown split.

This branch adds only the two coordinated GitHub owner cards to DECISIONS.md, preserving all
existing content/history. Owner-answer work remains with its existing editor. Coordinate each
later topic migration separately, checking links/cards at the final tip. Mark accepted but
unimplemented intent explicitly; code determines what works now.

## Remediation order

| Step and responsible role | Work | Exit evidence |
| --- | --- | --- |
| 1. Owner with audit executor | Triage historical path exposure/containment; revoke any later confirmed credential first. | Sanitized findings/dispositions. No rotation currently indicated by scanner candidates. |
| 2. Historical-path disposition | A is recorded on the [history card](github-history-treatment.md#github-historical-path-treatment): retain existing public history; clean current assumptions/future outputs. | Preserve tags, consumer pins and owner history. The [scratch proof](github-history-treatment.md) is comparison evidence only. No public rewrite is needed to dispose of P1/P2, and generic paths are not credential evidence. |
| 3. Portability executor/settings owner | [[task:tools/postgresql-setup-portable]] owns M1 and the setup portion of D2; separately review future build-path leakage and G1 settings. | Alternate-prefix, explicit-override, strict version/error tests and scratch setup/docs evidence at the fix's final tip. Consumer checkouts stay read-only. |
| 4. Dependency/security reviewers | [[task:tools/client-distribution-notices]] owns L3; [[task:tools/public-code-boundary-review]] owns the publication-sensitive code review. Actual-target dependency obligations remain scoped separately. | Replacement exact archives/notices, independent review and concrete code-boundary dispositions. Metadata inventory is not a full code audit. |
| 5. Document/media owners | [[task:docs/public-media-privacy-review]] owns the current media inventory/redactions in D4. Coordinate broader document migration/security/contribution guidance separately; never fabricate historical captures. | Complete per-file disposition and derivative/link/provenance checks at the media fix tip. Preserve owner history; initial 837a counts remain snapshot-specific. |
| 6. Independent readiness reviewer | Check final source/history scope against the request, with owner handling unresolved history/visibility actions. | Audited commit/ref set, all substantive findings closed or explicitly decided, limitations recorded. Scan success alone is insufficient. |

GitHub's [sensitive-data removal guidance](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/removing-sensitive-data-from-a-repository)
puts credential revocation before cleanup and explains that clones, forks and cached views can
retain data after rewriting. Its [visibility guidance](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/managing-repository-settings/setting-repository-visibility)
also says existing public forks remain public after privatization. A visibility toggle alone is
not complete containment. Neither operation is authorized by this initial staffed phase.

The orchestrator coordinates the named remediation and any remaining bounded review work.
Historical paths retain their recorded A disposition. Enabled PR delivery follows the separately
scoped owner-created landing task; neither choice is reopened by this audit. Public-readiness
still requires concrete fix/review dispositions for the selected immutable source and archives.
