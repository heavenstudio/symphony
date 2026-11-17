# Public-readiness audit

This initial read-only audit and remediation plan was recorded on 2026-11-17 for the GitHub
integration task. It is not publication clearance. The [integration proposal](github-integration.md)
is separate from repository readiness.

**The repository was already public when inspected.** `gh repo view heavenstudio/symphony`
reported PUBLIC, `isPrivate: false`, main as default branch, MIT recognized, issues enabled and
wiki disabled. This session changed no setting, credential, history, tag or visibility. The
remaining work is review and remediation of existing exposure.

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

Gitleaks came from its [official release](https://github.com/gitleaks/gitleaks/releases/tag/v8.30.1).
The Darwin ARM64 archive matched published SHA-256
`b40ab0ae55c505963e365f271a8d3846efbc170aa17f2607f13df610a9aeb6a5`. Configuration extended
default rules without a custom allowlist. The sanitized findings below are the lasting evidence.

Not covered: reflog-only/dangling objects, unadvertised PR refs, deleted remote objects, other
clones, forks, LFS objects, Actions logs/caches, and remote issue/PR bodies. Images were inventoried,
not exhaustively read or OCR-scanned. Encoded/archive content can evade signatures; regexes do
not prove absence of arbitrary credentials or private planning. Scan success cannot substitute
for the requested review.

## Findings

| ID | Evidence | Disposition |
| --- | --- | --- |
| S1 | History scan returned three `generic-api-key` candidates: `apps/symphony-server/tests/after_host_origin/after_host_origin.rs:735` in `29c2b1c7beee86e47a0f149258bc6ffc075fe3e0` and `22afbd67bcfbb44f5f044c2ad17403ddfb49396c`, and `apps/symphony-server/src/api/feed_socket.rs:417` in the latter. Current scan found the same two test locations at lines 737 and 424. Source inspection identifies the public RFC 6455 sample nonce used for handshakes. | False positives, not credentials. Both raw scan exits were 1 for candidates; they are not presented as clean scanner runs. No suppression or test changed. |
| S2 | No confirmed credential emerged from those candidates, the commit-message scan or the private-key-marker search. No tracked filename matched the inventory's environment/key-file patterns. | No rotation is indicated by this evidence. Any later confirmed credential is revoked/rotated first; publish only location/type, never its value. |
| P1 | Twelve historical client WASM blobs contain non-fixture home paths, all reachable from main. Representative blob `478142bfdcef88efa12abfee35729ffabd240eb5`, at `apps/symphony-server/client/public/assets/symphony-ui_bg-dxh8833f421cef363a.wasm`. | Current deletion does not remove historical exposure. Prepare the complete affected blob/commit manifest with private values withheld, and check future binary build-path remapping. |
| P2 | Historical shipped guidance contains a non-fixture home path: blob `04c10c08a560a348e132768de542ca104cbc52f7`, at `crates/symphony/shipped/skills/apply-review/rewrite-brief.md`. | Include in the history/privacy plan; do not copy the personal path into a public task or new guide. |
| P3 | Current home-prefix matches in 14 files, private IPv4 matches in four, macOS temporary-prefix matches in one and credential-shaped URLs in one are synthetic tests. Inspected cases exercise schedule quoting, navigation/traversal refusals and network/address rejection. | Retain meaningful fixtures; an empty grep is not a goal. |
| M1 | `symphony.toml:53` names a Homebrew-specific PostgreSQL executable folder; five other files mention that prefix in tests. | Runtime setting is machine-specific, not a secret. Replace the checkout assumption with documented discovery/local override, retaining the pinned PostgreSQL requirement and actionable errors. Test an alternate prefix. |
| G1 | GitHub reports secret scanning, non-provider patterns, validity checks, push protection and Dependabot security updates disabled. Main reports `protected: false`. | Propose appropriate secret/push protections and advisory/update policy separately. Account for the local land/regenerator contract before protection changes. No setting changed here. |
| G2 | Actions artifact API reports zero artifacts; releases API returns no GitHub releases. Git tags exist and were part of reachable history. | No assets in those responses to inspect; says nothing about logs/caches, deleted assets or forks. |
| L1 | Root LICENSE is MIT with Stefano Benatti's copyright. Workspace metadata says MIT and names `heavenstudio/symphony`; members inherit or declare licensing. Copied Dioxus styles retain upstream revision/provenance and adjacent MIT/Apache licenses. | MIT project intent is recorded. Preserve third-party notices; the root license does not relicense dependencies. |
| L2 | Cargo metadata: 512 packages, 17 workspace and 495 external registry packages, no missing external license field or external Git/path source. Five external crates declare MPL-2.0: cssparser, cssparser-macros, dtoa-short, option-ext, selectors. Other expressions include Apache, Unicode and BSD terms. Main npm lock: 117 non-root entries, all with licenses, all resolved URLs on npm registry. Experiment npm locks also declare mixed licenses. | Make a distribution-specific dependency/notice inventory including native desktop code, copied assets and experiments. Metadata is neither compliance evidence nor a vulnerability scan. Determine obligations of what actually ships; not every dependency is MIT. |
| D1 | DECISIONS.md: 65 cards, 49 resolved, nine rejected, three postponed, three waiting on an agent, one auto-approved for spot-check; 1,515 lines. Some durable behavior is already in owning docs; register still carries detailed designs and implementation-era context. | Migrate by topic, preserving card anchors, owner verdicts, pending asks and links. See the ownership plan below. |
| D2 | README setup says SurrealDB while root guide also describes PostgreSQL prerequisites. Product index calls its spec form a proposal although the card is resolved. `docs/analysis/own-names.md` keeps a superseded plan in a reference location. | Reconcile setup with supported behavior, apply accepted decisions into current prose and move superseded evidence to history. |
| D3 | No tracked SECURITY.md, CONTRIBUTING.md or third-party notice inventory found. There are 151 raster pictures. | Add a verified private vulnerability-reporting path and contributor doorway; inspect every published picture for private paths/accounts/task text and use fixtures for public examples. |

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
| Releases, consumer versions, update/schema compatibility, Git pins and built-client delivery | [RELEASES](../RELEASES.md), consumer checks in [CHECKS](../CHECKS.md) | Current contract once; dated measurements/alternatives in history. Preserve pending publication/distribution followups. |
| Lifecycle/workflows, search, stamps, land, sweep | [TASKS](../TASKS.md), knobs in [PROJECT_FILE](../PROJECT_FILE.md) | Separate accepted intent from implemented behavior: workflow approval does not make proposed keys loadable. |
| Header/look, task/agent pages, loading | Topic specs under [product](../product/README.md) | Complete missing page specs with public fixture pictures; link cards to the owning behavior. |
| Plugin delivery and kit versions | [PLUGIN_PROTOCOL](../PLUGIN_PROTOCOL.md) | Current version/trust contracts together; experiments remain bounded analysis/history. |
| Server framework, handover and request boundaries | [Server guide](../../apps/symphony-server/README.md) | Current setup/prerequisites in reference; engineering rationale links to analysis. |
| Client framework/cache/build/distribution | [Client guide](../../apps/symphony-web/AGENTS.md) and topic reference | Operating facts in the guide, benchmarks in history, third-party provenance retained. |
| Desktop | [Desktop guide](../../apps/symphony-desktop/AGENTS.md) | Supported platforms/prerequisites distinguished from local examples. |
| Reports, runs, costs and activity | [MACHINE_WORK](../MACHINE_WORK.md), [RUNS](../RUNS.md), [AGENT_ACTIVITY](../AGENT_ACTIVITY.md) | Each owns its semantics/data; page behavior belongs to its product spec. |
| Owner asks, cards/verdicts and guide content | [REVIEW](../REVIEW.md), standing guides/rules | Preserve live queue and exact verdict history; durable design goes to owning references. |
| Artifacts/pictures in clones | Dedicated artifact reference from [attached-artifacts analysis](attached-artifacts.md) | Accepted storage behavior in reference; verify links and retain rejected alternatives as history. |
| Names and rejected build experiments | Standing guides and history | Superseded naming migration is not current configuration guidance. |

For each resolved card, compare decision, code and owning document. Write missing durable intent
there, file any unimplemented requirement, and replace duplicated design with a concise outcome
and link. Preserve the stable anchor and dated verdict/thread where review can still reach them.
Rejected/postponed cards and live asks do not become accepted reference. The register becomes an
index and owner queue, rather than the sole design home. Moving verdict storage itself would
change the review format and requires separate design, not a blind Markdown split.

This branch leaves DECISIONS.md and owner-answer work to their existing editors. Coordinate one
bounded topic at a time, checking links/cards at each final tip. Mark accepted but unimplemented
intent explicitly; code determines what works now.

## Remediation order

| Step and responsible role | Work | Exit evidence |
| --- | --- | --- |
| 1. Owner with audit executor | Triage historical path exposure/containment; revoke any later confirmed credential first. | Sanitized findings/dispositions. No rotation currently indicated by scanner candidates. |
| 2. Owner's history decision | Choose coordinated rewrite or another explicit publication-history plan after enumerating affected immutable Git pins, release tags, branches, PR refs and forks. | Scratch-clone proof removes identified sensitive blobs/paths, preserves required source/license history and explains consumer migration. No rewrite/force push before a concrete plan is authorized. |
| 3. Portability executor/settings owner | Resolve M1 via tested configuration/discovery; review build-path remapping and source-map/binary contents; address G1 through repository settings work. | Alternate-prefix tests, sanitized output scan, selected protections read back. Consumer checkouts stay read-only. |
| 4. Dependency/security reviewers | Inventory actual source/binary/npm distributions, copied assets and notices. Review file reads/uploads, process/shell dispatch, plugins and network writes. | Resolved distribution obligations, triaged advisory findings, focused regression evidence for actual fixes. Metadata inventory is not a full code audit. |
| 5. Document/media owners | Apply topic migration, reconcile setup, add security/contribution guidance, inspect all 151 raster pictures and SVGs; regenerate private examples from fixtures. | Link/card checks, clean-environment setup evidence, media manifest with every item disposition; stable links and pending answers preserved. |
| 6. Independent readiness reviewer | Check final source/history scope against the request, with owner handling unresolved history/visibility actions. | Audited commit/ref set, all substantive findings closed or explicitly decided, limitations recorded. Scan success alone is insufficient. |

GitHub's [sensitive-data removal guidance](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/removing-sensitive-data-from-a-repository)
puts credential revocation before cleanup and explains that clones, forks and cached views can
retain data after rewriting. Its [visibility guidance](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/managing-repository-settings/setting-repository-visibility)
also says existing public forks remain public after privatization. A visibility toggle alone is
not complete containment. Neither operation is authorized by this initial staffed phase.

The orchestrator should file coordinated, bounded remediation from this report. History cleanup
and outbound PR delivery are distinct owner decisions; neither belongs hidden inside document
cleanup or an enable switch. Public-readiness remains incomplete until remediation and review.
