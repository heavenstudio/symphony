# Publication code boundary review

This is a bounded review of server request, file and process boundaries, not repository-wide
publication clearance. The initial inspected source is immutable commit
`3b460adb62827f674669e8e99f3090561107264f`. Source links below name that commit; later changes
and final verification are recorded under [Disposition and evidence](#disposition-and-evidence).

## Findings

### B1: JSON saves collect an unlimited request body

**Confirmed availability defect, medium within the trusted-peer deployment.**
[The native write handler](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/apps/symphony-server/src/write.rs#L79)
passes `usize::MAX` to `to_bytes`. An admitted client can make it allocate its entire body before
JSON parsing or save dispatch. No successful save or valid JSON is necessary. The Host/Origin
checks still apply; this is not a bypass of those checks. The upload and focus routes have
separate bounds and do not cause this defect.

The repair bounds JSON collection at 2 MiB and answers 413 on overflow, including
when no Content-Length is supplied. This matches the size of Axum's default extractor limit;
direct body collection does not use that extractor. It must preserve the existing empty-body
and malformed-JSON behavior and gate-before-body order. Reservation and overlap coordination
are on `tools/public-code-boundary-review`; no overlapping save/journal work is included here.

The cap was checked against the existing callers: the 13 `save::ROUTES` carry individual
decisions, comments, task fields/actions or chat text, not complete repository files. The client
[editor payloads](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/apps/symphony-web/src/editor.tsx#L70)
and [quick chat payloads](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/apps/symphony-web/src/quick.tsx#L173)
send prose plus identifiers. [Attachments](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/apps/symphony-web/src/attach.tsx#L101)
use a separate raw-file POST and insert references into that prose, rather than base64 bytes.
Two MiB leaves substantial room for those individual posts and follows the framework's familiar
default. This is a new explicit limit: a JSON post larger than that now gets 413, even if valid;
the contract does not promise unlimited prose. No upload limit is reduced and no wire field is
changed. Aggregate concurrent memory use and slow admitted clients remain deployment limits.

### B2: admitted peers have project-wide read and owner-write authority

**Trust assumption, not an authentication defect under the intended contract.**
[Peer admission](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/apps/symphony-server/src/gate.rs#L230)
trusts loopback and confirmed nodes in this machine's tailnet. There is no per-user or per-project
authorization. A permitted command-line client may omit Origin. The UI save routes act as the
owner, and raw project downloads can read files outside the configured document entrypoints,
subject to checkout containment and metadata exclusion. Entrypoint include/exclude patterns
govern document readers, not a confidentiality boundary around all project files.

This is unsuitable for an untrusted tailnet, an unauthenticated proxy exposing loopback, or
public hosting. No public authentication system is proposed here. Project contents, plugins,
local executables/PATH, Git configuration and machine registry are trusted local inputs.

### B3: local filesystem mutation and plugins are outside the confinement promise

**Explicit limitation.** Canonical containment checks precede ordinary file opens; they are not
descriptor-relative sandboxing against a hostile local process swapping paths concurrently.
[Upload storage](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/crates/symphony/src/uploads.rs#L420)
also trusts its configured local store and existing content-addressed files. Plugins run as the
server user, inherit ordinary process environment apart from session variables, and may execute
arbitrary configured programs. The process protocol's bounds contain malformed messages and
failed calls; they do not sandbox the plugin's filesystem, network or descendants.

## Reviewed boundaries

These are the production paths inspected, with supporting tests read separately. Paths linked
at the initial source commit remain evidence of that version, not a claim about later code.

| Boundary and reviewed paths | Disposition and existing test evidence |
| --- | --- |
| Socket peer, Host, native dispatch: [front.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/apps/symphony-server/src/front.rs), [gate.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/apps/symphony-server/src/gate.rs), [projects.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/apps/symphony-server/src/projects.rs) | Connection address, not forwarded headers, decides admission. Unknown peers fail closed. Previously confirmed peer/name answers survive lookup failure by design; explicit negative answers replace them. Host checks resist foreign-name rebinding. `gate::tests`, `after_access` and `after_host_origin` cover mapped IPv4, failed/garbage lookup output, Host spellings, missing/malformed ports and private write side effects. Stale allowed answers during lookup outages remain a deliberate availability tradeoff. |
| Browser writes and WebSocket reads: [write.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/apps/symphony-server/src/write.rs), [api.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/apps/symphony-server/src/api.rs), [feed_socket.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/apps/symphony-server/src/api/feed_socket.rs) | Present Origin must match `http://` plus Host on writes and WebSocket upgrades. `null` is refused for native API reads; ordinary cross-origin reads rely on the browser's same-origin policy, with no permissive CORS layer found. Gate checks precede body collection/dispatch. `after_host_origin` checks 21 refused Hosts, 12 allowed Hosts and 12 refused Origins, including WebSocket handshakes and unchanged scratch state. B1 is the body-bound exception. |
| Documents and entrypoints: [project.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/crates/symphony/src/project.rs), [entrypoint.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/crates/symphony/src/entrypoint.rs), [project/file.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/crates/symphony/src/project/file.rs), [docview.rs target](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/crates/symphony/src/docview.rs#L36), [save.rs target/scope](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/crates/symphony/src/save.rs#L191) | Non-normal path components are refused. The longest entrypoint maps routes; `reachable` follows the same walk as listings, excluding hidden/excluded folders, folder symlinks and outside-project file symlinks. Save targets use the same resolver. Worktree scope is selected from known worktrees, not a caller-supplied root. `project/entrypoints.rs` covers remapping, excluded folders, external links and uploads exclusion; `docview` outside tests cover direct reads. |
| Raw files, HTML, uploads and thumbnails: [media_files.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/apps/symphony-server/src/media_files.rs), [uploads.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/crates/symphony/src/uploads.rs), [images.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/crates/symphony/src/images.rs), [assets.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/crates/symphony/src/assets.rs) | Raw reads canonicalize under the selected checkout/static root and exclude repository metadata. HTML documents additionally require entrypoint reachability. File responses carry opaque-origin CSP sandboxes; upload responses carry `nosniff`, fixed MIME and a script-free sandbox, with PDF/Office attachment disposition. Upload names are generated SHA-256 plus allowed extensions; temporary writes use create-new. Type sniffing is not malware inspection or a full image/Office parser. Upload cap is enforced while streaming. Existing upload route tests cover content/cap refusals and read confinement. Thumbnail conversion uses argv and a bounded child wait; dimensions and aggregate cache/resource quotas remain trusted-peer limitations. |
| Additional file reads: [task_diff.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/apps/symphony-server/src/api/task_diff.rs), [run_log.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/apps/symphony-server/src/api/run_log.rs) | Diff file paths reject non-normal components; Git blob reads select a recorded task's repository/revision. Run-log queries accept a run id and byte range and delegate to the board's confined log reader. They are not arbitrary absolute path APIs. Detailed board storage and database authorization are outside this review. |
| Process plugins: [process.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/crates/symphony/src/plugins/process.rs), [process_seams.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/crates/symphony/src/plugins/process_seams.rs), [process_content.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/crates/symphony/src/plugins/process_content.rs), [project.rs plugin_processes](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/crates/symphony/src/project.rs#L1261) | Trusted project settings select executable and argv. Relative executable paths reject `..`; a worktree uses its main checkout's plugin. Declaration name/major/seams, line length, queue length, calls, cancellation and restart rate are checked. Plugin commands append `command`, name and args as separate argv entries, without shell interpolation. Process tests cover bad declarations, oversized lines, queues, cancellation/restarts and command forwarding. See B3 for sandbox limitations. |
| Native command dispatch: [children.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/crates/symphony/src/children.rs), [commit.rs command adapters](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/crates/symphony/src/commit.rs), [session_go.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/apps/symphony-server/src/session_go.rs), [shell.rs](https://github.com/heavenstudio/symphony/blob/3b460adb62827f674669e8e99f3090561107264f/apps/symphony-server/src/shell.rs) | The HTML shell is a template, not a command shell. Inspected command adapters pass argv rather than concatenating HTTP payload into `sh -c`. Git uses isolated repository settings/private indexes. Configured generator, landing and plugin commands are trusted code. Focus is additionally limited to this machine and a known session/pane; its JSON body has its own 4096-byte cap. Cancel cleanup dispatches `tasks finish` with slug as an argv entry. Save receipt/outcome, callback storage and owner task validation remain with their assigned executor. |

## Dependency advisory triage

Checked authoritative RustSec records for dependencies actually reached by these boundaries,
against this commit's `Cargo.lock`. This is a reachability triage, not a complete advisory scan,
package inventory or proof of absence. Exact packaged dependency and notice inventory belongs to
the separate publication inventory task, coordinated with `cobalt-ibis-6` on the board.

| Reached dependency | Advisory disposition |
| --- | --- |
| Axum 0.8.9 / axum-core 0.5.6 | [RUSTSEC-2022-0055](https://rustsec.org/advisories/RUSTSEC-2022-0055.html) concerns extractor body limits, patched in axum-core 0.2.8/0.3.0-rc.2 and Axum 0.5.16/0.6.0-rc.2. The locked version is later, but the application's explicit unlimited collection independently recreates the resource-exhaustion class (B1). Updating dependencies alone would not fix it. |
| Hyper 1.12.0, HTTP/1 parser | [Content-Length parsing](https://rustsec.org/advisories/RUSTSEC-2021-0078.html) and [chunk-size overflow](https://rustsec.org/advisories/RUSTSEC-2021-0079.html) are patched from 0.14.10. The locked version is outside those affected ranges. Proxy deployment assumptions are still B2. |
| Tungstenite 0.29.0, feed WebSocket | [RUSTSEC-2023-0065](https://rustsec.org/advisories/RUSTSEC-2023-0065.html) is patched from 0.20.1. The locked version is outside its affected range. Native Host/Origin checks run before the upgrade. |
| Tokio 1.54.1, runtime/file/socket operations | [Interval race](https://rustsec.org/advisories/RUSTSEC-2026-0384.html) is patched in 1.54.1; [AsyncFd I/O safety](https://rustsec.org/advisories/RUSTSEC-2026-0386.html) is patched in 1.53.3 and selected older branches. The locked version meets both fixes. |
| Bytes 1.12.1, HTTP/body buffers | [RUSTSEC-2026-0007](https://rustsec.org/advisories/RUSTSEC-2026-0007.html) is patched from 1.11.1. The locked version meets the fix; B1 is a separate unbounded-allocation issue. |
| Comrak 0.57.0, document rendering | [URL case handling](https://rustsec.org/advisories/RUSTSEC-2021-0026.html) and [entity escaping](https://rustsec.org/advisories/RUSTSEC-2021-0063.html) are patched from 0.9.1 and 0.10.1 respectively. The locked version meets both fixes. Trusted project markup/renderers and client rendering are broader than those advisories; this review does not certify every client HTML sink. |
| gix 0.87.1 / gix-fs 0.22.1 / gix-worktree 0.56.0, repository discovery/history | [GHSA-6p9q-f2xg-6pr5](https://github.com/GitoxideLabs/gitoxide/security/advisories/GHSA-6p9q-f2xg-6pr5) **does include these locked versions**; gix-fs is fixed in 0.23.0. Its forced-checkout prerequisite is not reached here: the application selects basic/blob-diff/revision/sha1 features, no `gix-worktree-state` appears in this lockfile, and inspected gix call sites discover repositories/read objects/diffs rather than materialize a checkout. Worktree creation uses Git CLI. This is a reachability disposition, not a claim the dependency version is patched; reconsider it if checkout/mutation features are enabled. The older [RUSTSEC-2024-0350](https://rustsec.org/advisories/RUSTSEC-2024-0350.html) is fixed from gix-fs 0.11.0. |
| rustix 1.1.5, via gix-index 0.55.0 and tempfile 3.27.0 | [RUSTSEC-2026-0486](https://rustsec.org/advisories/RUSTSEC-2026-0486.html) affects this version; patched from 1.1.6. The six affected APIs read/list extended attributes with zero-length buffers. `cargo tree --locked -p symphony-server -i rustix` identifies these two direct callers; their installed source uses stat/temporary-file operations, with no affected xattr calls. Application Rust sources likewise call none. Thus the reviewed server file paths do not reach the affected operation; this does not mark the locked version patched or dispose of unrelated targets. |

The inventory executor independently confirmed that tiny_http, multipart and multer are absent
from the initial root lockfile. The earlier targeted audit's framing/multipart advisory subjects
are therefore not this server's handler stack. Serde JSON 1.0.151 decodes native and plugin
payloads; plugin lines and JSON recursion are bounded. This review does not infer safety for
every locked crate from an empty search result.

The inventory executor's [preserved source-bound audit and inventory](/artifacts/uploads/0f3ba5212be524067079e6dc29aa3427651cebf4835cbce2b4096c72ddb7b64b.txt)
binds its cargo-audit 0.22.2 result to initial source `3b460adb` and RustSec database
`233155b4025b854c68a4b199c0e32d7fd53291a2` (2026-11-17): zero vulnerability entries,
**five informational warnings**, no ignored advisories, yank lookup excluded. One is the rustix
warning triaged above. The other exact warning versions (fxhash 0.2.1, proc-macro-error 1.0.4,
glib 0.18.5, rand 0.7.3) belong outside that executor's packaged archive set; this review does not
extend its server conclusions to the desktop or other targets. The gix forced-checkout GHSA is
absent from that database snapshot, which is why the upstream advisory was checked separately.
The source/feature trace also reads `versions.rs` and `docview/comparison.rs`, `docview/regroup.rs`
under `crates/symphony/src`, and the Git CLI adapter `apps/symphony-server/src/api/task_diff/git.rs`.
The [upstream supplement](/artifacts/uploads/cc12d1e0a3e6b8c53e09ebd6071b3892ed6cf49517efd205959e4d6bd04d4050.txt)
preserves the separate gix advisory, fix, version and database-gap evidence.

## Disposition and evidence

Initial finding B1 and reservations were posted before production edits. At initial source
`3b460adb62827f674669e8e99f3090561107264f`, the recorded command
`just --one -- test-rs -p symphony-server --lib -- gate:: api::tests:: media_files:: write::upload_tests::`
passed: **33 passed, 0 failed**, 0.57 seconds in the test harness (first compilation 3m 16s).
The final inspected **production source commit is
`bb294b057e228fca218a2537e5c5adb88c468885`**. B1 is fixed in its
[write handler and bounded collector](https://github.com/heavenstudio/symphony/blob/bb294b057e228fca218a2537e5c5adb88c468885/apps/symphony-server/src/write.rs#L77),
with [six regression tests](https://github.com/heavenstudio/symphony/blob/bb294b057e228fca218a2537e5c5adb88c468885/apps/symphony-server/src/write/body_tests.rs).
The report-only commits after that source commit change no production code. No confirmed
high-impact finding remains unexplained within the reviewed boundaries: B1 is repaired; B2/B3
are explicit trust/deployment limits; the affected dependency versions have the reachability
dispositions above, not waivers or claims they were patched.

The task's final recorded evidence uses these focused targets; the review tip and each run's
commit, result and log are available through `tasks runs tools/public-code-boundary-review`.

| Recorded command (each prefixed `just --one --`) | Green result |
| --- | --- |
| `test-rs -p symphony-server --lib -- gate:: front::tests:: media_files:: write:: api::feed_socket::` | 54 passed, 0 failed; includes all six new body tests and existing upload/peer/Origin/file sandbox cases. |
| `test-rs -p symphony-server --test after_host_origin -- host_and_origin_guard_native_handlers_without_side_effects` | 1 passed, 0 failed; real HTTP refusals and successful document/ask/board writes in the private fixture. |
| `test-rs -p symphony-server --test after_http -- native_routes_media_and_persistence` | 1 passed, 0 failed; raw-file/metadata/symlink refusals, media and save persistence. |
| `test-rs -p symphony-app --lib -- plugins::process::tests plugins::process::content::tests entrypoint:: docview::outside_tests uploads::tests` | 77 passed, 0 failed; protocol failure/cancellation/limits, command argv and uploads. |
| `test-rs -p symphony-app --test project -- entrypoints::` | 13 passed, 0 failed; mappings, include/exclude, external symlinks and upload boundaries. |
| `test-rs -p symphony-app --test hooks -- owner_words` | 4 passed, 0 failed. |
| `lint-rs` | Format, workspace/all-target Clippy, TypeScript wire checks, client lint and unit tests passed. |
| `crates-check` | Passed; packages are not yet on crates.io, so no published source comparison is available. This is not a publish dry-run or publication. |
| `docs-check` | Both document audit and status lint passed. |

**Coverage analysis.** At source commit `bb294b05`, the recorded command
`tools/record.sh --pipeline test test 'JSON save body coverage' tools/cargo.sh llvm-cov --locked -p symphony-server --lib --json --output-path artifacts/tools/public-code-boundary-review/coverage.json -- write::body_tests::`
passed all six tests. The new `json_body` function and its generated async/error closures have
**32 of 32 LLVM code regions covered (100%)**, with no uncovered regions in that function.
This is source-region coverage, not branch/MC/DC coverage or whole-server coverage. Reading the
tests against those regions confirms empty, null/object, exact-cap, single-chunk overflow,
unknown-length multi-chunk overflow, transport failure and malformed JSON cases. Router cases
also prove no save store opens on overflow and no body is polled for a refused Host/Origin.
No assertion, budget or lint was weakened. The initial lint found an oversized error type;
the implementation now carries only status/message until constructing the HTTP response.
The first coverage report could not write into a missing output directory; its retry and the
final-source run succeeded. No whole suite was run. The measured focused test harnesses take
fractions of a second for units and seconds for the real HTTP fixture; first compilation, not
test work, dominated these runs, so no test-performance refactor is justified here.

**Private HTTP probe.** A foreground server built from repair commit `44dab2f0` served an owned
temporary repository at `http://127.0.0.1:58740/`; its `/api/front` confirmed that scratch root.
`curl --noproxy '*' --max-time 30 -sS -i -H 'Content-Type: application/json' --data-binary @oversized.json http://127.0.0.1:58740/api/comment`
with a valid JSON body of 2 MiB + 1 byte returned **413**, JSON error, `no-store` and `nosniff`.
The same command with `--data-binary '{'` returned **400**; with `--data-binary '{}'` and
`-H 'Host: foreign.example'` it returned **403**. The owned server was signalled by its recorded
PID and observed stopped. The later compact error representation changes no response contract;
final-source router and real HTTP tests cover it again.

The branch changes server source, which is a release input. **Client build inputs, wire shapes,
dependency versions and distribution are unchanged**; no generated distribution is committed.

No live write request, external upload, production credential access, shared-service restart,
remote setting change, history rewrite or publication is part of this review. Test fixtures are
owned temporary directories/boards and private ports. There is no scanner-based clearance claim.

## Remaining limitations

This review excludes full client DOM/XSS analysis, Mac navigation policy, every board CLI and
landing command, database implementation/schema, dependency source audits, OS/browser image and
document parser vulnerabilities, release packaging/notices and personal-data/media/history
cleanup. Parallel executors own those publication concerns and the save/storage changes named
above. New commits outside the final inspected paths need their own review evidence. No statement
here authorizes public exposure or establishes repository-wide publication readiness.
