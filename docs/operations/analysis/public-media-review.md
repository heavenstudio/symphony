# Tracked media privacy review

This review found a personal computer label in **four wide timeline images**. It found no
confirmed credential, personal account/contact address or home-directory path in the reviewed
pixels or embedded metadata. Six board-layout mockups retain anonymized, consuming-project-derived
task content; their publication is a separate residual disclosure choice, not a newly discovered
copy of the original private titles. Three historical captures have existing blur that this review
does not certify as irreversible redaction. The four confirmed machine-label exposures are now
redacted in the current images, with visible privacy markers; their Git history is unchanged.

## Scope and reproducibility

The inspected source is commit `3b460adb62827f674669e8e99f3090561107264f`. The inventory below
covers **all 174 tracked image paths** at that commit: 76 PNG, 81 WebP and 17 SVG, 12,885,058 bytes
in total and 171 distinct SHA-256 values. These are the pre-remediation bytes. Each path has its
own disposition even when bytes match. The working copies matched that commit when inspected;
only the four H images changed afterward, with their new hashes recorded below. The inventory
hashes are SHA-256 of original file bytes, not resized previews or decoded pixels.

The earlier audit's source `837a0d36c8d8287860b44c7913c35a698779ac80` had 168 images. The six
additional paths here are the `timeline-orchestrator/` before/proposed captures listed below.
This report covers their bytes too; it does not extend the earlier code/history audit to this tip.

The review enumerated Git's 2,060 tracked entries and checked their file signatures/MIME types,
rather than relying on image extensions alone. The independently identified image set matched
the inventory exactly. No other tracked standalone audio, video or PDF was identified. All raster
files are single-frame. Every image was viewed in full, with native-resolution reinspection of
suspected disclosures and existing blurred captures. Composite screenshots include all their
panels, including dark/light and waiting/loaded states. Local macOS Vision OCR of all 157 raster
files assisted text checks; it did not supply the visual verdict. Recognition initially failed in
the sandbox and was rerun successfully outside it, with no recognition errors. OCR can miss tiny,
truncated or blurred letters: the personal label was confirmed visually despite truncation.

All 17 SVGs were rendered locally in private headless Chrome and their complete XML was read,
including text, titles, descriptions, attributes, styles and comments. Their diagrams use generic
paths, project placeholders and pseudonymous agent handles. No SVG contains a script, event
handler, external resource link or embedded image. Some source colours depend on the surrounding
page's CSS variables; this review checked their source text as well as the rendered geometry.
It is a privacy review, not a visual-fidelity or accessibility assessment of those renderings.

ExifTool read all 174 files. PNG text chunks contain creation/modification timestamps; the
seven XMP-bearing PNGs contain toolkit, colour-space and dimension fields; fourteen PNGs contain
EXIF colour-space/dimension fields and eight WebPs have generic sRGB ICC profiles. No GPS, author,
contact, personal account, home path or credential metadata was found. SVG titles/descriptions
match the diagrams, and comments describe layout. The PNG/WebP chunk walk found no data beyond
IEND or the RIFF payload. Timestamps and generic colour-profile vendor credits were retained.

Disposable local helpers assembled the inventory, viewing sheets, OCR and metadata records.
No scanner, helper, OCR transcript or copy of exposed pixels is committed or attached. Git retains
the original evidence at the named commit; the table identifies it without republishing sensitive
strings. A reviewer can re-enumerate with `git ls-tree -r --name-only` at that commit and hash each
blob with `git cat-file blob COMMIT:PATH` piped to `shasum -a 256`, then inspect the same originals.

## Findings and concrete dispositions

### H: personal machine label — four images, redacted in the current tree

In the inspected source, the Builds and Tests lanes of `timeline-burst/a-wide.webp`, `b-wide.webp`,
`c-wide.webp` and `c-wide-dark.webp` contained the same personal machine label, truncated by the UI
but still identifying a person's computer. The literal value is withheld here. This is computer-specific
information, not an authentication token. Both phone images in that folder omit machine labels
and have no matching exposure.

