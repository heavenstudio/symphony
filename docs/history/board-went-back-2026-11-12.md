# The board that went back on 2026-11-12

Evidence, not reference: how [A board that went back](../workflow/TASKS.md#a-board-that-went-back) was first
done by hand, with the numbers of that day. The cause is on the board in
`tools/board-database-lost-its-log`.

On 2026-11-12 the consuming project's board store came back three days old: at schema 13, where it
had been 16. Its data folder was cloned while the server on the older state still ran, as
`sf-tasks/data.snapshot-2026-11-12-0845` in the consuming project's git common dir; nothing was ever
started on that folder, and every try below worked on a clone of it (`cp -c -R`).

- **No hold.** The first swap failed: the launcher started the database again on the old path
  between the stop and the rename, and that server then wrote and deleted sstables by path inside
  the recovered folder.
- **The manifest.** Its log number said 18 (`0x12`, byte 17 of the file) and its last sequence
  17491149, while wal 17, 3.3 GB written until 08:34, held sequences up to 21049118: the engine had
  moved past a segment it had not applied. wal 18 (22 kB) was the log of the server started on the
  older state; its sequences start after the manifest's last, inside wal 17's.
- **The patch.** On the clone, the log number set to 17
  (`printf '\021' | dd of=CLONE/manifest/00000000000000000000.manifest bs=1 seek=17 count=1
  conv=notrunc`, byte 17 alone being enough below 256), and `wal/00000000000000000018.wal`
  removed.
- **The replay.** A private server with a memtable above wal 17's size replayed it in 36 s.
- **The check.** The clone read schema 16, 2678 tasks and 59806 messages, the newest at 08:34:44,
  where the older state read schema 13, 2508 tasks and 56330 messages.
