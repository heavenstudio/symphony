# The client's first visits, move by move

What each change of the client did to the pages' first visits on the browser target's fixture, as
the client's guide recorded it under its Budget until every page kind had moved to the React
client, each against main's build of its day. Superseded: the guide gives today's figures alone
([its Budget](../../apps/symphony-ui/AGENTS.md)).

On the fixture a first visit to `/tasks` took, while this client drew the board, 261,350
bytes, 38,650 under the cap: the board WASM 230,907, its glue 11,086, the shared style sheet 9,175,
the shell 6,151 (the client cache's store about 1,430 of it), the HTML 1,142, API
replies 2,804 (`/api/machine-work` 247 of them) and the feed's head and
opening comment 85. `/` takes 232,052; cached returns take 1,986 and 2,397.
Drawing a return visit from the device costs the first visit its code: about
6.9 KB of home's WASM (`wasm_size` on the named copies); about 13.6 KB of the task page's and
`/agents`' kind, 11.0 KB of it the task page's draw, checks and retries (`task_live`), and
the documents' 2.2 KB. The pages' ghosts and what they keep (Loading, below) cost it
about 2.4 KB of `/`, 13.0 KB of a task page and
`/agents` and 6.8 KB of the documents. Documents
drawn from the device, an entrypoint's page kept and the reads ahead
(`perf/client-cache/documents` and `/ahead`, against main's build at 8ef025d) cost every page
about 0.9 KB of the shell and the documents' kind about 30.4 KB (`wasm_size` on the named
copies: `docs_live`'s draw from the device, reads and keeping 9.0 KB, the cache's bindings
3.5 KB, and the maps it adds with their trees and sorts about 9.8 KB of `alloc` and
`core::slice`, a stable sort compiled for each `collect` into a map): `/tasks` takes 296,924 bytes
(3,076 under the cap, from 295,902), `/` 237,202 (from 236,322) and `/docs/` 505,402 (from
474,965); a document's return takes 1,633 (from 2,371), the budget's stand-in naming no cursor
on its document reads, so its outline's `304` is read again whole. The reads ahead start only
once the page's own reads have answered (`cache_ahead.rs`) and go marked as prefetches, so the
budget counts them outside the first visit, with the warmed modules (`prefetched_bytes`): on
the stand-in's links page, a task and seven documents, 5,511 bytes on the wire. The task
pages, `/agents` and Reports (until it left for the web client, below) became kinds of the
program (Build, above; `perf/client-shared-runtime/task-pages-program`, against main's build at
5352c2d), which costs every page the shell's 38 bytes that name its chunks (`/tasks` 295,939 from 295,902, `/`
236,351 from 236,322): a first visit to one takes the program's script (17,907 bytes on the
wire) and main module (216,782) and its kind's chunk (the task pages' 315,343, Reports'
48,996), so `/agents` and a task page take 572,269 and 572,862 (from 559,770 and 560,363,
2,241 on a return that finds a task unchanged, its page and thread each a `304` of 82) and
`/reports` 306,609 (from 558,271), and a move between them takes the target's chunk alone. A
task page's timeline, a chunk of the same program, comes once the page has drawn and is
idle: 54,357 bytes outside its first visit (from 202,226; its chunk 50,648, style sheet 2,604
and first read 1,105), and `/agents` never loads it. Once a page has its data and is idle,
the shell warms the chat's and the menus' chunks, with the program's script and main module
where the page is not one of the program's: 609,120 bytes outside home's first visit (from
748,701), about 374,600 outside a task page's, and the chat's first open transfers nothing
more. The media layer leaving the main module for a chunk of its own
(`perf/client-shared-runtime/trim-shared-part`, against main's build at 2eec23a) takes it to
207,514 bytes on the wire (from 216,490; 492,237 bytes of wasm from 515,290), and the task
pages' chunk to 318,193 (from 316,744), the code they shared with it now theirs: `/agents` and
a task page take 567,190 and 567,783 (from 574,579 and 575,172), `/reports` 298,785 (from
307,557), and every other page 71 to 94 bytes more, about 60 of them the shell's for a media
chunk that does not load (`/tasks` 299,205 from 299,134). The chunk, 31,502 bytes of wasm and
12,903 on the wire, comes only once a page shows media: a task page that shows media on its
first screen takes it too, 580,686 bytes, 5,514 more than main's, and one more round trip.
The timeline then takes 55,938 outside a task page's first visit (from 55,208), and the warmed
chat and menus 604,545 outside home's (from 611,040) and about 377,800 outside a task page's:
the chunks that share code with the media layer (the task pages', the chat's, the menus', the
timeline's and Reports') each hold a copy of it, 4,646 bytes on the wire in all, 19,295 bytes
a build. The keyboard's key names, `document::eval` with serde_json, and float formatting stay
in the main module: dioxus-web makes at launch the event converter and the document that
reach them, and the splitter keeps there every function of a table the main module reaches
([the analysis](../analysis/client-shared-runtime.md#building-a-in-order)). The
client reading none of a task's plugin fields (`symphony_api::Task::fields`, which it did
not draw: a feature of `symphony-api`'s that `Cargo.toml` turned on for the wasm32 build
alone, gone with the task page's move, when no page of the program read a task) and the media
layer's notice moving to the program's script
(`perf/tasks-first-visit-gets-room`, against main's build at 029d581) take `/tasks` to
281,222 bytes (from 299,662: the board's WASM 247,735 from 266,112, the shell 7,880 from
7,928), `/agents` and a task page to 548,973 and 549,566 (from 567,204 and 567,797, the task
pages' chunk 18 KB less) and `/reports` to 298,951 (from 298,790), the program's script now
holding the notice; every other page stays within 50 bytes of main's. Reports moving to the
web client (`app/client-moves-to-react/reports`, against main's build at 8bd7918) takes
`/reports` to 105,591 bytes (from 302,156), its entry and the chunk of what it shares with the
queue's (React and the frame, 74,471 on the wire), so `/queue` takes 103,264 (from 101,789) and
a move between the two fetches the other's entry alone; the program, Reports' chunk gone, takes
`/agents` and a task page to 548,597 and 549,190 (from 549,285 and 549,878) and the warmed chat
and menus to 603,539 outside home's (from 604,282); every other page stays within 12 bytes of
main's. Home moving to the web client (`app/client-moves-to-react/home`, against main's build
at 6f89f02) takes `/` to 108,315 bytes (from 237,728) and its cap to 125,000: its entry, its
sheet (the media sheet's copy), the chunk the three web kinds share and one it shares with the
queue (the board's feed and keep, 3,129 on the wire), so `/queue` takes 104,965 (from 103,296)
and `/reports` 106,455 (from 105,617); the Dioxus kinds, home's code gone from the shared
sources, take `/tasks` to 280,848 (from 281,212), `/agents` and a task page to 548,042 and
548,635 (from 548,627 and 549,220) and `/docs/` to 506,544 (from 507,082); the warmed chat and
menus take 604,993 outside home's (from 603,664), the media sheet home no longer holds under
the program's name among them. The documents moving to the web client
(`app/client-moves-to-react/documents-page`, against main's build at 98a254b) take `/docs/` to
135,757 bytes (from 506,529) and a document to 137,374 (from 508,146): their entry (23,707 on
the wire), their sheet (5,393), the media layer's sheet, the chunk the four web kinds share
(76,340, 767 more than main's, the documents' share of what it holds), the media layer's
chunk they now share with home (4,272), the board's feed and keep (3,397) and the cards' Later
they share with the queue (`postpone.tsx`, 983), with the same reads as the Dioxus page. Home
takes 111,400 (from 108,364): its entry 4,124 (from 6,536), the media layer's chunk, and its
task chip a chunk of 371 it shares with the queue alone, so `/queue` takes 106,825 (from
105,004) and `/reports` 107,236 (from 106,470); the Dioxus kinds, the documents' code gone from
the shared sources, take `/tasks` to 280,543 (from 280,812) and `/agents` and a task page to
545,491 and 546,084 (from 547,991 and 548,584), and the warmed chat and menus 602,819 outside
home's (from 604,967). The task board's move to the React client
(`app/client-moves-to-react/task-board`, against main's build at a220cce) takes `/tasks` to
116,215 bytes (from 280,541) and its cap to 150,000: its entry (12,779 on the wire), the chunk
the five web kinds share (76,342), the board's feed and keep it shares with home, the queue and
the documents (3,532, 135 more than main's) and the select field it shares with the documents
(`select.tsx`, 1,249), where the board's WASM and glue were. Home takes 111,528 (from 111,400),
`/queue` 106,971 (from 106,825) and `/reports` 107,258 (from 107,235); `/docs/` and a document
take 136,580 and 138,197 (from 135,756 and 137,373), their entry 558 less with the select field
a chunk of its own; every other page stays within 150 bytes of main's. The shell preloading the
chunks each web kind's entry imports beside it (`app/client-moves-to-react/shared-chunk-preload-and-read-signal`,
against main's build at ed6868a) costs every page the 17 names it adds to the manifest, 153
bytes of the shell on the wire, for the same requests, a kind's chunks asked for with its entry
rather than once it has come: `/` takes 111,667 (from 111,515), `/tasks` 116,371 (from
116,202), `/queue` 107,113 (from 106,958), `/reports` 107,396 (from 107,244), `/docs/` and a
document 136,723 and 138,340 (from 136,566 and 138,183), the returns within a byte. The agents
page's move to the React client (`app/client-moves-to-react/agents-page`, against main's build
at f56bde5) takes `/agents` to 113,775 bytes (from 545,758): its entry (7,324 on the wire), the
chunk the six web kinds share (76,404), the board's feed and keep (3,530) and the chunk it
shares with the task board (4,734: the card, the task readers, the machine work's phases and the
read journal, `journal.ts`), where the program's script, main module and the task pages' chunk
were. `/tasks` takes 117,463 (from 116,357), its entry 9,066 (from 12,794) with that chunk
apart, which costs it the compression of one file and the names the two pass; a task page
takes 512,598 (from 546,351), the task pages' chunk without the agents' code; every other page
stays within 120 bytes of main's. A task's page's move to the React client
(`app/client-moves-to-react/task-page`, against main's build at f56bde5) takes its first visit
to 140,089 bytes (from 546,351) and gives it a cap of 160,000: its entry (16,823 on the wire)
and sheet (1,460), the chunk the web kinds share and the pieces it shares with the board and the
documents, where the program's WASM, glue and the task pages' chunk were; its timeline, a chunk
of the web client's, takes 12,787 after it (from 56,385), its return 2,241 (from 2,240). No page
is the program's after both: the chat and the menus are all it builds.