These are evidence of an earlier design comparison. [Its card](../../workflow/TASKS.md#timeline-burst-drawing)
records which parts were captured and which options were drawn by hand. A new capture of today's
UI would not be evidence of that comparison. The applied remedy is an explicit opaque redaction
of the two machine-label regions in each original. Each mask visibly says `privacy redacted`, so
the edited historical capture is identified even when viewed outside its document. Lane headings,
event bars, times, commands, results, dimensions and capture/design provenance are preserved.
No synthetic computer name was substituted and no scene was generated or recaptured.

The rectangles below use original 1280×1200 pixel coordinates, with a top-left origin; each tuple
is `(x, y, width, height)`. They cover the computer text with a small margin and leave the
Builds/Tests headings untouched. The original hashes remain in the complete inventory.

| File under `docs/design/analysis/timeline-burst/` | Builds label | Tests label |
|---|---|---|
| `a-wide.webp` | `(44, 388, 156, 18)` | `(44, 678, 156, 18)` |
| `b-wide.webp` | `(44, 388, 156, 18)` | `(44, 582, 156, 18)` |
| `c-wide.webp` | `(44, 388, 156, 18)` | `(44, 477, 156, 18)` |
| `c-wide-dark.webp` | `(44, 388, 156, 18)` | `(44, 477, 156, 18)` |

ImageMagick applied deterministic opaque masks and encoded lossless WebP with metadata stripped.
A comparison of every decoded RGB pixel against the source commit proved that exactly 5,616
pixels changed per image, all inside those two rectangles: all 1,530,384 pixels outside them are
identical. Each edited image was visually reinspected at native size, and ExifTool found only
WebP dimensions/encoding information. The explicit redaction labels, not blur, cover the original
text. Existing paths and document/card links still name the same historical illustrations, now
clearly marked as privacy-redacted derivatives. No card or verdict text was edited.

| Edited file under `docs/design/analysis/timeline-burst/` | Redacted SHA-256 |
|---|---|
| `a-wide.webp` | `947ed3bbae21c5de20e7d12e26c227fe1143d0b370b75614c5547161e46cc669` |
| `b-wide.webp` | `fb71aedfbbd443f300ef7fee3f4d6f8a897d807a606547d29fdda925ebe6697c` |
| `c-wide.webp` | `7010a0101d517b2fd3ef7aa7d2b615b1120a0974d426ce4dc70c69983e4b789f` |
| `c-wide-dark.webp` | `85feb13ffbcaedb817c49ef067d3a4098a1dde05584b7ad2869bdfd7a368c66d` |

The exact paths, original hashes and eight regions were coordinated on
`docs/public-media-privacy-review` with the `inbox/github-integration` executor before changing
any image. The authorized ordinary privacy repair is complete in this tree. The original commit
and inventory hashes preserve the before evidence; the marked images and new hashes are the
after evidence. Unredacted copies were not uploaded again. This repairs no already published
Git object or external copy. The separate coordinated historical privacy decision still owns
that treatment; no history rewrite or remote action occurred.

### D: task-derived mockups — six images

`board-design/board-{a,b,c}-{phone,wide}.png` depict the consuming project's tasks and operating
state through layout mockups. The [source analysis](../../product/analysis/board-design.md#the-data) explicitly says
product titles and slugs were reworded neutrally before rendering; status, age, priority, waits
and review counts remain the recorded values. That provenance prevents calling these unredacted
original private titles or wholly synthetic data. No account name, credential or personal path
was found in them. Their remaining task themes, blocker summaries and operating state are
derived data, not wholly synthetic fixtures.

Disposition: retain these six mockups with their truthful derived-data and layout-mockup
provenance, as settled in publication disposition #10954 on
[[task:docs/public-media-privacy-review]]. Supplemental independent review #10968 inspected all
six at native dimensions and found no concrete additional disclosure. Preserve the recorded
status, age, priority, waits and review-count provenance; neutral rewording does not turn the
illustrations into wholly fictional data. No withholding or synthetic replacement is required
for the reviewed content. Original Git evidence remains available.

### B: already blurred historical captures — three images

`board-design/board-now-phone.png`, `board-now-wide.png` and `projects-now-phone.png` have existing
blur over the consuming project's identity/tab and, on the board, task contents. Native-resolution
inspection found no private identity/title confidently readable. Layout, stage counts and aggregate
health/work counts remain visible, as the [source analysis](../../product/analysis/board-design.md#the-data) records.
No attempt to reverse the blur was made; inability to read it is not proof of safe anonymization.

Disposition: retain these three historical illustrations under publication disposition #10954,
independently confirmed in supplemental review #10968. Native-size reinspection found no private
identity/title confidently readable. This bounded retention does not certify irreversible
anonymization; preserve the blur caveat, historical captions and original-history availability.
No invented historical screenshot, deletion or history rewrite follows from this disposition.

### R: retain — 161 images

The remaining images show the app mark, generic diagrams, synthetic fixtures, layout crops or
Symphony's own documented work and design discussions. They have no confirmed private content in
this review. Pseudonymous agent handles identify board sessions, not personal login accounts.
References to token usage are usage counters or design prose, not secret token values.
The generic machine placeholder in two `agents-page/` captures is documented as replaced in the
[source analysis](../../product/analysis/agents-page.md); it is not the personal machine finding H.

The public product examples in `docs/product/pictures/` use the owned fictional recipe-site fixture
described by the [product picture guide](../../product/README.md#pictures). The framework-comparison
pictures use the seeded benchmark fixture described by their
[analysis](../../engineering/analysis/client-framework-comparison.md#pictures). These do not need regeneration for privacy.
R means no confirmed finding within the stated inspection, not a whole-repository security or
publication clearance.

## Handoff and limits

This change adds this report and redacts only the four H images. No media link, runtime code,
fixture, client build input, generated client distribution, owner card or recorded verdict is
changed. The existing analysis-directory link from the [documentation index](../../README.md) makes
this report discoverable; its links point back to the provenance and tracked media. The local links and
complete inventory are validated, with the project's document and lint checks recorded on the task
thread at the ready tip. No new executable behaviour needs unit tests or new coverage.

The four confirmed machine-label exposures are repaired in the current tree; their unredacted
historical objects remain available in retained Git history. The six D and three B images are
retained under publication disposition #10954 and independent confirmation #10968, with their
derived-data and blur caveats intact. These bounded media results do not clear the rest of the
repository or a package archive.

Uninspected exclusions are reachable historical versions of images, untracked/ignored files,
board uploads and live attachments, user/consumer checkouts, remote-hosted copies, `.crate`
archives, and images or text synthesized/embedded inside source strings or generated application
bundles rather than tracked standalone media. Text documents and mockup data were read only where
needed to establish image provenance; this is not a fresh general text, dependency, licence or
credential audit. The exact 13-package archive/licence review, experiments evaluation and
historical-path treatment stay with their respective tasks. No remote settings, credentials,
live task submissions, broad media deletion or history were changed.

## Complete inventory

Each row was visually inspected and its metadata/source checked. Dispositions: **H** = confirmed
personal machine label in the inspected original, current derivative redacted; **D** = anonymized
task-derived mockup, retained with truthful provenance; **B** = already blurred historical capture,
retained with the blur limitation; **R** = retain, no confirmed finding. H is repaired only in the
current tree; D and B retention is settled by disposition #10954 and independent review #10968.
Paths are relative to the repository root. Hashes identify the inspected source commit; the four
H links now open redacted derivatives whose hashes are above. No exposed pixels are embedded.

| ID | Tracked path | Kind; width × height | SHA-256 | Disposition |
|---|---|---|---|---|
| 1 | [apps/symphony-server/src/mark/icon.svg](../../../apps/symphony-server/src/mark/icon.svg) | SVG; 32 × 32 | `1c2bc1bb484e228c88a15f7b1e226eb2af4b8462d514d6041bfa87207460d398` | R |
| 2 | [apps/symphony-server/src/mark/mark.svg](../../../apps/symphony-server/src/mark/mark.svg) | SVG; 23 × 14.4 | `723e3bd0b52506ab2135dfe9800928a1b62d32ec9b99c627760fad869ca8f14a` | R |
| 3 | [apps/symphony-server/src/mark/touch.png](../../../apps/symphony-server/src/mark/touch.png) | PNG; 180 × 180 | `462e0f93ea7270a93fca0e679b755e1b725bd146f5b86cfd177ea8b13a77ad75` | R |
| 4 | [apps/symphony-web-tests/tests/browser/fixtures/apple-touch-icon.png](../../../apps/symphony-web-tests/tests/browser/fixtures/apple-touch-icon.png) | PNG; 180 × 180 | `54262d4d9f8b448ef1428e5143d068ea30f81d72bd4cf2ddac6068ad1fa55eaf` | R |
| 5 | [docs/product/analysis/agents-page/a-phone.png](../../product/analysis/agents-page/a-phone.png) | PNG; 780 × 1688 | `0c014666f2334fc24842f2235d6d44f7f9b4a7454000f064f659c92e0da663dd` | R |
| 6 | [docs/product/analysis/agents-page/a-wide.png](../../product/analysis/agents-page/a-wide.png) | PNG; 1440 × 1639 | `f6b198789a2bfa75f706719c987911649bf1aac1307e1c635a5c7b297d82a002` | R |
| 7 | [docs/product/analysis/agents-page/b-phone.png](../../product/analysis/agents-page/b-phone.png) | PNG; 780 × 1688 | `eb3d79360d0984b083c5bc201ea18aea5be22c523247b95a4bba7db66a8812f8` | R |
| 8 | [docs/product/analysis/agents-page/b-wide.png](../../product/analysis/agents-page/b-wide.png) | PNG; 1440 × 1412 | `daadda4a3ce6fdd9a09c60fb55edaeb478fe99d7c0ff85755912c3c309ac8db3` | R |
| 9 | [docs/product/analysis/agents-page/c-phone.png](../../product/analysis/agents-page/c-phone.png) | PNG; 780 × 1688 | `6102fc0a822e4652ee8e6660b628d6efbae801ae2fb7b3f47e5aa8001d4dea28` | R |
| 10 | [docs/product/analysis/agents-page/c-wide.png](../../product/analysis/agents-page/c-wide.png) | PNG; 1440 × 1284 | `82afd830d3e5b8feaacd96a0552de3d23b1eb1c7550c878bbc4cd1b5027f8b9c` | R |
| 11 | [docs/product/analysis/agents-page/today-phone-orchestrator.png](../../product/analysis/agents-page/today-phone-orchestrator.png) | PNG; 780 × 1688 | `286b225e223aa26d579c174890fefc19bae4d4330768d8d06d903a2ab1646d79` | R |
| 12 | [docs/product/analysis/agents-page/today-phone.png](../../product/analysis/agents-page/today-phone.png) | PNG; 780 × 1688 | `d189f0e3ac48a077a628cf186a88fc5c48b9fc2476763cd5c0ea306079477d92` | R |
| 13 | [docs/product/analysis/agents-page/today-wide.png](../../product/analysis/agents-page/today-wide.png) | PNG; 1440 × 900 | `3fba92e254c20c7636e6a54ec84587053cb5f578778a65a4e45bc458c015f0ab` | R |
| 14 | [docs/workflow/analysis/attached-artifacts/proposed.svg](../../workflow/analysis/attached-artifacts/proposed.svg) | SVG; 1000 × 540 | `c412eae37ac870d3630893a6724dcba98bb353a7426e4b45cbaaa06976f84e13` | R |
| 15 | [docs/workflow/analysis/attached-artifacts/split.svg](../../workflow/analysis/attached-artifacts/split.svg) | SVG; 1000 × 560 | `bc9187a31b6bf620a92093f525cff42b2b49e92c46df001ac00762aef38f2d59` | R |
| 16 | [docs/workflow/analysis/attached-artifacts/today.svg](../../workflow/analysis/attached-artifacts/today.svg) | SVG; 1000 × 540 | `c43e30127c218082df0e197b83db3ed22ed9f26d4a53e4ba07b8c1c61f3dd3e9` | R |
| 17 | [docs/product/analysis/board-design/board-a-phone.png](../../product/analysis/board-design/board-a-phone.png) | PNG; 780 × 1688 | `630f130ca25b0a3cc295caa087c815e41485c673078e8f5f815b6831cf024fa7` | D |
| 18 | [docs/product/analysis/board-design/board-a-wide.png](../../product/analysis/board-design/board-a-wide.png) | PNG; 1440 × 900 | `87d921b12fac915390fcee3659a2e8610dbb7d826b32479139f095d73248b050` | D |
| 19 | [docs/product/analysis/board-design/board-b-phone.png](../../product/analysis/board-design/board-b-phone.png) | PNG; 780 × 1688 | `684b10606c618f88f5b352a21e9278c97617595fc15c8999e919d91ea9b82f40` | D |
| 20 | [docs/product/analysis/board-design/board-b-wide.png](../../product/analysis/board-design/board-b-wide.png) | PNG; 1440 × 900 | `3d503086bd30af8dcd0c5e693ec9ff16f6f011a42a2c4540ed91b39e0adc5e02` | D |
| 21 | [docs/product/analysis/board-design/board-c-phone.png](../../product/analysis/board-design/board-c-phone.png) | PNG; 780 × 1688 | `4151839e64e1d52328afb9fe6b37239edc511721dad17cca65ecac2ca4c06c88` | D |
| 22 | [docs/product/analysis/board-design/board-c-wide.png](../../product/analysis/board-design/board-c-wide.png) | PNG; 1440 × 900 | `f0adbb2197d8773ce8d4a6b9275e09bdbda47f736aea421e0ab907cd49e80279` | D |
| 23 | [docs/product/analysis/board-design/board-now-phone.png](../../product/analysis/board-design/board-now-phone.png) | PNG; 780 × 1688 | `840ce90c087be5f63768fc2af5da62dd1c40ef24d549fc850aae1314f9b92284` | B |
| 24 | [docs/product/analysis/board-design/board-now-wide.png](../../product/analysis/board-design/board-now-wide.png) | PNG; 1440 × 900 | `3b6938af626741bdcbcd85dcc417d9aecddfac54b19c69832ff88ab4b4c7f3c4` | B |
| 25 | [docs/product/analysis/board-design/projects-health-phone.png](../../product/analysis/board-design/projects-health-phone.png) | PNG; 780 × 720 | `50d507f4fbc4c6c3ee62c35bc312df671c99726c29445f836e0ff5f796967d1d` | R |
| 26 | [docs/product/analysis/board-design/projects-health-wide.png](../../product/analysis/board-design/projects-health-wide.png) | PNG; 1425 × 330 | `412b963943767aa7a48d1db647d6429c1e3bc6adead458a8bbf84eb98903e3ee` | R |
| 27 | [docs/product/analysis/board-design/projects-now-phone.png](../../product/analysis/board-design/projects-now-phone.png) | PNG; 780 × 640 | `db26afbf754272424b40eb030cb1a3efe146a7634e5642950ee0f62903bfc369` | B |
| 28 | [docs/engineering/analysis/client-framework-board-dioxus-phone.webp](../../engineering/analysis/client-framework-board-dioxus-phone.webp) | WEBP; 585 × 1266 | `6728ee9062c247f806c518b18847bd64fdca3aca0b46f6a847779d8db10d2c6d` | R |
| 29 | [docs/engineering/analysis/client-framework-board-dioxus-wide.webp](../../engineering/analysis/client-framework-board-dioxus-wide.webp) | WEBP; 1440 × 900 | `f00935f08e542b09e70d0591b290d59faacf3fcd0665e2f4b200125b21008900` | R |
| 30 | [docs/engineering/analysis/client-framework-board-preact-phone.webp](../../engineering/analysis/client-framework-board-preact-phone.webp) | WEBP; 585 × 1266 | `a63e09dec5ad4aa25a5512cfa584958c8e93ba02776a330a7d926e006d7a00d7` | R |
| 31 | [docs/engineering/analysis/client-framework-board-preact-wide.webp](../../engineering/analysis/client-framework-board-preact-wide.webp) | WEBP; 1440 × 900 | `6f2ec996e5162fb82a2cc8ddce84464805834c739f9a043bb3a5755b9bdfad6d` | R |
| 32 | [docs/engineering/analysis/client-framework-document-dioxus-phone.webp](../../engineering/analysis/client-framework-document-dioxus-phone.webp) | WEBP; 585 × 1266 | `ce3031e99a838482bf39757815a482b242ff711390c98a7e8d0214f773a2d7cd` | R |
| 33 | [docs/engineering/analysis/client-framework-document-dioxus-wide.webp](../../engineering/analysis/client-framework-document-dioxus-wide.webp) | WEBP; 1440 × 900 | `faca6e7f739991da557662aa0bbb14655912860cafe6e8d49844524c9ca428aa` | R |
| 34 | [docs/engineering/analysis/client-framework-document-preact-phone.webp](../../engineering/analysis/client-framework-document-preact-phone.webp) | WEBP; 585 × 1266 | `ce3031e99a838482bf39757815a482b242ff711390c98a7e8d0214f773a2d7cd` | R |
| 35 | [docs/engineering/analysis/client-framework-document-preact-wide.webp](../../engineering/analysis/client-framework-document-preact-wide.webp) | WEBP; 1440 × 900 | `5cf91dde5971128492f7c383ef85ffb11d19aa55b4c6ac01e67873fa3e044996` | R |
| 36 | [docs/engineering/analysis/client-framework-task-dioxus-phone.webp](../../engineering/analysis/client-framework-task-dioxus-phone.webp) | WEBP; 585 × 1266 | `aa339871396b39172773f11f5cfa1cfc85a540243fc68285cfe7ceb0acfa4c06` | R |
| 37 | [docs/engineering/analysis/client-framework-task-dioxus-wide.webp](../../engineering/analysis/client-framework-task-dioxus-wide.webp) | WEBP; 1440 × 900 | `9414dfe891ab181f5543f41fd21d881b39bf26b357b929fe9873496f98c7bf6d` | R |
| 38 | [docs/engineering/analysis/client-framework-task-preact-phone.webp](../../engineering/analysis/client-framework-task-preact-phone.webp) | WEBP; 585 × 1266 | `3c70eddd866fbfa89bbeabd30f877c39568b01a14dc594e37ddc35740a3c3b01` | R |
| 39 | [docs/engineering/analysis/client-framework-task-preact-wide.webp](../../engineering/analysis/client-framework-task-preact-wide.webp) | WEBP; 1440 × 900 | `a960a9673da4a2c47919fd69f00dd13c74c19b9d93470f7ddcb071159e4138d5` | R |
| 40 | [docs/engineering/analysis/client-framework-verdict-dioxus-wide.webp](../../engineering/analysis/client-framework-verdict-dioxus-wide.webp) | WEBP; 1440 × 900 | `8452763fde8d1f1c1e2cb2cb1127b20fad3de7f7c5f1fb72e72ff84b4890e336` | R |
| 41 | [docs/engineering/analysis/client-framework-verdict-preact-wide.webp](../../engineering/analysis/client-framework-verdict-preact-wide.webp) | WEBP; 1440 × 900 | `90f64e4beddd9ca4d0bdbde578f8d8f132e17629e62f8dd1b34dba42d0ddb769` | R |
| 42 | [docs/design/analysis/header-names/appearance-after-phone.png](../../design/analysis/header-names/appearance-after-phone.png) | PNG; 780 × 532 | `4838aad2c48bb7647bb11d93f692bd9e5ba0dfc0269a4bcf878eda08f0d87b5b` | R |
| 43 | [docs/design/analysis/header-names/appearance-after-wide.png](../../design/analysis/header-names/appearance-after-wide.png) | PNG; 780 × 272 | `1199ad0cb73992204707e1db22080052b9d7b8f36fcfadecc423064ec3fad8d4` | R |
| 44 | [docs/design/analysis/header-names/appearance-before-phone.png](../../design/analysis/header-names/appearance-before-phone.png) | PNG; 780 × 712 | `c76038e6c6800447d50df994c24427f8c8064e2d16495832fffcc7247c229e8b` | R |
| 45 | [docs/design/analysis/header-names/appearance-before-wide.png](../../design/analysis/header-names/appearance-before-wide.png) | PNG; 780 × 580 | `0c2c5e27cb87e462d3672ef11abedde5de99b0548447a534baaf527411cbc2ff` | R |
| 46 | [docs/design/analysis/header-names/names-after-phone.png](../../design/analysis/header-names/names-after-phone.png) | PNG; 780 × 362 | `bc5463ee8bee598642f414288a0f86766f94ca19e4461d18a442a3b70261e5d6` | R |
| 47 | [docs/design/analysis/header-names/names-after-wide.png](../../design/analysis/header-names/names-after-wide.png) | PNG; 780 × 156 | `9e8b7764105c0e9b0cb1d879f434b365a804b7c9bade699715aa737f83f7109a` | R |
| 48 | [docs/design/analysis/header-names/names-before-phone.png](../../design/analysis/header-names/names-before-phone.png) | PNG; 780 × 402 | `7838396edeabc5a7c7ec41c9f616e44ef152e4be21ef0a04765be7c69899083d` | R |
| 49 | [docs/design/analysis/header-names/names-before-wide.png](../../design/analysis/header-names/names-before-wide.png) | PNG; 780 × 156 | `8eb2dcc1e07293b0e3071116a852f90fbfb59327ce876eca4527eca0924f9614` | R |
| 50 | [docs/design/analysis/header-names/phone-after-wide.png](../../design/analysis/header-names/phone-after-wide.png) | PNG; 780 × 192 | `a7158dbed587739ee8b75ea45427bdca1c174996226ea3dfeb37b896b1aecf29` | R |
| 51 | [docs/design/analysis/header-names/phone-after.png](../../design/analysis/header-names/phone-after.png) | PNG; 780 × 468 | `81885e522e5bab90be5c3dc941c00525fb3b25d1342e17627e36f38b1568b258` | R |
| 52 | [docs/design/analysis/header-names/phone-before-wide.png](../../design/analysis/header-names/phone-before-wide.png) | PNG; 780 × 192 | `b2c70ffe3f2099450f9d742ff4d6f9ab478a7976bfe4e70788ddd9d73d0de191` | R |
| 53 | [docs/design/analysis/header-names/phone-before.png](../../design/analysis/header-names/phone-before.png) | PNG; 780 × 508 | `0b841c450e79e7a6e5e6e460ffbeaaa7c904012073491c4f3a2f56992abf7366` | R |
| 54 | [docs/design/analysis/header-names/sheet-after-phone.png](../../design/analysis/header-names/sheet-after-phone.png) | PNG; 780 × 712 | `afda37188381474e2b2a60828f4e3162d3d6b23e73796aabefa1c9ee016a441c` | R |
| 55 | [docs/design/analysis/header-names/sheet-after-wide.png](../../design/analysis/header-names/sheet-after-wide.png) | PNG; 780 × 580 | `35f09a6df6f11db059e461f83017bde8f1a58912df90426be7e483c32d59d7d1` | R |
| 56 | [docs/product/analysis/owner-asks-board-asks-phone.png](../../product/analysis/owner-asks-board-asks-phone.png) | PNG; 1170 × 2532 | `e548219e6155be987ce00a2c8973dec1202e77c157db00527bb6567bcc67ea70` | R |
| 57 | [docs/product/analysis/owner-asks-board-asks.png](../../product/analysis/owner-asks-board-asks.png) | PNG; 1440 × 1000 | `027185d372237b07669e2d467a69548b5b401b942e1d5f59a6d45c380567f8b1` | R |
| 58 | [docs/product/analysis/owner-asks-board-today.png](../../product/analysis/owner-asks-board-today.png) | PNG; 1440 × 1000 | `fa48a9b0d67d060c3839c06e7ccbac134f432d12ddcdab4ce3b8d9176efd7015` | R |
| 59 | [docs/product/analysis/owner-asks-home-asks.png](../../product/analysis/owner-asks-home-asks.png) | PNG; 1440 × 900 | `83455252b67d1d8bd2dc93cc5c2d878ad8444c61d8d131deadf6041256b3f5a5` | R |
| 60 | [docs/product/analysis/owner-asks-queue-asks-phone.png](../../product/analysis/owner-asks-queue-asks-phone.png) | PNG; 1170 × 2532 | `52bca2ff2618555ae6121fd340f183b4ecb4addd4b14d9950b7d6421165ff0c4` | R |
| 61 | [docs/product/analysis/owner-asks-queue-asks.png](../../product/analysis/owner-asks-queue-asks.png) | PNG; 1440 × 900 | `9feb54a882cc7cb4e4057a03b10ba6696bb40c670b16e8934888968620d84dad` | R |
| 62 | [docs/product/analysis/owner-asks-queue-today.png](../../product/analysis/owner-asks-queue-today.png) | PNG; 1440 × 900 | `8b836c2e68420ff0906ce3150e28f46f7eecb015dfa2846979709fc39c960762` | R |
| 63 | [docs/product/analysis/owner-asks-related-today.png](../../product/analysis/owner-asks-related-today.png) | PNG; 1440 × 900 | `e16be15a59b895c548beefa209bec8643e17ae476de17177e294c62fdddc550f` | R |
| 64 | [docs/design/analysis/phone-header/board-dark.webp](../../design/analysis/phone-header/board-dark.webp) | WEBP; 1827 × 864 | `11f16bb2d81c0a188d2aab4cb9ac5c1873d099e8be43e8d3a7b807aad25f5884` | R |
| 65 | [docs/design/analysis/phone-header/board-light.webp](../../design/analysis/phone-header/board-light.webp) | WEBP; 1827 × 864 | `fa9dc54750ce2c9d5cdd4c2ea5d039f156be3a5b362a8a8491ad57e097c0ee88` | R |
| 66 | [docs/design/analysis/phone-header/doc-dark.webp](../../design/analysis/phone-header/doc-dark.webp) | WEBP; 1827 × 864 | `0611deb2469124641bcfda71e06c89783818142303bcba23a467ca3c575878f5` | R |
| 67 | [docs/design/analysis/phone-header/doc-light.webp](../../design/analysis/phone-header/doc-light.webp) | WEBP; 1827 × 864 | `26000db65a37d4a29410ac39c93420a1872977c12ba5e1828a386b0cf7fd4d19` | R |
| 68 | [docs/design/analysis/phone-header/mark-only-board-dark.webp](../../design/analysis/phone-header/mark-only-board-dark.webp) | WEBP; 1827 × 864 | `eb3e4a7aa6065b283e5ca7162f33926cbb4084dccb820bc09fe6ae3b244be2fe` | R |
| 69 | [docs/design/analysis/phone-header/mark-only-board-light.webp](../../design/analysis/phone-header/mark-only-board-light.webp) | WEBP; 1827 × 864 | `5f081a5776a5af7935cc9900b75da3ab05bea9efd522be36cc8be249777be8c9` | R |
| 70 | [docs/design/analysis/phone-header/mark-only-doc-dark.webp](../../design/analysis/phone-header/mark-only-doc-dark.webp) | WEBP; 1827 × 864 | `e3e654a7504183bcf057097636112d99bba78dd6acb3c9709ec0d7298fbfd44c` | R |
| 71 | [docs/design/analysis/phone-header/mark-only-doc-light.webp](../../design/analysis/phone-header/mark-only-doc-light.webp) | WEBP; 1827 × 864 | `098ea57cf47192741124e428872fcb09e2a915a7f3e9ed63354cd4ea53b38e03` | R |
| 72 | [docs/engineering/analysis/plugin-delivery/crates.svg](../../engineering/analysis/plugin-delivery/crates.svg) | SVG; 1000 × 600 | `04d4e9adddd0144e3781444c8f87308b68bbdb22c33efb276bc4bb877f94bdeb` | R |
| 73 | [docs/engineering/analysis/plugin-delivery/proposed.svg](../../engineering/analysis/plugin-delivery/proposed.svg) | SVG; 1000 × 560 | `073e60423e86288ce46216ea6aaf459996e5b04e693251b1e03cc0d62faed81e` | R |
| 74 | [docs/engineering/analysis/plugin-delivery/today.svg](../../engineering/analysis/plugin-delivery/today.svg) | SVG; 1000 × 560 | `e83def8d0591b2fe3c570d2b1b2da97fc99b28611efdb7ff7fe8efbca51b14aa` | R |
| 75 | [docs/workflow/analysis/release-candidate/proposed.svg](../../workflow/analysis/release-candidate/proposed.svg) | SVG; 440 × 660 | `964ac0141638af9a2cbcd1e8b9f168970abafabd52f5250d8e6c269499796be2` | R |
| 76 | [docs/workflow/analysis/release-candidate/today.svg](../../workflow/analysis/release-candidate/today.svg) | SVG; 440 × 560 | `0f3767ee393e2686aa02470b6a913dfddc28d26d074d3931f0d93e00eada96a1` | R |
| 77 | [docs/product/analysis/reports-gates-a-phone.svg](../../product/analysis/reports-gates-a-phone.svg) | SVG; 390 × 658 | `5b4ddf620e99a8bafa0df409e8f67f00c9b1eb3c39706d62fd6fb7b7c929b4cc` | R |
| 78 | [docs/product/analysis/reports-gates-a-wide.svg](../../product/analysis/reports-gates-a-wide.svg) | SVG; 1000 × 538 | `569ea6aeb4ad8f6570c703d03149ad30adab0b2e3728e4874856f3401ae27f45` | R |
| 79 | [docs/product/analysis/reports-gates-b-phone.svg](../../product/analysis/reports-gates-b-phone.svg) | SVG; 390 × 618 | `c8affff2d26a8a2ccb4515eea7e5e76a1c0d5e9e69e3b159ce5316b026d6b9aa` | R |
| 80 | [docs/product/analysis/reports-gates-b-wide.svg](../../product/analysis/reports-gates-b-wide.svg) | SVG; 1000 × 572 | `ef9ded1879a8255dcc7009abe4532d4369ebdbd3e1dc86148ec37dcaf707c511` | R |
| 81 | [docs/product/analysis/reports-timeline-phone.svg](../../product/analysis/reports-timeline-phone.svg) | SVG; 390 × 752 | `fd11a7f7b1cadf88bce11f43ece7199851f54896114cb8bb2b1c7abc90236c94` | R |
| 82 | [docs/product/analysis/reports-timeline-wide.svg](../../product/analysis/reports-timeline-wide.svg) | SVG; 1200 × 512 | `42d347130490ce4e11fbf58aad54477ad391c138a20c15fbe714a18b90357857` | R |
| 83 | [docs/design/analysis/skeleton-first-visit-a.webp](../../design/analysis/skeleton-first-visit-a.webp) | WEBP; 626 × 676 | `07e888ddf4d21750a4d4229fb8b6f3325373684a19f1879751f2f38920616ddc` | R |
| 84 | [docs/design/analysis/skeleton-first-visit-b.webp](../../design/analysis/skeleton-first-visit-b.webp) | WEBP; 626 × 676 | `d2e84211b7bad85329fac896aa0ddf2b93d67ae358d9da56096a60af89e99501` | R |
| 85 | [docs/design/analysis/skeleton-first-visit-base.webp](../../design/analysis/skeleton-first-visit-base.webp) | WEBP; 626 × 676 | `d7ed42426fabba6b4d253a37da949f5826ecd9af2245effba94c0fbaedc24790` | R |
| 86 | [docs/design/analysis/skeleton-kept-document-phone.webp](../../design/analysis/skeleton-kept-document-phone.webp) | WEBP; 938 × 1352 | `f1cce6e90055e00fd8e3589a85c922c987741c21df64c73f71aa6ff2019b3103` | R |
| 87 | [docs/design/analysis/skeleton-kept-document-wide.webp](../../design/analysis/skeleton-kept-document-wide.webp) | WEBP; 1923 × 802 | `3b4871f260c6d1d7c32f67587571ae61c1f3fe61b6ad9915293eb814d36442cb` | R |
| 88 | [docs/design/analysis/skeleton-kept-queue-phone.webp](../../design/analysis/skeleton-kept-queue-phone.webp) | WEBP; 1251 × 1352 | `920abed18255e64635b2f303f77327c360773bc9e0b587ad6a80b5ca776ddf0a` | R |
| 89 | [docs/design/analysis/skeleton-kept-queue-wide.webp](../../design/analysis/skeleton-kept-queue-wide.webp) | WEBP; 2564 × 802 | `2da1335e3e9ae3dd171619d980d00877051c75ba4c0769c81712ba39fae75280` | R |
| 90 | [docs/design/analysis/skeleton-kept-task-phone.webp](../../design/analysis/skeleton-kept-task-phone.webp) | WEBP; 1564 × 1352 | `270b798c654c367a71c782c03d7e59899af016bb4b220f1c3005a37a25251dc0` | R |
| 91 | [docs/design/analysis/skeleton-kept-task-wide.webp](../../design/analysis/skeleton-kept-task-wide.webp) | WEBP; 3205 × 802 | `c0d50ac36b3f677cd73bb57a6a64ed07947fa8e26615b5369885578f4927fa8d` | R |
| 92 | [docs/design/analysis/skeleton-kept-tasks-phone.webp](../../design/analysis/skeleton-kept-tasks-phone.webp) | WEBP; 1251 × 1352 | `4944af36b5a6a7a1916df4c8ca728fff2e1d0f271f820d01fe849733817de65f` | R |
| 93 | [docs/design/analysis/skeleton-kept-tasks-wide.webp](../../design/analysis/skeleton-kept-tasks-wide.webp) | WEBP; 2564 × 802 | `a27cebc07b4aa191cd1ae1314bc7b1e615f93428fede21e30a9953fc7dab0ecd` | R |
| 94 | [docs/design/analysis/skeleton-loading-chat-phone-before.webp](../../design/analysis/skeleton-loading-chat-phone-before.webp) | WEBP; 780 × 1688 | `9d863b9dbc1d9590cacd447097fffa2a3e1a5deea80eb42d1853654acf41560a` | R |
| 95 | [docs/design/analysis/skeleton-loading-chat-phone.webp](../../design/analysis/skeleton-loading-chat-phone.webp) | WEBP; 780 × 1688 | `bb177e22cb480aa65068f5dd36f7793407fad4f9224b09635942f14dcfaafaf0` | R |
| 96 | [docs/design/analysis/skeleton-loading-document-phone-before.webp](../../design/analysis/skeleton-loading-document-phone-before.webp) | WEBP; 780 × 1688 | `f068233dc2dd43d449e7113539fa929587067950d84b94422bb34e6a2a3d31bb` | R |
| 97 | [docs/design/analysis/skeleton-loading-document-phone-dark.webp](../../design/analysis/skeleton-loading-document-phone-dark.webp) | WEBP; 780 × 1688 | `f9d607448a3ed7c748e279ee40f1a08449e4e8739876c4680d8a951fea7d9acf` | R |
| 98 | [docs/design/analysis/skeleton-loading-document-phone.webp](../../design/analysis/skeleton-loading-document-phone.webp) | WEBP; 780 × 1688 | `58bb5d8332468db61af00d8958d4386938f55dafd07543033ed155f3b3e563b8` | R |
| 99 | [docs/design/analysis/skeleton-loading-document-wide.webp](../../design/analysis/skeleton-loading-document-wide.webp) | WEBP; 2560 × 1800 | `bd78feb0a7b109f33f7f85d8451c45b6605c911409941bb45ccd76e7e7aed261` | R |
| 100 | [docs/design/analysis/skeleton-loading-task-phone-before.webp](../../design/analysis/skeleton-loading-task-phone-before.webp) | WEBP; 780 × 1688 | `847c08ea139493d17567ee47e062cde59a026b9fae7c8d0bf277ed3eda4bd561` | R |
| 101 | [docs/design/analysis/skeleton-loading-task-phone.webp](../../design/analysis/skeleton-loading-task-phone.webp) | WEBP; 780 × 1688 | `87bd1dd26627235152a8bf2bc6a8daa662af5eaea9bda7b046c7f7862e155a63` | R |
| 102 | [docs/design/analysis/skeleton-outline-chat-phone.webp](../../design/analysis/skeleton-outline-chat-phone.webp) | WEBP; 938 × 1352 | `e4813cab9f359084c786c4a099c10ffc0443f6a0eb4f2a607380b66875bfd98c` | R |
| 103 | [docs/design/analysis/skeleton-outline-chat-wide.webp](../../design/analysis/skeleton-outline-chat-wide.webp) | WEBP; 1923 × 802 | `cdb7b1bba0815f3228ea0c87cc8917976363eda0b78f9c0e8fb781282117c852` | R |
| 104 | [docs/design/analysis/skeleton-outline-document-part-phone.webp](../../design/analysis/skeleton-outline-document-part-phone.webp) | WEBP; 626 × 1352 | `828ddc54db4d8f349963c5e6297ae1d97c1797dff5ae28c78def1878a8c9c30e` | R |
| 105 | [docs/design/analysis/skeleton-outline-document-part-wide.webp](../../design/analysis/skeleton-outline-document-part-wide.webp) | WEBP; 1282 × 802 | `371e2d57d82f7c403fedfb8a5e4f5cc7df42edf54bafdf112ece105e6f388b1b` | R |
| 106 | [docs/design/analysis/skeleton-outline-document-phone.webp](../../design/analysis/skeleton-outline-document-phone.webp) | WEBP; 938 × 1352 | `76fc2ef107cbc82d95deb0177e91ddf057071a23c9ee92161ebee0a847aae116` | R |
| 107 | [docs/design/analysis/skeleton-outline-document-wide.webp](../../design/analysis/skeleton-outline-document-wide.webp) | WEBP; 1923 × 802 | `273bda9679adb3f746c5a890b6dadb932798683366e894c14b00b500ae8405d4` | R |
| 108 | [docs/design/analysis/skeleton-outline-gates-task-part-phone.webp](../../design/analysis/skeleton-outline-gates-task-part-phone.webp) | WEBP; 626 × 1352 | `f3bdc625b50e6a7e0f5e36dd66884f76a51fb51fc6a5989bd9d891bfdb355d18` | R |
| 109 | [docs/design/analysis/skeleton-outline-gates-task-part-wide.webp](../../design/analysis/skeleton-outline-gates-task-part-wide.webp) | WEBP; 1282 × 802 | `434534fe4bca42f77c647e6fb9b76908009493cc4cbe46cd93fe1ac4cd9c492d` | R |
| 110 | [docs/design/analysis/skeleton-outline-gates-task-phone.webp](../../design/analysis/skeleton-outline-gates-task-phone.webp) | WEBP; 938 × 1352 | `a65f1fba7958cddc1bec08efcca2603b9a414712f9a092c7ed81edfeca34d179` | R |
| 111 | [docs/design/analysis/skeleton-outline-gates-task-wide.webp](../../design/analysis/skeleton-outline-gates-task-wide.webp) | WEBP; 1923 × 802 | `1376a3ef4351d703c62267ed7488fd20df9311a7043828a3f7e734f73f0b71f6` | R |
| 112 | [docs/design/analysis/skeleton-outline-queue-phone.webp](../../design/analysis/skeleton-outline-queue-phone.webp) | WEBP; 938 × 1352 | `587af6e9de2d00c6962b951af8e5257f2acdbdc68e8e91e37442a55f005961f2` | R |
| 113 | [docs/design/analysis/skeleton-outline-queue-wide.webp](../../design/analysis/skeleton-outline-queue-wide.webp) | WEBP; 1923 × 802 | `21365dee97693d091b4553f328137534954fd2b6d920ce4a76172fe3b877e6f2` | R |
| 114 | [docs/design/analysis/skeleton-outline-task-part-phone.webp](../../design/analysis/skeleton-outline-task-part-phone.webp) | WEBP; 626 × 1352 | `d392fde8d56d8adcbb3ca3e66841feee7bbc7dddb43f76ec2dd93ea707c17b44` | R |
| 115 | [docs/design/analysis/skeleton-outline-task-part-wide.webp](../../design/analysis/skeleton-outline-task-part-wide.webp) | WEBP; 1282 × 802 | `c4f9cec99b6fca742edf6b4c82152adc6b9af36c06d351cf270060d54687536c` | R |
| 116 | [docs/design/analysis/skeleton-outline-task-phone.webp](../../design/analysis/skeleton-outline-task-phone.webp) | WEBP; 938 × 1352 | `cbfe3380d2defdc9ee527c0fd0214f5ca8472265f4435338399b43c6a00628f1` | R |
| 117 | [docs/design/analysis/skeleton-outline-task-wide.webp](../../design/analysis/skeleton-outline-task-wide.webp) | WEBP; 1923 × 802 | `a53207e5a5527fd23d11da116febb2f3ac28e288316130f04df3764ab38463b6` | R |
| 118 | [docs/design/analysis/skeleton-outline-tasks-phone.webp](../../design/analysis/skeleton-outline-tasks-phone.webp) | WEBP; 938 × 1352 | `3459804acccf763e1983a07025021b804190f6ee73e1b83e323a4d6160658999` | R |
| 119 | [docs/design/analysis/skeleton-outline-tasks-wide.webp](../../design/analysis/skeleton-outline-tasks-wide.webp) | WEBP; 1923 × 802 | `2b50850d48f4d3d1d87bcbfa3ecb2832032d775524d637bfc08ab94238de2dbd` | R |
| 120 | [docs/design/analysis/skeleton-shape-document-phone.webp](../../design/analysis/skeleton-shape-document-phone.webp) | WEBP; 938 × 1352 | `f1cce6e90055e00fd8e3589a85c922c987741c21df64c73f71aa6ff2019b3103` | R |
| 121 | [docs/design/analysis/skeleton-shape-document-wide.webp](../../design/analysis/skeleton-shape-document-wide.webp) | WEBP; 1923 × 802 | `3b4871f260c6d1d7c32f67587571ae61c1f3fe61b6ad9915293eb814d36442cb` | R |
| 122 | [docs/design/analysis/skeleton-shape-queue-phone.webp](../../design/analysis/skeleton-shape-queue-phone.webp) | WEBP; 938 × 1352 | `6a205b4a322ae81bd37a6eca376242849b81862304ec3b69e8b15c310ba3eba9` | R |
| 123 | [docs/design/analysis/skeleton-shape-queue-wide.webp](../../design/analysis/skeleton-shape-queue-wide.webp) | WEBP; 1923 × 802 | `1e8e4e8507a2080f9535d9205cab5b1a1554f7faf41df8b93e3debb4d62a7ebb` | R |
| 124 | [docs/design/analysis/skeleton-shape-task-part-phone.webp](../../design/analysis/skeleton-shape-task-part-phone.webp) | WEBP; 626 × 1352 | `70d858144668f0e8f3a54a364e10e2d883ff6fd8608ebb42d182c36b0d25dee6` | R |
| 125 | [docs/design/analysis/skeleton-shape-task-part-wide.webp](../../design/analysis/skeleton-shape-task-part-wide.webp) | WEBP; 1282 × 802 | `e9ae2b3e4bbe013acc4de370d6fd2ed2cb56badd50391e3cda45833cb1a72b3a` | R |
| 126 | [docs/design/analysis/skeleton-shape-task-phone.webp](../../design/analysis/skeleton-shape-task-phone.webp) | WEBP; 1251 × 1352 | `134544ccf061a99e124717fea05ede16ba9622547353d1ab56705129e8d9b9f6` | R |
| 127 | [docs/design/analysis/skeleton-shape-task-wide.webp](../../design/analysis/skeleton-shape-task-wide.webp) | WEBP; 2564 × 802 | `92d07a1c4eb13691997c445949aa18c361896b44ff8ed3f48170ae7492ac0d83` | R |
| 128 | [docs/design/analysis/skeleton-shape-tasks-phone.webp](../../design/analysis/skeleton-shape-tasks-phone.webp) | WEBP; 1251 × 1352 | `171fe78c778aafeb9b73bde62d0d153df04dd8f2c706d0b141f12f57daf1c3aa` | R |
| 129 | [docs/design/analysis/skeleton-shape-tasks-wide.webp](../../design/analysis/skeleton-shape-tasks-wide.webp) | WEBP; 2564 × 802 | `225f898ce3d8fb59eb3962d7ae743168934591e6a658d95922a69b1d9ad31110` | R |
| 130 | [docs/workflow/analysis/sweep-dry-run-set-aside.svg](../../workflow/analysis/sweep-dry-run-set-aside.svg) | SVG; 860 × 512 | `f28b879ca9ed92604aa0a7e58ac3a2d0db6a7fc05710209f9aead4080d5ab531` | R |
| 131 | [docs/design/analysis/symphony-mark/a.png](../../design/analysis/symphony-mark/a.png) | PNG; 1000 × 351 | `5273397a68ccb9bb6d69c20198a085ecb9bfee6adef6a1c402ba234c54550681` | R |
| 132 | [docs/design/analysis/symphony-mark/b.png](../../design/analysis/symphony-mark/b.png) | PNG; 1000 × 351 | `a97a81d2c001dd947cd469714737746f3f8b973d6be9b6051cc2818fdec0eaba` | R |
| 133 | [docs/design/analysis/symphony-mark/c.png](../../design/analysis/symphony-mark/c.png) | PNG; 1000 × 351 | `42240e5b03e6ff66a6d243d36612f684a65530da99a476307120db6ad6e1621e` | R |
| 134 | [docs/design/analysis/symphony-mark/d.png](../../design/analysis/symphony-mark/d.png) | PNG; 1000 × 351 | `ce66ddce07d156e020cea0fda4cb49e93e00124abf80fcc3128c6f09c30a2ffa` | R |
| 135 | [docs/design/analysis/symphony-mark/sizes.png](../../design/analysis/symphony-mark/sizes.png) | PNG; 864 × 580 | `edbaa4aab3d9eb3a3cac1b707ea3cf298efe6b8e3c8f061b85d0993e53a3cb91` | R |
| 136 | [docs/design/analysis/symphony-mark/today.png](../../design/analysis/symphony-mark/today.png) | PNG; 1000 × 193 | `032d86104fad2db02e42a558fd8797a1d92e2b0d529d071c5adda1bb329a9be1` | R |
| 137 | [docs/design/analysis/task-first-visit-six-gates-phone.webp](../../design/analysis/task-first-visit-six-gates-phone.webp) | WEBP; 1202 × 890 | `6a783c9540eb7fcabea167dadca8e0fb724c681a9a6c229e37b474735534bc16` | R |
| 138 | [docs/design/analysis/task-first-visit-six-gates-wide.webp](../../design/analysis/task-first-visit-six-gates-wide.webp) | WEBP; 3104 × 766 | `07f2a3d1a126ddbf27b3d840573d7fb5233f4048223066d380557ca62438d176` | R |
| 139 | [docs/design/analysis/task-first-visit-two-gates-phone-a.webp](../../design/analysis/task-first-visit-two-gates-phone-a.webp) | WEBP; 796 × 890 | `299f52021007c8c4cc0dd328dda42db4a0bd297acda926b78385ee1c8ba7e555` | R |
| 140 | [docs/design/analysis/task-first-visit-two-gates-phone-b.webp](../../design/analysis/task-first-visit-two-gates-phone-b.webp) | WEBP; 796 × 890 | `7aa8b2b87ba8ef37c32bfc1a913e276f16773d77a6a0717676c51852a88951c6` | R |
| 141 | [docs/design/analysis/task-first-visit-two-gates-phone.webp](../../design/analysis/task-first-visit-two-gates-phone.webp) | WEBP; 1202 × 890 | `8bf73140e76fcc7177f0996bfd8fb7243b6ab62ca5cd3ec0bcee0961ad5cf016` | R |
| 142 | [docs/design/analysis/task-first-visit-two-gates-wide.webp](../../design/analysis/task-first-visit-two-gates-wide.webp) | WEBP; 3104 × 766 | `2e41896054f203ef9f7ed1061b9ce3f2bec722ac3a0aeeb9ca12194d456dc246` | R |
| 143 | [docs/design/analysis/task-page-timeline-phone-a.png](../../design/analysis/task-page-timeline-phone-a.png) | PNG; 1170 × 2532 | `b71f8420aa0e9e352046e98b07c78f713d1f2b34c62041d273ec18859e25284a` | R |
| 144 | [docs/design/analysis/task-page-timeline-phone-b.png](../../design/analysis/task-page-timeline-phone-b.png) | PNG; 1170 × 2532 | `43e89d113df6bec473527f94db96d803fd7e68b637eaa2d6d775c12d192e4e22` | R |
| 145 | [docs/design/analysis/task-page-timeline-phone-c.png](../../design/analysis/task-page-timeline-phone-c.png) | PNG; 1170 × 2532 | `17bdaae52c6eb1c257dba226d9c9b99b76169b4f29bf0d23bd9f0c3176b9d511` | R |
| 146 | [docs/design/analysis/task-page-timeline-phone-d.png](../../design/analysis/task-page-timeline-phone-d.png) | PNG; 1170 × 2532 | `8fc4659bb2e50a00ed1b4970c47b9471c05ff058b3cc51a97a092aa2b4cd5a4d` | R |
| 147 | [docs/design/analysis/task-page-timeline-wide-a.png](../../design/analysis/task-page-timeline-wide-a.png) | PNG; 1280 × 800 | `9268ad3f8223775c272f211df031c437ca1ac98285d011ad6d323a535314461b` | R |
| 148 | [docs/design/analysis/task-page-timeline-wide-b.png](../../design/analysis/task-page-timeline-wide-b.png) | PNG; 1280 × 800 | `553d56ef5291ea03877b528a83ce2bbdc99e870dd8f001bb6d0c2f893fe44ba4` | R |
| 149 | [docs/design/analysis/task-page-timeline-wide-c.png](../../design/analysis/task-page-timeline-wide-c.png) | PNG; 1280 × 800 | `855efa21798cd14713879a6e5bdea26b4d0e4d1eb2a04703763b809aaf33b8c7` | R |
| 150 | [docs/design/analysis/task-page-timeline-wide-d.png](../../design/analysis/task-page-timeline-wide-d.png) | PNG; 1280 × 800 | `3dc3f1eaa194aaeb238c1c658ce23357e0773bd9693531f9de3f580f2924198d` | R |
| 151 | [docs/design/analysis/task-timeline-fixture-phone-dark.png](../../design/analysis/task-timeline-fixture-phone-dark.png) | PNG; 1170 × 2532 | `d4445ac3520922736a1a15665294d1866d41a90e70d6179b64f4c234f70d4b9b` | R |
| 152 | [docs/design/analysis/task-timeline-fixture-phone-light.png](../../design/analysis/task-timeline-fixture-phone-light.png) | PNG; 1170 × 2532 | `642f4d1887063e6f219dc9c1b2e7b9c42c51673c2b7b7fd581ea7536a4820780` | R |
| 153 | [docs/design/analysis/task-timeline-fixture-wide-dark.png](../../design/analysis/task-timeline-fixture-wide-dark.png) | PNG; 1280 × 900 | `79af3e7008fa893e09f9c0690b9263d07d869efca16d4295c08efc923d8b408d` | R |
| 154 | [docs/design/analysis/task-timeline-fixture-wide-light.png](../../design/analysis/task-timeline-fixture-wide-light.png) | PNG; 1280 × 900 | `d8913c3003e3b6678394f3b6965b8675127b3e73fed7bd26e97e435642eb6b52` | R |
| 155 | [docs/design/analysis/task-timeline-week-phone-dark.png](../../design/analysis/task-timeline-week-phone-dark.png) | PNG; 1170 × 2532 | `92907700fe1f5bf409121925021308973136b2d2e91938316be7ae6d0723262b` | R |
| 156 | [docs/design/analysis/task-timeline-week-phone-light.png](../../design/analysis/task-timeline-week-phone-light.png) | PNG; 1170 × 2532 | `2f7f58548fccb57904fa93317db438ad71360c988d3c558e938bf625fff9a96b` | R |
| 157 | [docs/design/analysis/task-timeline-week-wide-dark.png](../../design/analysis/task-timeline-week-wide-dark.png) | PNG; 1280 × 900 | `2b04f08fb19855d7869ee00617c1bada66617dcea1752c82aa30898c8bb2bd30` | R |
| 158 | [docs/design/analysis/task-timeline-week-wide-light.png](../../design/analysis/task-timeline-week-wide-light.png) | PNG; 1280 × 900 | `1adbd59d059e4b439b441a4e5308868ac28a89eb9ab6e515583486792e4df815` | R |
| 159 | [docs/design/analysis/timeline-burst/a-wide.webp](../../design/analysis/timeline-burst/a-wide.webp) | WEBP; 1280 × 1200 | `f0fe4a9e3f117219747c1b699895da1386aaa2f4f0cbe8c1d301198b90f7d1a2` | H |
| 160 | [docs/design/analysis/timeline-burst/b-wide.webp](../../design/analysis/timeline-burst/b-wide.webp) | WEBP; 1280 × 1200 | `f69dcf3c532ca034786b34e66a914ceb4b5008ddc016e5af64c4ceb860477bd7` | H |
| 161 | [docs/design/analysis/timeline-burst/c-phone-all.webp](../../design/analysis/timeline-burst/c-phone-all.webp) | WEBP; 1170 × 2532 | `9174f44da2864f66465652cc737ac762e3da096c0e041143ec6093c02f86225e` | R |
| 162 | [docs/design/analysis/timeline-burst/c-wide-dark.webp](../../design/analysis/timeline-burst/c-wide-dark.webp) | WEBP; 1280 × 1200 | `ddd947b0de8bd75a1f49b3aa3e3d9140b77fdfc69c4aebe95485c4fd28052f1f` | H |
| 163 | [docs/design/analysis/timeline-burst/c-wide.webp](../../design/analysis/timeline-burst/c-wide.webp) | WEBP; 1280 × 1200 | `a32cdc161a7f594c44f099f0086ef7d83be92db3e5c9e41a2768ec12ea924d4c` | H |
| 164 | [docs/design/analysis/timeline-burst/phone-all.webp](../../design/analysis/timeline-burst/phone-all.webp) | WEBP; 1170 × 2532 | `203a6de608155e872f50c1ee5e3691edcbce557d3b92ba456d30588ba4d06282` | R |
| 165 | [docs/design/analysis/timeline-orchestrator/before-phone.png](../../design/analysis/timeline-orchestrator/before-phone.png) | PNG; 1170 × 2532 | `3520c73d637fc56329ef89fcffd24e07e49ff2edb0f8fc353659e0d3822340e1` | R |
| 166 | [docs/design/analysis/timeline-orchestrator/before-wide.png](../../design/analysis/timeline-orchestrator/before-wide.png) | PNG; 1280 × 800 | `f06a70daf3c5aa58f4b85ea2efa0122664ca06d0b011c99867cc672724856dfc` | R |
| 167 | [docs/design/analysis/timeline-orchestrator/proposed-direction-wide.png](../../design/analysis/timeline-orchestrator/proposed-direction-wide.png) | PNG; 1280 × 1183 | `15ee88fc5a9b7e96196921c268503d7c8313650b4114492375ee8eb6ab58b186` | R |
| 168 | [docs/design/analysis/timeline-orchestrator/proposed-phone-all.png](../../design/analysis/timeline-orchestrator/proposed-phone-all.png) | PNG; 1170 × 4791 | `a56e2f3b94151dc9ab2dd13b644982e63c974b1e981040978f0b12a9f133cc84` | R |
| 169 | [docs/design/analysis/timeline-orchestrator/proposed-phone.png](../../design/analysis/timeline-orchestrator/proposed-phone.png) | PNG; 1170 × 3585 | `0724f4b40774fb35a9d05ef2d11f20224b7d6ba16f0e294222668ff101b9b653` | R |
| 170 | [docs/design/analysis/timeline-orchestrator/proposed-wide.png](../../design/analysis/timeline-orchestrator/proposed-wide.png) | PNG; 1280 × 1183 | `f1748cc8ab14f8a6d5b83b85234906d7e785503a1471cad4d65d3665e17cc8cc` | R |
| 171 | [docs/product/pictures/queue-phone.png](../../product/pictures/queue-phone.png) | PNG; 1170 × 2532 | `1241a1d50458a1bc79cd3365254fcfb66e59cd61e7cdf26bbfb8f877b996f32f` | R |
| 172 | [docs/product/pictures/queue-topic-wide.png](../../product/pictures/queue-topic-wide.png) | PNG; 1280 × 800 | `cb8e87c273ca822677e50be87bc4d09905e2d65ac487c787ec6c4e0dc297e4c0` | R |
| 173 | [docs/product/pictures/queue-wide.png](../../product/pictures/queue-wide.png) | PNG; 1280 × 800 | `9ffb1cfe9ea5043ad9bd7f009c93d9491b8a340f8fa8d3ac7a5d753672c3da32` | R |
| 174 | [docs/product/pictures/register-wide.png](../../product/pictures/register-wide.png) | PNG; 1280 × 800 | `f47f02e9331658956e21157cd8d7b6c110cfcd9525183cf56e712d5f384f1acc` | R |
